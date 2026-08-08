#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use proptest::prelude::*;
use psyche_core::contracts::execution::{
    AdoptionState, CancellationState, ExecutionBinding, TerminationRequestCorrelation,
};
use psyche_core::contracts::{
    CanonicalDocument, ContractError, Intent, RecordKind, SchemaKind, SchemaVersion,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, ExecutionCorrelation, ExecutionRequestInput, PortError,
    ReconciliationDisposition, ReconciliationRequest,
};
use psyche_store::{
    IngestOutcome, QuarantineId, QuarantineReasonCode, QuarantineResolution,
    QuarantineResolutionCode, ResolveQuarantineOutcome, Store, StoreError, Transition,
};
use psyche_test_support::{
    CovenConformanceFixture, CovenFaultPoint, DurableDispositionKind, RedispatchEligibility,
    scripted_fixture,
};
use serde_json::{Map, json};
use tempfile::TempDir;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

const LAUNCH_GOLDEN: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json");
const INPUT_GOLDEN: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-input.json");

#[derive(Debug, Clone)]
enum FoundationOperation {
    Insert { slot: u8 },
    IdenticalReinsert { slot: u8 },
    ConflictingReinsert { slot: u8 },
    InvalidDirectInsertSchema { slot: u8 },
    InvalidDirectInsertFieldId { slot: u8 },
    InsertInitialBinding { slot: u8 },
    AppendNextBindingRevision { slot: u8 },
    ReplayBindingRevision { slot: u8, selector: u8 },
    InvalidBindingRevision { slot: u8, mutation: u8 },
    AppendNextTransition { slot: u8 },
    AppendDuplicateVersion { slot: u8 },
    InvalidTransitionDigest { slot: u8 },
    InvalidTransitionKind { slot: u8 },
    Quarantine { slot: u8 },
    ResolveQuarantineFirst { slot: u8 },
    ResolveQuarantineReplay { slot: u8 },
    ResolveQuarantineUnknown,
    ResolveQuarantineStale { slot: u8 },
    ResolveQuarantineConflict { slot: u8 },
    Prune { future_cutoff: bool },
    Checkpoint,
    Reopen,
}

impl Arbitrary for FoundationOperation {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        let slot = 0_u8..2;
        prop_oneof![
            4 => slot.clone().prop_map(|slot| Self::Insert { slot }),
            2 => slot.clone().prop_map(|slot| Self::IdenticalReinsert { slot }),
            2 => slot.clone().prop_map(|slot| Self::ConflictingReinsert { slot }),
            2 => slot.clone().prop_map(|slot| Self::InvalidDirectInsertSchema { slot }),
            2 => slot.clone().prop_map(|slot| Self::InvalidDirectInsertFieldId { slot }),
            3 => slot.clone().prop_map(|slot| Self::InsertInitialBinding { slot }),
            4 => slot.clone().prop_map(|slot| Self::AppendNextBindingRevision { slot }),
            2 => (slot.clone(), any::<u8>())
                .prop_map(|(slot, selector)| Self::ReplayBindingRevision { slot, selector }),
            4 => (slot.clone(), 0_u8..16)
                .prop_map(|(slot, mutation)| Self::InvalidBindingRevision { slot, mutation }),
            4 => slot.clone().prop_map(|slot| Self::AppendNextTransition { slot }),
            2 => slot.clone().prop_map(|slot| Self::AppendDuplicateVersion { slot }),
            2 => slot.clone().prop_map(|slot| Self::InvalidTransitionDigest { slot }),
            2 => slot.clone().prop_map(|slot| Self::InvalidTransitionKind { slot }),
            3 => slot.clone().prop_map(|slot| Self::Quarantine { slot }),
            3 => slot.clone().prop_map(|slot| Self::ResolveQuarantineFirst { slot }),
            2 => slot.clone().prop_map(|slot| Self::ResolveQuarantineReplay { slot }),
            1 => Just(Self::ResolveQuarantineUnknown),
            2 => slot.clone().prop_map(|slot| Self::ResolveQuarantineStale { slot }),
            2 => slot.clone().prop_map(|slot| Self::ResolveQuarantineConflict { slot }),
            2 => any::<bool>().prop_map(|future_cutoff| Self::Prune { future_cutoff }),
            1 => Just(Self::Checkpoint),
            2 => Just(Self::Reopen),
        ]
        .boxed()
    }
}

impl OperationOutcome {
    fn must_preserve_logical_state(&self) -> bool {
        matches!(
            self,
            Self::AlreadyPresent
                | Self::Conflict
                | Self::Invalid
                | Self::NoTarget
                | Self::AlreadyResolved
                | Self::NotFound
                | Self::StaleResolution
                | Self::ResolutionConflict
                | Self::Checkpointed
                | Self::Reopened
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
enum OperationOutcome {
    Applied,
    AlreadyPresent,
    Conflict,
    Invalid,
    NoTarget,
    Quarantined,
    Resolved,
    AlreadyResolved,
    NotFound,
    StaleResolution,
    ResolutionConflict,
    Pruned(u64),
    Checkpointed,
    Reopened,
}

#[derive(Debug, Clone, PartialEq)]
struct QuarantineObservation {
    payload_digest: Sha256Digest,
    reason: QuarantineReasonCode,
    resolution_code: Option<QuarantineResolutionCode>,
    resolved_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq)]
struct AuditObservation {
    event_code: String,
    created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
struct FoundationSnapshot {
    records: BTreeMap<u8, CanonicalDocument>,
    record_digests: BTreeMap<u8, Sha256Digest>,
    binding_revisions: BTreeMap<u8, Vec<ExecutionBinding>>,
    binding_digests: BTreeMap<u8, Vec<Sha256Digest>>,
    transitions: BTreeMap<u8, Vec<Transition>>,
    quarantines: BTreeMap<u8, QuarantineObservation>,
    audit_events: Vec<AuditObservation>,
    total_record_count: u64,
    transition_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct FoundationStep {
    outcome: OperationOutcome,
    snapshot: FoundationSnapshot,
}

#[derive(Debug, Clone)]
struct ModelQuarantine {
    payload_digest: Sha256Digest,
    reason: QuarantineReasonCode,
    resolution: Option<QuarantineResolution>,
}

#[derive(Debug, Default)]
struct FoundationModel {
    records: BTreeMap<u8, CanonicalDocument>,
    bindings: BTreeMap<u8, Vec<ExecutionBinding>>,
    transitions: BTreeMap<u8, Vec<Transition>>,
    quarantines: BTreeMap<u8, ModelQuarantine>,
    audit_events: Vec<AuditObservation>,
}

impl FoundationModel {
    fn apply(&mut self, operation: FoundationOperation) -> FoundationStep {
        let before = self.snapshot();
        let outcome = match operation.clone() {
            FoundationOperation::Insert { slot } => {
                let candidate = fixture_intent(slot, false);
                match self.records.get(&slot) {
                    None => {
                        self.records.insert(slot, candidate);
                        OperationOutcome::Applied
                    }
                    Some(stored) if stored == &candidate => OperationOutcome::AlreadyPresent,
                    Some(_) => OperationOutcome::Conflict,
                }
            }
            FoundationOperation::IdenticalReinsert { slot } => {
                if self.records.contains_key(&slot) {
                    OperationOutcome::AlreadyPresent
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::ConflictingReinsert { slot } => {
                if self.records.contains_key(&slot) {
                    OperationOutcome::Conflict
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::InvalidDirectInsertSchema { .. }
            | FoundationOperation::InvalidDirectInsertFieldId { .. } => OperationOutcome::Invalid,
            FoundationOperation::InsertInitialBinding { slot } => {
                let candidate = fixture_binding(slot);
                match self.bindings.get(&slot) {
                    None => {
                        self.bindings.insert(slot, vec![candidate]);
                        OperationOutcome::Applied
                    }
                    Some(history) if history.first() == Some(&candidate) => {
                        OperationOutcome::AlreadyPresent
                    }
                    Some(_) => OperationOutcome::Conflict,
                }
            }
            FoundationOperation::AppendNextBindingRevision { slot } => {
                let Some(history) = self.bindings.get_mut(&slot) else {
                    return self.step_with_preservation(
                        operation,
                        before,
                        OperationOutcome::NoTarget,
                    );
                };
                let next = next_binding(history.last().expect("history is nonempty"), slot);
                history.push(next);
                OperationOutcome::Applied
            }
            FoundationOperation::ReplayBindingRevision { slot, selector } => {
                if self
                    .bindings
                    .get(&slot)
                    .is_some_and(|history| !history.is_empty())
                {
                    let _ = selector;
                    OperationOutcome::AlreadyPresent
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::InvalidBindingRevision { slot, .. } => {
                if self.bindings.contains_key(&slot) {
                    OperationOutcome::Conflict
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::AppendNextTransition { slot } => {
                let history = self.transitions.entry(slot).or_default();
                history.push(next_transition(history, slot));
                OperationOutcome::Applied
            }
            FoundationOperation::AppendDuplicateVersion { slot } => {
                if self
                    .transitions
                    .get(&slot)
                    .is_some_and(|history| !history.is_empty())
                {
                    OperationOutcome::Conflict
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::InvalidTransitionDigest { .. }
            | FoundationOperation::InvalidTransitionKind { .. } => OperationOutcome::Invalid,
            FoundationOperation::Quarantine { slot } => {
                self.quarantines.entry(slot).or_insert_with(|| {
                    let rejected = psyche_core::contracts::RejectedDocument::from_decode_error(
                        &unknown_major_bytes(slot),
                        ContractError::UnsupportedMajor {
                            found: 2,
                            supported: 1,
                        },
                    );
                    ModelQuarantine {
                        payload_digest: rejected.payload_digest,
                        reason: QuarantineReasonCode::UnsupportedMajor,
                        resolution: None,
                    }
                });
                OperationOutcome::Quarantined
            }
            FoundationOperation::ResolveQuarantineFirst { slot } => {
                let Some(quarantine) = self.quarantines.get_mut(&slot) else {
                    return self.step_with_preservation(
                        operation,
                        before,
                        OperationOutcome::NotFound,
                    );
                };
                let requested = first_resolution(slot);
                if quarantine.resolution.as_ref() == Some(&requested) {
                    OperationOutcome::AlreadyResolved
                } else if quarantine.resolution.is_some() {
                    OperationOutcome::ResolutionConflict
                } else {
                    quarantine.resolution = Some(requested.clone());
                    self.audit_events.push(AuditObservation {
                        event_code: "quarantine_resolved".to_owned(),
                        created_at: requested.resolved_at,
                    });
                    OperationOutcome::Resolved
                }
            }
            FoundationOperation::ResolveQuarantineReplay { slot } => {
                if self
                    .quarantines
                    .get(&slot)
                    .and_then(|record| record.resolution.as_ref())
                    .is_some()
                {
                    OperationOutcome::AlreadyResolved
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::ResolveQuarantineUnknown => OperationOutcome::NotFound,
            FoundationOperation::ResolveQuarantineStale { slot } => {
                if self.quarantines.contains_key(&slot) {
                    OperationOutcome::StaleResolution
                } else {
                    OperationOutcome::NotFound
                }
            }
            FoundationOperation::ResolveQuarantineConflict { slot } => {
                if self
                    .quarantines
                    .get(&slot)
                    .and_then(|record| record.resolution.as_ref())
                    .is_some()
                {
                    OperationOutcome::ResolutionConflict
                } else {
                    OperationOutcome::NoTarget
                }
            }
            FoundationOperation::Prune { future_cutoff } => {
                let before_count = self.quarantines.len();
                if future_cutoff {
                    self.quarantines
                        .retain(|_, record| record.resolution.is_none());
                }
                OperationOutcome::Pruned(
                    u64::try_from(before_count.saturating_sub(self.quarantines.len())).unwrap(),
                )
            }
            FoundationOperation::Checkpoint => OperationOutcome::Checkpointed,
            FoundationOperation::Reopen => OperationOutcome::Reopened,
        };
        self.step_with_preservation(operation, before, outcome)
    }

    fn step_with_preservation(
        &self,
        operation: FoundationOperation,
        before: FoundationSnapshot,
        outcome: OperationOutcome,
    ) -> FoundationStep {
        let snapshot = self.snapshot();
        if outcome.must_preserve_logical_state() {
            assert_eq!(snapshot, before, "{operation:?}");
        }
        FoundationStep { outcome, snapshot }
    }

    fn snapshot(&self) -> FoundationSnapshot {
        let record_digests = self
            .records
            .iter()
            .map(|(slot, document)| (*slot, digest(document).unwrap()))
            .collect();
        let binding_digests = self
            .bindings
            .iter()
            .map(|(slot, history)| {
                (
                    *slot,
                    history
                        .iter()
                        .map(|binding| digest(binding).unwrap())
                        .collect(),
                )
            })
            .collect();
        FoundationSnapshot {
            records: self.records.clone(),
            record_digests,
            binding_revisions: self.bindings.clone(),
            binding_digests,
            transitions: self.transitions.clone(),
            quarantines: self
                .quarantines
                .iter()
                .map(|(slot, record)| {
                    (
                        *slot,
                        QuarantineObservation {
                            payload_digest: record.payload_digest.clone(),
                            reason: record.reason,
                            resolution_code: record
                                .resolution
                                .as_ref()
                                .map(|resolution| resolution.code),
                            resolved_at: record
                                .resolution
                                .as_ref()
                                .map(|resolution| resolution.resolved_at),
                        },
                    )
                })
                .collect(),
            audit_events: self.audit_events.clone(),
            total_record_count: u64::try_from(self.records.len() + self.bindings.len()).unwrap(),
            transition_count: self
                .transitions
                .values()
                .map(Vec::len)
                .sum::<usize>()
                .try_into()
                .unwrap(),
        }
    }
}

struct FoundationStore {
    store: Store,
    path: PathBuf,
    quarantine_ids: BTreeMap<u8, QuarantineId>,
}

fn test_store() -> (FoundationStore, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    let store = Store::open(&path).unwrap();
    (
        FoundationStore {
            store,
            path,
            quarantine_ids: BTreeMap::new(),
        },
        dir,
    )
}

fn apply_to_store(harness: &mut FoundationStore, operation: FoundationOperation) -> FoundationStep {
    let before = store_snapshot(harness);
    let outcome = match operation.clone() {
        FoundationOperation::Insert { slot } => {
            match harness.store.insert(&fixture_intent(slot, false)) {
                Ok(()) if before.records.contains_key(&slot) => OperationOutcome::AlreadyPresent,
                Ok(()) => OperationOutcome::Applied,
                Err(StoreError::RecordConflict { .. }) => OperationOutcome::Conflict,
                other => panic!("unexpected insert result: {other:?}"),
            }
        }
        FoundationOperation::IdenticalReinsert { slot } => {
            if before.records.contains_key(&slot) {
                harness.store.insert(&fixture_intent(slot, false)).unwrap();
                OperationOutcome::AlreadyPresent
            } else {
                OperationOutcome::NoTarget
            }
        }
        FoundationOperation::ConflictingReinsert { slot } => {
            if before.records.contains_key(&slot) {
                assert!(matches!(
                    harness.store.insert(&fixture_intent(slot, true)),
                    Err(StoreError::RecordConflict { .. })
                ));
                OperationOutcome::Conflict
            } else {
                OperationOutcome::NoTarget
            }
        }
        FoundationOperation::InvalidDirectInsertSchema { slot } => {
            let mut invalid = match fixture_intent(slot, false) {
                CanonicalDocument::Intent(intent) => intent,
                _ => unreachable!(),
            };
            invalid.schema_version = schema("psyche.graph.v1");
            assert!(matches!(
                harness.store.insert(&CanonicalDocument::Intent(invalid)),
                Err(StoreError::Contract(ContractError::SchemaMismatch { .. }))
            ));
            OperationOutcome::Invalid
        }
        FoundationOperation::InvalidDirectInsertFieldId { slot } => {
            let mut invalid = match fixture_intent(slot, false) {
                CanonicalDocument::Intent(intent) => intent,
                _ => unreachable!(),
            };
            invalid.intent_id = record_id(RecordKind::Graph, slot);
            assert!(matches!(
                harness.store.insert(&CanonicalDocument::Intent(invalid)),
                Err(StoreError::Contract(ContractError::WrongRecordKind { .. }))
            ));
            OperationOutcome::Invalid
        }
        FoundationOperation::InsertInitialBinding { slot } => {
            let before_len = before.binding_revisions.get(&slot).map_or(0, Vec::len);
            match harness
                .store
                .insert(&CanonicalDocument::ExecutionBinding(fixture_binding(slot)))
            {
                Ok(()) if before_len == 0 => OperationOutcome::Applied,
                Ok(()) => OperationOutcome::AlreadyPresent,
                Err(StoreError::ExecutionBindingRevisionConflict { .. }) => {
                    OperationOutcome::Conflict
                }
                other => panic!("unexpected initial binding result: {other:?}"),
            }
        }
        FoundationOperation::AppendNextBindingRevision { slot } => {
            let history = harness
                .store
                .execution_binding_revisions(&attempt_id(slot))
                .unwrap();
            if let Some(latest) = history.last() {
                harness
                    .store
                    .insert(&CanonicalDocument::ExecutionBinding(next_binding(
                        latest, slot,
                    )))
                    .unwrap();
                OperationOutcome::Applied
            } else {
                OperationOutcome::NoTarget
            }
        }
        FoundationOperation::ReplayBindingRevision { slot, selector } => {
            let history = harness
                .store
                .execution_binding_revisions(&attempt_id(slot))
                .unwrap();
            if history.is_empty() {
                OperationOutcome::NoTarget
            } else {
                let index = usize::from(selector) % history.len();
                harness
                    .store
                    .insert(&CanonicalDocument::ExecutionBinding(history[index].clone()))
                    .unwrap();
                OperationOutcome::AlreadyPresent
            }
        }
        FoundationOperation::InvalidBindingRevision { slot, mutation } => {
            let history = harness
                .store
                .execution_binding_revisions(&attempt_id(slot))
                .unwrap();
            if let Some(latest) = history.last() {
                let invalid = invalid_binding(latest, slot, mutation);
                assert!(matches!(
                    harness
                        .store
                        .insert(&CanonicalDocument::ExecutionBinding(invalid)),
                    Err(StoreError::ExecutionBindingRevisionConflict { .. })
                ));
                OperationOutcome::Conflict
            } else {
                OperationOutcome::NoTarget
            }
        }
        FoundationOperation::AppendNextTransition { slot } => {
            let history = harness.store.transitions(&attempt_id(slot)).unwrap();
            harness
                .store
                .append_transition(&next_transition(&history, slot))
                .unwrap();
            OperationOutcome::Applied
        }
        FoundationOperation::AppendDuplicateVersion { slot } => {
            let history = harness.store.transitions(&attempt_id(slot)).unwrap();
            if let Some(latest) = history.last() {
                let duplicate = Transition::new(
                    SchemaKind::ExecutionBinding,
                    attempt_id(slot),
                    latest.record_version,
                    latest.from_state.clone(),
                    format!("conflict_{}", latest.record_version),
                    latest.created_at + Duration::nanoseconds(1),
                )
                .unwrap();
                assert!(matches!(
                    harness.store.append_transition(&duplicate),
                    Err(StoreError::TransitionConflict { .. })
                ));
                OperationOutcome::Conflict
            } else {
                OperationOutcome::NoTarget
            }
        }
        FoundationOperation::InvalidTransitionDigest { slot } => {
            let history = harness.store.transitions(&attempt_id(slot)).unwrap();
            let mut invalid = next_transition(&history, slot);
            invalid.transition_digest = digest_of('f');
            assert!(matches!(
                harness.store.append_transition(&invalid),
                Err(StoreError::Contract(ContractError::DigestMismatch { .. }))
            ));
            OperationOutcome::Invalid
        }
        FoundationOperation::InvalidTransitionKind { slot } => {
            let history = harness.store.transitions(&attempt_id(slot)).unwrap();
            let mut invalid = next_transition(&history, slot);
            invalid.kind = SchemaKind::Intent;
            assert!(matches!(
                harness.store.append_transition(&invalid),
                Err(StoreError::Contract(ContractError::WrongRecordKind { .. }))
            ));
            OperationOutcome::Invalid
        }
        FoundationOperation::Quarantine { slot } => {
            let IngestOutcome::Quarantined { quarantine_id } =
                harness.store.ingest(&unknown_major_bytes(slot)).unwrap()
            else {
                panic!("unknown major must be quarantined")
            };
            harness.quarantine_ids.insert(slot, quarantine_id);
            OperationOutcome::Quarantined
        }
        FoundationOperation::ResolveQuarantineFirst { slot } => {
            let Some(id) = harness.quarantine_ids.get(&slot) else {
                return store_step(harness, operation, before, OperationOutcome::NotFound);
            };
            match harness
                .store
                .resolve_quarantine(id, &first_resolution(slot))
            {
                Ok(ResolveQuarantineOutcome::Resolved { .. }) => OperationOutcome::Resolved,
                Ok(ResolveQuarantineOutcome::AlreadyResolved { .. }) => {
                    OperationOutcome::AlreadyResolved
                }
                Err(StoreError::QuarantineResolutionConflict { .. }) => {
                    OperationOutcome::ResolutionConflict
                }
                Err(StoreError::QuarantineNotFound { .. }) => OperationOutcome::NotFound,
                other => panic!("unexpected first resolution result: {other:?}"),
            }
        }
        FoundationOperation::ResolveQuarantineReplay { slot } => {
            let Some(id) = harness.quarantine_ids.get(&slot) else {
                return store_step(harness, operation, before, OperationOutcome::NoTarget);
            };
            let Some(record) = harness.store.quarantine_record(id).unwrap() else {
                return store_step(harness, operation, before, OperationOutcome::NoTarget);
            };
            if record.resolution_code.is_none() {
                OperationOutcome::NoTarget
            } else {
                assert!(matches!(
                    harness
                        .store
                        .resolve_quarantine(id, &first_resolution(slot)),
                    Ok(ResolveQuarantineOutcome::AlreadyResolved { .. })
                ));
                OperationOutcome::AlreadyResolved
            }
        }
        FoundationOperation::ResolveQuarantineUnknown => {
            let unknown = QuarantineId::parse("qua_01J00000000000000000000000").unwrap();
            assert!(matches!(
                harness
                    .store
                    .resolve_quarantine(&unknown, &first_resolution(0)),
                Err(StoreError::QuarantineNotFound { .. })
            ));
            OperationOutcome::NotFound
        }
        FoundationOperation::ResolveQuarantineStale { slot } => {
            let Some(id) = harness.quarantine_ids.get(&slot) else {
                return store_step(harness, operation, before, OperationOutcome::NotFound);
            };
            match harness.store.resolve_quarantine(
                id,
                &QuarantineResolution {
                    code: QuarantineResolutionCode::ConfirmedInvalid,
                    resolved_at: OffsetDateTime::UNIX_EPOCH,
                },
            ) {
                Err(StoreError::InvalidQuarantineResolution { .. }) => {
                    OperationOutcome::StaleResolution
                }
                Err(StoreError::QuarantineNotFound { .. }) => OperationOutcome::NotFound,
                other => panic!("unexpected stale resolution result: {other:?}"),
            }
        }
        FoundationOperation::ResolveQuarantineConflict { slot } => {
            let Some(id) = harness.quarantine_ids.get(&slot) else {
                return store_step(harness, operation, before, OperationOutcome::NoTarget);
            };
            let Some(record) = harness.store.quarantine_record(id).unwrap() else {
                return store_step(harness, operation, before, OperationOutcome::NoTarget);
            };
            if record.resolution_code.is_none() {
                OperationOutcome::NoTarget
            } else {
                assert!(matches!(
                    harness.store.resolve_quarantine(
                        id,
                        &QuarantineResolution {
                            code: QuarantineResolutionCode::DuplicatePayload,
                            resolved_at: at("2101-01-01T00:00:00Z"),
                        },
                    ),
                    Err(StoreError::QuarantineResolutionConflict { .. })
                ));
                OperationOutcome::ResolutionConflict
            }
        }
        FoundationOperation::Prune { future_cutoff } => {
            let report = harness
                .store
                .prune(if future_cutoff {
                    at("2200-01-01T00:00:00Z")
                } else {
                    at("2000-01-01T00:00:00Z")
                })
                .unwrap();
            assert_eq!(report.execution_binding_revisions_deleted, 0);
            assert_eq!(report.transitions_deleted, 0);
            assert_eq!(report.audit_events_deleted, 0);
            assert_eq!(report.unresolved_quarantine_deleted, 0);
            OperationOutcome::Pruned(report.resolved_quarantine_deleted)
        }
        FoundationOperation::Checkpoint => {
            harness.store.checkpoint().unwrap();
            OperationOutcome::Checkpointed
        }
        FoundationOperation::Reopen => {
            harness.store = Store::open(&harness.path).unwrap();
            OperationOutcome::Reopened
        }
    };
    store_step(harness, operation, before, outcome)
}

fn store_step(
    harness: &FoundationStore,
    operation: FoundationOperation,
    before: FoundationSnapshot,
    outcome: OperationOutcome,
) -> FoundationStep {
    let snapshot = store_snapshot(harness);
    if outcome.must_preserve_logical_state() {
        assert_eq!(snapshot, before, "{operation:?}");
    }
    FoundationStep { outcome, snapshot }
}

fn store_snapshot(harness: &FoundationStore) -> FoundationSnapshot {
    let mut records = BTreeMap::new();
    let mut record_digests = BTreeMap::new();
    let mut binding_revisions = BTreeMap::new();
    let mut binding_digests = BTreeMap::new();
    let mut transitions = BTreeMap::new();
    let mut quarantines = BTreeMap::new();
    for slot in 0..2 {
        if let Some(document) = harness
            .store
            .load(SchemaKind::Intent, &intent_id(slot))
            .unwrap()
        {
            record_digests.insert(slot, digest(&document).unwrap());
            records.insert(slot, document);
        }
        let history = harness
            .store
            .execution_binding_revisions(&attempt_id(slot))
            .unwrap();
        if !history.is_empty() {
            binding_digests.insert(
                slot,
                history
                    .iter()
                    .map(|binding| digest(binding).unwrap())
                    .collect(),
            );
            binding_revisions.insert(slot, history);
        }
        let history = harness.store.transitions(&attempt_id(slot)).unwrap();
        if !history.is_empty() {
            transitions.insert(slot, history);
        }
        if let Some(id) = harness.quarantine_ids.get(&slot) {
            if let Some(record) = harness.store.quarantine_record(id).unwrap() {
                quarantines.insert(
                    slot,
                    QuarantineObservation {
                        payload_digest: record.payload_digest,
                        reason: record.reason,
                        resolution_code: record.resolution_code,
                        resolved_at: record.resolved_at,
                    },
                );
            }
        }
    }
    let audit_events = harness
        .store
        .audit_events()
        .unwrap()
        .into_iter()
        .map(|event| AuditObservation {
            event_code: event.event_code,
            created_at: event.created_at,
        })
        .collect();
    FoundationSnapshot {
        records,
        record_digests,
        binding_revisions,
        binding_digests,
        transitions,
        quarantines,
        audit_events,
        total_record_count: harness.store.total_record_count().unwrap(),
        transition_count: harness.store.count_transitions().unwrap(),
    }
}

fn fixture_intent(slot: u8, changed: bool) -> CanonicalDocument {
    CanonicalDocument::Intent(Intent {
        schema_version: schema("psyche.intent.v1"),
        intent_id: intent_id(slot),
        principal_id: "principal-a".to_owned(),
        familiar_snapshot_id: snapshot_id(slot),
        project_id: format!("project-{slot}"),
        requested_outcome: if changed {
            "changed immutable outcome"
        } else {
            "original immutable outcome"
        }
        .to_owned(),
        constraints: Map::new(),
        required_evidence: vec!["review".to_owned()],
        surface_event_id: None,
        created_at: at("2026-08-05T12:00:00Z"),
        digest: if changed {
            digest_of('b')
        } else {
            digest_of('a')
        },
    })
}

fn fixture_binding(slot: u8) -> ExecutionBinding {
    ExecutionBinding {
        schema_version: schema("psyche.execution_binding.v1"),
        attempt_id: attempt_id(slot),
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: at("2026-08-05T12:00:00Z"),
        familiar_snapshot_id: snapshot_id(slot),
        project_id: format!("project-{slot}"),
        request_id: request_id(slot),
        request_digest: digest_of(if slot == 0 { 'a' } else { 'b' }),
        request_created_at: at("2026-08-05T11:59:00Z"),
        request_valid_until: at("2026-08-05T12:05:00Z"),
        coven_contract_version: "coven.daemon.v1".to_owned(),
        coven_session_id: None,
        adoption_state: AdoptionState::Adopted,
        event_cursor: Some("cursor:0".to_owned()),
        cancellation_state: CancellationState::NotRequested,
        termination_request: None,
        termination_reason_code: None,
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    }
}

fn next_binding(previous: &ExecutionBinding, slot: u8) -> ExecutionBinding {
    let mut next = previous.clone();
    next.revision = previous.revision.checked_add(1).unwrap();
    next.previous_revision_digest = Some(digest(previous).unwrap());
    next.revision_created_at = previous.revision_created_at + Duration::nanoseconds(1);
    if previous.coven_session_id.is_none() {
        next.coven_session_id = Some(format!("session-{slot}"));
    } else if previous.termination_request.is_none() {
        next.cancellation_state = CancellationState::TerminationRequested;
        next.termination_request = Some(TerminationRequestCorrelation {
            termination_request_id: request_id(slot.saturating_add(10)),
            created_at: at("2026-08-05T12:01:00Z"),
            valid_until: at("2026-08-05T12:03:00Z"),
        });
        next.termination_reason_code = Some("operator_request".to_owned());
    }
    next
}

fn invalid_binding(previous: &ExecutionBinding, slot: u8, mutation: u8) -> ExecutionBinding {
    let mut candidate = next_binding(previous, slot);
    match mutation {
        0 => {
            candidate.revision = previous.revision;
            candidate.previous_revision_digest = previous.previous_revision_digest.clone();
            candidate.event_cursor = Some("cursor:fork".to_owned());
        }
        1 => {
            candidate.revision = previous.revision.saturating_add(2);
        }
        2 => candidate.previous_revision_digest = Some(digest_of('e')),
        3 => candidate.attempt_id = attempt_id(slot.saturating_add(10)),
        4 => candidate.familiar_snapshot_id = snapshot_id(slot.saturating_add(10)),
        5 => candidate.project_id = "project-mismatch".to_owned(),
        6 => candidate.request_id = request_id(slot.saturating_add(20)),
        7 => candidate.request_digest = digest_of('e'),
        8 => candidate.request_created_at += Duration::seconds(1),
        9 => candidate.request_valid_until -= Duration::seconds(1),
        10 => candidate.coven_contract_version = "coven.daemon.v2".to_owned(),
        11 => {
            if previous.coven_session_id.is_some() {
                candidate.coven_session_id = Some("session-rebound".to_owned());
            } else {
                candidate.project_id = "project-mismatch".to_owned();
            }
        }
        12 => {
            if previous.termination_request.is_some() {
                let termination = candidate
                    .termination_request
                    .as_mut()
                    .expect("candidate retains termination");
                termination.termination_request_id = request_id(slot.saturating_add(20));
            } else {
                candidate.project_id = "project-mismatch".to_owned();
            }
        }
        13 => {
            if previous.termination_request.is_some() {
                let termination = candidate
                    .termination_request
                    .as_mut()
                    .expect("candidate retains termination");
                termination.created_at += Duration::seconds(1);
            } else {
                candidate.project_id = "project-mismatch".to_owned();
            }
        }
        14 => {
            if previous.termination_request.is_some() {
                let termination = candidate
                    .termination_request
                    .as_mut()
                    .expect("candidate retains termination");
                termination.valid_until += Duration::seconds(1);
            } else {
                candidate.project_id = "project-mismatch".to_owned();
            }
        }
        _ => {
            if previous.termination_reason_code.is_some() {
                candidate.termination_reason_code = Some("changed_reason".to_owned());
            } else {
                candidate.project_id = "project-mismatch".to_owned();
            }
        }
    }
    candidate
}

fn next_transition(history: &[Transition], slot: u8) -> Transition {
    let version = u64::try_from(history.len()).unwrap().saturating_add(1);
    Transition::new(
        SchemaKind::ExecutionBinding,
        attempt_id(slot),
        version,
        history.last().map(|transition| transition.to_state.clone()),
        format!("state_{version}"),
        at("2026-08-05T12:00:00Z") + Duration::seconds(i64::try_from(version).unwrap()),
    )
    .unwrap()
}

fn unknown_major_bytes(slot: u8) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": "psyche.intent.v2",
        "slot": slot,
    }))
    .unwrap()
}

fn first_resolution(slot: u8) -> QuarantineResolution {
    QuarantineResolution {
        code: QuarantineResolutionCode::ConfirmedInvalid,
        resolved_at: at("2100-01-01T00:00:00Z") + Duration::seconds(i64::from(slot)),
    }
}

fn schema(value: &str) -> SchemaVersion {
    SchemaVersion::parse(value).unwrap()
}

fn record_id(kind: RecordKind, slot: u8) -> RecordId {
    RecordId::parse(
        kind,
        &format!("{}01J000000000000000000000{slot:02}", kind.prefix()),
    )
    .unwrap()
}

fn intent_id(slot: u8) -> RecordId {
    record_id(RecordKind::Intent, slot)
}

fn attempt_id(slot: u8) -> RecordId {
    record_id(RecordKind::Attempt, slot)
}

fn snapshot_id(slot: u8) -> RecordId {
    record_id(RecordKind::IdentitySnapshot, slot)
}

fn request_id(slot: u8) -> RequestId {
    RequestId::parse(&format!("req_01J000000000000000000000{slot:02}")).unwrap()
}

fn digest_of(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn quarantine_as_unknown_major(
    store: &mut Store,
    payload: Vec<u8>,
) -> Result<IngestOutcome, StoreError> {
    let bytes = serde_json::to_vec(&json!({
        "schema_version": "psyche.intent.v2",
        "payload": payload,
    }))
    .unwrap();
    store.ingest(&bytes)
}

fn fixture_graph_bytes_with_state(unknown_state: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": "psyche.graph.v1",
        "graph_id": "grf_01J00000000000000000000003",
        "root_intent_id": "int_01J00000000000000000000004",
        "owner_principal_id": "principal:one",
        "policy_revision": "policy:one",
        "state": unknown_state,
        "version": 1
    }))
    .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn model_and_store_agree_after_any_foundation_operation_sequence(
        operations in proptest::collection::vec(any::<FoundationOperation>(), 1..64)
    ) {
        let (mut store, _dir) = test_store();
        let mut model = FoundationModel::default();
        for operation in operations {
            let expected = model.apply(operation.clone());
            let actual = apply_to_store(&mut store, operation);
            prop_assert_eq!(expected, actual);
        }
    }
}

#[derive(Debug, Clone)]
enum CovenRecoveryOperation {
    MarkAmbiguous,
    Reconcile { fenced: bool, mutation: u8 },
    DisconnectBeforeDisposition { fenced: bool, stall: bool },
    DisconnectAfterDisposition { fenced: bool },
    Restart,
    AttemptRedispatch,
}

impl Arbitrary for CovenRecoveryOperation {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        prop_oneof![
            4 => Just(Self::MarkAmbiguous),
            5 => (any::<bool>(), 0_u8..11)
                .prop_map(|(fenced, mutation)| Self::Reconcile { fenced, mutation }),
            3 => (any::<bool>(), any::<bool>()).prop_map(|(fenced, stall)| {
                Self::DisconnectBeforeDisposition { fenced, stall }
            }),
            3 => any::<bool>()
                .prop_map(|fenced| Self::DisconnectAfterDisposition { fenced }),
            2 => Just(Self::Restart),
            3 => Just(Self::AttemptRedispatch),
        ]
        .boxed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryState {
    Clean,
    Ambiguous,
    Returned,
    Fenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryDispatchDecision {
    Rejected,
    RedispatchEligible,
}

#[derive(Debug)]
struct CovenRecoveryModel {
    state: RecoveryState,
    adoption_calls: u64,
    request: Option<ReconciliationRequest>,
    disposition: Option<ReconciliationDisposition>,
}

impl Default for CovenRecoveryModel {
    fn default() -> Self {
        Self {
            state: RecoveryState::Clean,
            adoption_calls: 0,
            request: None,
            disposition: None,
        }
    }
}

fn reconciliation_for(correlation: ExecutionCorrelation, fenced: bool) -> ReconciliationRequest {
    ReconciliationRequest {
        correlation,
        ambiguity_digest: digest_of('d'),
        reason_code: if fenced {
            "fence_ambiguous"
        } else {
            "return_original"
        }
        .to_owned(),
    }
}

fn mutate_reconciliation(request: &ReconciliationRequest, mutation: u8) -> ReconciliationRequest {
    let mut changed = request.clone();
    match mutation {
        1 => changed.correlation.request_id = request_id(11),
        2 => changed.correlation.request_digest = digest_of('a'),
        3 => {
            changed.correlation.familiar_snapshot_id = record_id(RecordKind::IdentitySnapshot, 11);
        }
        4 => changed.correlation.project_id = "project:sha256:changed".to_owned(),
        5 => changed.correlation.graph_id = record_id(RecordKind::Graph, 11),
        6 => changed.correlation.node_id = record_id(RecordKind::GraphNode, 11),
        7 => changed.correlation.attempt_id = record_id(RecordKind::Attempt, 11),
        8 => changed.correlation.created_at += Duration::seconds(1),
        9 => changed.correlation.valid_until -= Duration::seconds(1),
        10 => changed.ambiguity_digest = digest_of('e'),
        _ => {}
    }
    changed
}

async fn compare_c_s6_model_and_fixture(
    operations: Vec<CovenRecoveryOperation>,
) -> Result<(), TestCaseError> {
    let mut fixture = scripted_fixture();
    let adoption = launch_adoption();
    let correlation = adoption.correlation();
    let mut model = CovenRecoveryModel::default();

    for operation in operations {
        match operation {
            CovenRecoveryOperation::MarkAmbiguous => {
                fixture.reset().await;
                fixture
                    .select_fault(CovenFaultPoint::AdoptionAfterCommit)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(
                    fixture.port().adopt(adoption.clone()).await,
                    Err(PortError::Unavailable)
                );
                fixture
                    .clear_fault()
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                fixture
                    .select_fault(CovenFaultPoint::LookupAfterRead)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(
                    fixture.port().lookup(&correlation.request_id).await,
                    Err(PortError::Unavailable)
                );
                fixture
                    .clear_fault()
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                model = CovenRecoveryModel {
                    state: RecoveryState::Ambiguous,
                    adoption_calls: 1,
                    request: None,
                    disposition: None,
                };
            }
            CovenRecoveryOperation::Reconcile { fenced, mutation }
                if model.state != RecoveryState::Clean =>
            {
                let exact = model
                    .request
                    .clone()
                    .unwrap_or_else(|| reconciliation_for(correlation.clone(), fenced));
                let candidate = if model.request.is_some()
                    && exact.reason_code
                        != reconciliation_for(correlation.clone(), fenced).reason_code
                {
                    reconciliation_for(correlation.clone(), fenced)
                } else {
                    exact.clone()
                };
                let candidate = mutate_reconciliation(&candidate, mutation);
                let changed = if model.state == RecoveryState::Ambiguous {
                    (1..=9).contains(&mutation)
                } else {
                    candidate != exact
                };
                let result = fixture.port().reconcile(candidate.clone()).await;
                if changed {
                    prop_assert_eq!(result, Err(PortError::IntentConflict));
                } else if model.state == RecoveryState::Ambiguous {
                    let disposition =
                        result.map_err(|error| TestCaseError::fail(error.to_string()))?;
                    model.state = if fenced {
                        RecoveryState::Fenced
                    } else {
                        RecoveryState::Returned
                    };
                    model.request = Some(candidate);
                    model.disposition = Some(disposition);
                } else {
                    prop_assert_eq!(result, Ok(model.disposition.clone().unwrap()));
                }
            }
            CovenRecoveryOperation::DisconnectBeforeDisposition { fenced, stall }
                if model.state == RecoveryState::Ambiguous =>
            {
                let point = if stall {
                    CovenFaultPoint::ReconcileStall
                } else {
                    CovenFaultPoint::ReconcileBeforeDisposition
                };
                fixture
                    .select_fault(point)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let request = reconciliation_for(correlation.clone(), fenced);
                let expected = if stall {
                    PortError::Stalled
                } else {
                    PortError::Unavailable
                };
                prop_assert_eq!(fixture.port().reconcile(request).await, Err(expected));
                fixture.restart().await;
                model.adoption_calls = 0;
                prop_assert!(
                    fixture
                        .observations()
                        .await
                        .durable_reconciliation
                        .is_none()
                );
                fixture
                    .clear_fault()
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
            }
            CovenRecoveryOperation::DisconnectAfterDisposition { fenced }
                if model.state == RecoveryState::Ambiguous =>
            {
                let request = reconciliation_for(correlation.clone(), fenced);
                fixture
                    .select_fault(CovenFaultPoint::ReconcileAfterDisposition)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(
                    fixture.port().reconcile(request.clone()).await,
                    Err(PortError::Unavailable)
                );
                let committed = fixture
                    .observations()
                    .await
                    .durable_reconciliation
                    .ok_or_else(|| TestCaseError::fail("after-commit disposition was lost"))?;
                fixture.restart().await;
                model.adoption_calls = 0;
                fixture
                    .clear_fault()
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let disposition = fixture
                    .port()
                    .reconcile(request.clone())
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let expected_kind = match &disposition {
                    ReconciliationDisposition::Returned {
                        disposition_id,
                        session_id,
                        correlation,
                        ambiguity_digest,
                        recorded_at,
                    } => {
                        prop_assert_eq!(&committed.disposition_id, disposition_id);
                        prop_assert_eq!(&committed.correlation, correlation);
                        prop_assert_eq!(&committed.ambiguity_digest, ambiguity_digest);
                        prop_assert_eq!(committed.recorded_at, *recorded_at);
                        DurableDispositionKind::Returned {
                            session_id: session_id.clone(),
                        }
                    }
                    ReconciliationDisposition::Fenced {
                        disposition_id,
                        fence_token,
                        correlation,
                        ambiguity_digest,
                        recorded_at,
                    } => {
                        prop_assert_eq!(&committed.disposition_id, disposition_id);
                        prop_assert_eq!(&committed.correlation, correlation);
                        prop_assert_eq!(&committed.ambiguity_digest, ambiguity_digest);
                        prop_assert_eq!(committed.recorded_at, *recorded_at);
                        DurableDispositionKind::Fenced {
                            fence_token: fence_token.clone(),
                        }
                    }
                    ReconciliationDisposition::Unresolved => {
                        return Err(TestCaseError::fail(
                            "after-commit terminal replay became unresolved",
                        ));
                    }
                };
                prop_assert_eq!(committed.kind, expected_kind);
                model.state = if fenced {
                    RecoveryState::Fenced
                } else {
                    RecoveryState::Returned
                };
                model.request = Some(request);
                model.disposition = Some(disposition);
            }
            CovenRecoveryOperation::Restart => {
                fixture.restart().await;
                model.adoption_calls = 0;
            }
            CovenRecoveryOperation::AttemptRedispatch => {
                let decision = match model.state {
                    RecoveryState::Fenced => RecoveryDispatchDecision::RedispatchEligible,
                    RecoveryState::Clean | RecoveryState::Ambiguous | RecoveryState::Returned => {
                        RecoveryDispatchDecision::Rejected
                    }
                };
                if matches!(
                    model.state,
                    RecoveryState::Ambiguous | RecoveryState::Returned
                ) {
                    prop_assert_eq!(decision, RecoveryDispatchDecision::Rejected);
                }
                let before = fixture.observations().await.adoption_calls;
                let actual = fixture
                    .redispatch_eligibility(&correlation)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let expected = if decision == RecoveryDispatchDecision::RedispatchEligible {
                    RedispatchEligibility::EligibleAfterFence
                } else {
                    RedispatchEligibility::Blocked
                };
                prop_assert_eq!(actual, expected);
                prop_assert_eq!(fixture.observations().await.adoption_calls, before);
            }
            _ => {}
        }

        let observations = fixture.observations().await;
        prop_assert_eq!(observations.adoption_calls, model.adoption_calls);
        match model.state {
            RecoveryState::Clean | RecoveryState::Ambiguous => {
                prop_assert!(observations.durable_reconciliation.is_none());
            }
            RecoveryState::Returned => {
                let durable = observations
                    .durable_reconciliation
                    .ok_or_else(|| TestCaseError::fail("returned disposition was not durable"))?;
                prop_assert_eq!(durable.correlation, correlation.clone());
                let DurableDispositionKind::Returned { session_id } = durable.kind else {
                    return Err(TestCaseError::fail("returned model observed a fence"));
                };
                prop_assert_eq!(session_id, "session-1");
                let resumed = fixture
                    .port()
                    .inspect("session-1")
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(resumed.correlation, correlation.clone());
            }
            RecoveryState::Fenced => {
                let durable = observations
                    .durable_reconciliation
                    .ok_or_else(|| TestCaseError::fail("fenced disposition was not durable"))?;
                prop_assert_eq!(durable.correlation, correlation.clone());
                let DurableDispositionKind::Fenced { fence_token } = durable.kind else {
                    return Err(TestCaseError::fail("fenced model observed a return"));
                };
                prop_assert!(!fence_token.is_empty());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum RequestDigestOperation {
    ConstructRequest { input: bool },
    Replay,
    MutateRequestFieldRetainDigest { field: u8 },
    Restart,
}

impl Arbitrary for RequestDigestOperation {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        prop_oneof![
            4 => any::<bool>().prop_map(|input| Self::ConstructRequest { input }),
            3 => Just(Self::Replay),
            6 => any::<u8>().prop_map(|field| Self::MutateRequestFieldRetainDigest { field }),
            2 => Just(Self::Restart),
        ]
        .boxed()
    }
}

#[derive(Debug, Default)]
struct RequestDigestModel {
    request: Option<AdoptionRequest>,
    disposition: Option<AdoptionDisposition>,
    adoption_calls: u64,
}

async fn compare_request_digest_model_and_fixture(
    operations: Vec<RequestDigestOperation>,
) -> Result<(), TestCaseError> {
    let mut fixture = scripted_fixture();
    let mut model = RequestDigestModel::default();
    for operation in operations {
        match operation {
            RequestDigestOperation::ConstructRequest { input } => {
                fixture.reset().await;
                model = RequestDigestModel::default();
                if input {
                    fixture
                        .port()
                        .adopt(launch_adoption())
                        .await
                        .map_err(|error| TestCaseError::fail(error.to_string()))?;
                    model.adoption_calls = 1;
                }
                let request = if input {
                    session_input_adoption()
                } else {
                    launch_adoption()
                };
                prop_assert_eq!(
                    digest(request.input()).unwrap(),
                    request.request_digest().clone()
                );
                prop_assert!(!canonical_bytes(request.input()).unwrap().is_empty());
                prop_assert_eq!(
                    request.recompute_digest().unwrap(),
                    request.request_digest().clone()
                );
                let disposition = fixture
                    .port()
                    .adopt(request.clone())
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                model.adoption_calls = model.adoption_calls.saturating_add(1);
                model.request = Some(request);
                model.disposition = Some(disposition);
            }
            RequestDigestOperation::Replay => {
                if let (Some(request), Some(disposition)) = (&model.request, &model.disposition) {
                    fixture.restart().await;
                    model.adoption_calls = 0;
                    prop_assert_eq!(
                        fixture.port().adopt(request.clone()).await,
                        Ok(disposition.clone())
                    );
                }
            }
            RequestDigestOperation::MutateRequestFieldRetainDigest { field } => {
                if let Some(request) = &model.request {
                    let mutations = stale_digest_requests(request);
                    let (_, forged) = &mutations[usize::from(field) % mutations.len()];
                    let before = fixture.observations().await;
                    prop_assert_eq!(
                        fixture.port().adopt(forged.clone()).await,
                        Err(PortError::RequestDigestMismatch)
                    );
                    let after = fixture.observations().await;
                    prop_assert_eq!(after.adoption_calls, before.adoption_calls);
                    prop_assert_eq!(after.durable_reconciliation, before.durable_reconciliation);
                    let forged_id = forged.correlation().request_id;
                    if forged_id != request.correlation().request_id {
                        prop_assert_ne!(
                            fixture.port().lookup(&forged_id).await,
                            Ok(AdoptionDisposition::Adopted {
                                session_id: "session-1".to_owned(),
                            })
                        );
                    }
                    prop_assert_eq!(
                        fixture.port().adopt(request.clone()).await,
                        Ok(model.disposition.clone().unwrap())
                    );
                }
            }
            RequestDigestOperation::Restart => {
                fixture.restart().await;
                model.adoption_calls = 0;
            }
        }
        prop_assert_eq!(
            fixture.observations().await.adoption_calls,
            model.adoption_calls
        );
    }
    Ok(())
}

fn launch_adoption() -> AdoptionRequest {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    AdoptionRequest::new(input).unwrap()
}

fn session_input_adoption() -> AdoptionRequest {
    let mut value: serde_json::Value = serde_json::from_slice(INPUT_GOLDEN).unwrap();
    value["request_id"] = json!("req_01J00000000000000000000003");
    AdoptionRequest::new(serde_json::from_value(value).unwrap()).unwrap()
}

fn stale_digest_requests(request: &AdoptionRequest) -> Vec<(&'static str, AdoptionRequest)> {
    let mut mutations: Vec<(&str, serde_json::Value)> = match request.input() {
        ExecutionRequestInput::Launch { .. } => vec![
            (
                "/input/schema_version",
                json!("psyche.execution_request.v2"),
            ),
            ("/input/request_id", json!("req_01J00000000000000000000011")),
            ("/input/graph_id", json!("grf_01J00000000000000000000011")),
            ("/input/node_id", json!("nod_01J00000000000000000000011")),
            ("/input/attempt_id", json!("att_01J00000000000000000000011")),
            ("/input/principal_id", json!("principal:changed")),
            (
                "/input/familiar_snapshot_id",
                json!("ids_01J00000000000000000000011"),
            ),
            ("/input/project_id", json!("project:sha256:changed")),
            ("/input/project_root", json!("/workspace/changed")),
            ("/input/cwd", json!("/workspace/project/changed")),
            ("/input/harness", json!("future_harness")),
            (
                "/input/context_manifest_digest",
                json!(digest_of('7').as_str()),
            ),
            ("/input/delegation_digest", json!(digest_of('8').as_str())),
            ("/input/budget_digest", json!(digest_of('9').as_str())),
            (
                "/input/required_artifact_bindings/0/artifact_id",
                json!("artifact-changed"),
            ),
            (
                "/input/required_artifact_bindings/0/digest",
                json!(digest_of('a').as_str()),
            ),
            (
                "/input/required_artifact_bindings/0/media_type",
                json!("application/json"),
            ),
            ("/input/required_artifact_bindings/0/size", json!(13)),
            (
                "/input/required_artifact_bindings",
                json!([
                    {
                        "artifact_id": "artifact-2",
                        "digest": digest_of('a').as_str(),
                        "media_type": "application/json",
                        "size": 7
                    },
                    {
                        "artifact_id": "artifact-1",
                        "digest": digest_of('3').as_str(),
                        "media_type": "text/plain",
                        "size": 12
                    }
                ]),
            ),
            ("/input/payload_digest", json!(digest_of('b').as_str())),
            ("/input/created_at", json!("2026-08-05T14:00:01Z")),
            ("/input/valid_until", json!("2026-08-05T14:04:59Z")),
        ],
        ExecutionRequestInput::Input { .. } => vec![
            (
                "/input/schema_version",
                json!("psyche.execution_request.v2"),
            ),
            ("/input/request_id", json!("req_01J00000000000000000000011")),
            ("/input/graph_id", json!("grf_01J00000000000000000000011")),
            ("/input/node_id", json!("nod_01J00000000000000000000011")),
            ("/input/attempt_id", json!("att_01J00000000000000000000011")),
            ("/input/principal_id", json!("principal:changed")),
            (
                "/input/familiar_snapshot_id",
                json!("ids_01J00000000000000000000011"),
            ),
            ("/input/project_id", json!("project:sha256:changed")),
            ("/input/session_id", json!("session-changed")),
            ("/input/input_digest", json!(digest_of('7').as_str())),
            (
                "/input/context_manifest_digest",
                json!(digest_of('8').as_str()),
            ),
            (
                "/input/required_artifact_bindings",
                json!([{
                    "artifact_id": "artifact-new",
                    "digest": digest_of('9').as_str(),
                    "media_type": "text/plain",
                    "size": 1
                }]),
            ),
            ("/input/payload_digest", json!(digest_of('a').as_str())),
            ("/input/created_at", json!("2026-08-05T14:01:01Z")),
            ("/input/valid_until", json!("2026-08-05T14:05:59Z")),
        ],
    };
    let mut other_input: serde_json::Value =
        if matches!(request.input(), ExecutionRequestInput::Launch { .. }) {
            serde_json::from_slice(INPUT_GOLDEN).unwrap()
        } else {
            serde_json::from_slice(LAUNCH_GOLDEN).unwrap()
        };
    other_input["request_id"] = json!(request.correlation().request_id.as_str());
    mutations.push(("/input", other_input));
    mutations
        .into_iter()
        .map(|(pointer, replacement)| {
            let mut value = serde_json::to_value(request).unwrap();
            *value.pointer_mut(pointer).unwrap() = replacement;
            (pointer, serde_json::from_value(value).unwrap())
        })
        .collect()
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn c_s6_model_never_redispatches_without_fence(
        operations in proptest::collection::vec(any::<CovenRecoveryOperation>(), 1..64)
    ) {
        runtime().block_on(compare_c_s6_model_and_fixture(operations))?;
    }

    #[test]
    fn request_digest_binds_every_typed_field(
        operations in proptest::collection::vec(any::<RequestDigestOperation>(), 1..64)
    ) {
        runtime().block_on(compare_request_digest_model_and_fixture(operations))?;
    }
}

#[test]
fn c_s6_fixture_reports_fence_eligibility_without_redispatch() {
    runtime()
        .block_on(compare_c_s6_model_and_fixture(vec![
            CovenRecoveryOperation::MarkAmbiguous,
            CovenRecoveryOperation::Reconcile {
                fenced: true,
                mutation: 0,
            },
            CovenRecoveryOperation::AttemptRedispatch,
        ]))
        .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn unknown_schema_operations_never_create_dispatchable_records(
        payload in proptest::collection::vec(any::<u8>(), 0..8192)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("private").join("psyche.sqlite3")).unwrap();
        let outcome = quarantine_as_unknown_major(&mut store, payload).unwrap();
        prop_assert!(matches!(outcome, IngestOutcome::Quarantined { .. }), "unknown major was not quarantined");
        prop_assert_eq!(store.total_record_count().unwrap(), 0);
        prop_assert_eq!(store.count_transitions().unwrap(), 0);
    }

    #[test]
    fn unknown_enum_operations_never_create_dispatchable_records(
        unknown_state in "future_[a-z]{1,24}"
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("private").join("psyche.sqlite3")).unwrap();
        let outcome = store.ingest(&fixture_graph_bytes_with_state(&unknown_state)).unwrap();
        prop_assert!(matches!(outcome, IngestOutcome::Quarantined { .. }), "unknown enum was not quarantined");
        prop_assert_eq!(store.total_record_count().unwrap(), 0);
        prop_assert_eq!(store.count_transitions().unwrap(), 0);
    }
}
