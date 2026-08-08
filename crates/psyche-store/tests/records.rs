#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use psyche_core::contracts::error::{ErrorBody, ErrorCode, ErrorEnvelope};
use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, CancellationUnresolvedEvidence, ExecutionBinding,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::surface::{
    DeliveryDecisionState, DeliveryRelationship, DeliveryState, DeliverySurfaceDecision,
    DeliveryTopic,
};
use psyche_core::contracts::{
    CanonicalDocument, ContractError, Delivery, Intent, RecordKind, SchemaKind, SchemaVersion,
    VersionedRecord,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use psyche_store::{IngestOutcome, Store, StoreError, Transition};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Map, json};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

fn test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("private").join("psyche.sqlite3")).unwrap();
    (store, dir)
}

fn test_store_with_path() -> (Store, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    let store = Store::open(&path).unwrap();
    (store, dir, path)
}

fn raw_connection(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn assert_database_corruption<T: std::fmt::Debug>(result: Result<T, StoreError>) {
    let error = result.unwrap_err();
    assert_eq!(
        error.to_string(),
        "stored database content failed integrity validation"
    );
    assert_eq!(
        format!("{error:?}"),
        "StoreError(stored database content failed integrity validation)"
    );
    assert!(error.source().is_none());
}

fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn fixture_digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn fixture_other_digest() -> Sha256Digest {
    fixture_digest('b')
}

fn record_id(kind: RecordKind, suffix: &str) -> RecordId {
    RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())).unwrap()
}

fn fixture_attempt_id() -> RecordId {
    record_id(RecordKind::Attempt, "01J00000000000000000000000")
}

fn fixture_other_attempt_id() -> RecordId {
    record_id(RecordKind::Attempt, "01J00000000000000000000001")
}

fn fixture_snapshot_id() -> RecordId {
    record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000002")
}

fn fixture_other_snapshot_id() -> RecordId {
    record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000003")
}

fn fixture_project_id() -> String {
    "project-a".to_owned()
}

fn fixture_other_project_id() -> String {
    "project-b".to_owned()
}

fn fixture_request_id() -> RequestId {
    RequestId::parse("req_01J00000000000000000000004").unwrap()
}

fn fixture_termination_request_id() -> RequestId {
    RequestId::parse("req_01J00000000000000000000005").unwrap()
}

fn fixture_other_request_id() -> RequestId {
    RequestId::parse("req_01J00000000000000000000006").unwrap()
}

fn fixture_other_coven_contract_version() -> String {
    "coven.v2".to_owned()
}

fn fixture_intent(outcome: &str) -> Intent {
    Intent {
        schema_version: SchemaVersion::parse("psyche.intent.v1").unwrap(),
        intent_id: record_id(RecordKind::Intent, "01J00000000000000000000007"),
        principal_id: "principal-a".to_owned(),
        familiar_snapshot_id: fixture_snapshot_id(),
        project_id: fixture_project_id(),
        requested_outcome: outcome.to_owned(),
        constraints: Map::new(),
        required_evidence: vec!["review".to_owned()],
        surface_event_id: None,
        created_at: at("2026-08-05T12:00:00Z"),
        digest: fixture_digest('a'),
    }
}

fn fixture_intent_with_same_id(outcome: &str) -> Intent {
    fixture_intent(outcome)
}

fn fixture_error_envelope() -> ErrorEnvelope {
    ErrorEnvelope {
        schema_version: SchemaVersion::parse("psyche.error.v1").unwrap(),
        error: ErrorBody {
            code: ErrorCode::StorageUnavailable,
            message: "unavailable".to_owned(),
            retryable: true,
            correlation_id: "correlation-a".to_owned(),
            details: BTreeMap::new(),
        },
    }
}

fn fixture_delivery() -> Delivery {
    let effect = json!({"method": "send_message", "text": "hello"});
    Delivery {
        schema_version: SchemaVersion::parse("psyche.delivery.v1").unwrap(),
        delivery_id: record_id(RecordKind::Delivery, "01J00000000000000000000008"),
        intent_id: fixture_intent("deliver").intent_id,
        action_class: "send_message".to_owned(),
        account_id: "account-a".to_owned(),
        chat_id: "-100123".to_owned(),
        topic: DeliveryTopic {
            kind: "telegram_topic".to_owned(),
            id: "42".to_owned(),
        },
        relationship: DeliveryRelationship::ReplySameTopic,
        effect_digest: digest(&effect).unwrap(),
        effect,
        surface_decision: DeliverySurfaceDecision {
            decision_id: "decision-a".to_owned(),
            request_digest: fixture_digest('c'),
            policy_revision: "policy-v1".to_owned(),
            expires_at: at("2026-08-05T12:10:00Z"),
            state: DeliveryDecisionState::Reserved,
        },
        logical_response_id: "response-a".to_owned(),
        logical_part: 0,
        state: DeliveryState::Ready,
        attempt_count: 0,
        telegram_message_id: None,
    }
}

fn fixture_execution_binding_revision_1() -> ExecutionBinding {
    ExecutionBinding {
        schema_version: SchemaVersion::parse("psyche.execution_binding.v1").unwrap(),
        attempt_id: fixture_attempt_id(),
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: at("2026-08-05T12:00:00Z"),
        familiar_snapshot_id: fixture_snapshot_id(),
        project_id: fixture_project_id(),
        request_id: fixture_request_id(),
        request_digest: fixture_digest('a'),
        request_created_at: at("2026-08-05T11:59:00Z"),
        request_valid_until: at("2026-08-05T12:05:00Z"),
        coven_contract_version: "coven.v1".to_owned(),
        coven_session_id: None,
        adoption_state: AdoptionState::Adopted,
        event_cursor: Some("cursor:1".to_owned()),
        cancellation_state: CancellationState::NotRequested,
        termination_request: None,
        termination_reason_code: None,
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    }
}

fn fixture_execution_binding() -> ExecutionBinding {
    fixture_execution_binding_revision_1()
}

fn next_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut next = previous.clone();
    next.revision += 1;
    next.previous_revision_digest = Some(digest(previous).unwrap());
    next.revision_created_at += time::Duration::nanoseconds(1);
    next
}

fn fixture_next_not_requested_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    next_revision(previous)
}

fn termination_request() -> TerminationRequestCorrelation {
    TerminationRequestCorrelation {
        termination_request_id: fixture_termination_request_id(),
        created_at: at("2026-08-05T12:06:00Z"),
        valid_until: at("2026-08-05T12:08:00Z"),
    }
}

fn fixture_termination_requested_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut requested = next_revision(previous);
    requested.coven_session_id = Some("session-a".to_owned());
    requested.cancellation_state = CancellationState::TerminationRequested;
    requested.termination_request = Some(termination_request());
    requested.termination_reason_code = Some("operator_request".to_owned());
    requested
}

fn fixture_next_termination_requested_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    next_revision(previous)
}

fn fixture_not_requested_revision_after(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut next = next_revision(previous);
    next.cancellation_state = CancellationState::NotRequested;
    next.termination_request = None;
    next.termination_reason_code = None;
    next.cancellation_acknowledgement = None;
    next.cancellation_unresolved = None;
    next
}

fn fixture_session_bound_revision(previous: &ExecutionBinding, session: &str) -> ExecutionBinding {
    let mut next = next_revision(previous);
    next.coven_session_id = Some(session.to_owned());
    next
}

fn acknowledgement(binding: &ExecutionBinding) -> CancellationAcknowledgementEvidence {
    let termination = binding.termination_request.as_ref().unwrap();
    CancellationAcknowledgementEvidence {
        acknowledgement_id: "ack-a".to_owned(),
        termination_request_id: termination.termination_request_id.clone(),
        session_id: binding.coven_session_id.clone().unwrap(),
        execution_request_id: binding.request_id.clone(),
        execution_request_digest: binding.request_digest.clone(),
        kind: CancellationAcknowledgementKind::Terminated,
        authority_evidence_digest: fixture_digest('d'),
        acknowledged_at: termination.created_at + time::Duration::seconds(30),
    }
}

fn unresolved(binding: &ExecutionBinding) -> CancellationUnresolvedEvidence {
    let termination = binding.termination_request.as_ref().unwrap();
    CancellationUnresolvedEvidence {
        disposition_id: "unresolved-a".to_owned(),
        termination_request_id: termination.termination_request_id.clone(),
        session_id: binding.coven_session_id.clone().unwrap(),
        execution_request_id: binding.request_id.clone(),
        execution_request_digest: binding.request_digest.clone(),
        reason_code: "timeout".to_owned(),
        recorded_at: termination.created_at + time::Duration::seconds(30),
    }
}

fn fixture_acknowledged_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut acknowledged = next_revision(previous);
    acknowledged.cancellation_state = CancellationState::AcknowledgedTerminated;
    acknowledged.cancellation_acknowledgement = Some(acknowledgement(&acknowledged));
    acknowledged
}

fn fixture_already_terminal_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut acknowledged = next_revision(previous);
    acknowledged.cancellation_state = CancellationState::AcknowledgedAlreadyTerminal;
    let mut evidence = acknowledgement(&acknowledged);
    evidence.kind = CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
    acknowledged.cancellation_acknowledgement = Some(evidence);
    acknowledged
}

fn fixture_unresolved_revision(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut unresolved_binding = next_revision(previous);
    unresolved_binding.cancellation_state = CancellationState::TerminationUnknown;
    unresolved_binding.cancellation_unresolved = Some(unresolved(&unresolved_binding));
    unresolved_binding
}

fn fixture_acknowledged_execution_binding() -> ExecutionBinding {
    let mut binding = fixture_execution_binding();
    binding.coven_session_id = Some("session-a".to_owned());
    binding.cancellation_state = CancellationState::AcknowledgedTerminated;
    binding.termination_request = Some(termination_request());
    binding.termination_reason_code = Some("operator_request".to_owned());
    binding.cancellation_acknowledgement = Some(acknowledgement(&binding));
    binding
}

fn fixture_unresolved_execution_binding() -> ExecutionBinding {
    let mut binding = fixture_execution_binding();
    binding.coven_session_id = Some("session-a".to_owned());
    binding.cancellation_state = CancellationState::TerminationUnknown;
    binding.termination_request = Some(termination_request());
    binding.termination_reason_code = Some("operator_request".to_owned());
    binding.cancellation_unresolved = Some(unresolved(&binding));
    binding
}

fn fixture_acknowledged_binding_after_execution_deadline() -> ExecutionBinding {
    let mut binding = fixture_acknowledged_execution_binding();
    let termination = binding.termination_request.as_mut().unwrap();
    termination.created_at = binding.request_valid_until + time::Duration::seconds(1);
    termination.valid_until = termination.created_at + time::Duration::minutes(1);
    binding
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .acknowledged_at = termination.created_at;
    binding
}

fn transition(record_version: u64, from_state: Option<&str>, to_state: &str) -> Transition {
    Transition::new(
        SchemaKind::ExecutionBinding,
        fixture_attempt_id(),
        record_version,
        from_state.map(str::to_owned),
        to_state.to_owned(),
        at("2026-08-05T12:00:00Z") + time::Duration::seconds(record_version as i64),
    )
    .unwrap()
}

#[test]
fn delivery_direct_insert_round_trips_canonically() {
    let (mut store, _dir) = test_store();
    let delivery = fixture_delivery();
    let id = delivery.record_id().clone();
    let expected = CanonicalDocument::Delivery(delivery);
    store.insert(&expected).unwrap();
    assert_eq!(
        store.load(SchemaKind::Delivery, &id).unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        store
            .load_canonical_bytes(SchemaKind::Delivery, &id)
            .unwrap(),
        Some(canonical_bytes(&expected).unwrap())
    );
}

#[test]
fn same_id_same_digest_is_idempotent_but_changed_payload_conflicts() {
    let (mut store, _dir) = test_store();
    let intent = fixture_intent("Review A");
    store
        .insert(&CanonicalDocument::Intent(intent.clone()))
        .unwrap();
    store.insert(&CanonicalDocument::Intent(intent)).unwrap();
    let changed = fixture_intent_with_same_id("Review B");
    assert!(matches!(
        store.insert(&CanonicalDocument::Intent(changed)),
        Err(StoreError::RecordConflict { .. })
    ));
    assert_eq!(store.total_record_count().unwrap(), 1);
}

#[test]
fn direct_insert_rejects_wrong_field_id_kind_without_writing() {
    let (mut store, _dir) = test_store();
    let mut intent = fixture_intent("Review A");
    intent.intent_id =
        RecordId::parse(RecordKind::Graph, "grf_01J00000000000000000000000").unwrap();
    assert!(matches!(
        store.insert(&CanonicalDocument::Intent(intent)),
        Err(StoreError::Contract(ContractError::WrongRecordKind { .. }))
    ));
    assert_eq!(store.total_record_count().unwrap(), 0);
}

#[test]
fn direct_insert_rejects_wrong_schema_without_writing() {
    let (mut store, _dir) = test_store();
    let mut intent = fixture_intent("Review A");
    intent.schema_version = SchemaVersion::parse("psyche.graph.v1").unwrap();
    assert!(matches!(
        store.insert(&CanonicalDocument::Intent(intent)),
        Err(StoreError::Contract(ContractError::SchemaMismatch { .. }))
    ));
    assert_eq!(store.total_record_count().unwrap(), 0);
}

#[test]
fn direct_insert_rejects_non_persistable_error_envelope() {
    let (mut store, _dir) = test_store();
    assert!(matches!(
        store.insert(&CanonicalDocument::Error(fixture_error_envelope())),
        Err(StoreError::NonPersistableKind {
            kind: SchemaKind::Error
        })
    ));
    assert_eq!(store.total_record_count().unwrap(), 0);
}

#[test]
fn ingest_rejects_non_persistable_error_envelope() {
    let (mut store, _dir) = test_store();
    let bytes = canonical_bytes(&fixture_error_envelope()).unwrap();
    assert!(matches!(
        store.ingest(&bytes),
        Err(StoreError::NonPersistableKind {
            kind: SchemaKind::Error
        })
    ));
    assert_eq!(store.total_record_count().unwrap(), 0);
}

#[test]
fn ingest_distinguishes_insert_from_exact_replay() {
    let (mut store, _dir) = test_store();
    let bytes = canonical_bytes(&fixture_intent("Review A")).unwrap();
    assert_eq!(store.ingest(&bytes).unwrap(), IngestOutcome::Inserted);
    assert_eq!(store.ingest(&bytes).unwrap(), IngestOutcome::AlreadyPresent);
}

#[test]
fn load_helpers_reject_non_persistable_error_before_querying() {
    let (store, _dir) = test_store();
    let id = fixture_attempt_id();
    assert!(matches!(
        store.load(SchemaKind::Error, &id),
        Err(StoreError::NonPersistableKind {
            kind: SchemaKind::Error
        })
    ));
    assert!(matches!(
        store.load_canonical_bytes(SchemaKind::Error, &id),
        Err(StoreError::NonPersistableKind {
            kind: SchemaKind::Error
        })
    ));
}

#[test]
fn direct_insert_rejects_acknowledged_cancellation_without_evidence() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    binding.cancellation_acknowledgement = None;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_acknowledged_state_without_termination_correlation() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    binding.termination_request = None;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_mismatched_cancellation_evidence() {
    fn mutation(
        name: &'static str,
        baseline: &ExecutionBinding,
        mutate: impl FnOnce(&mut ExecutionBinding),
    ) -> (&'static str, ExecutionBinding) {
        baseline.validate().unwrap();
        let before = serde_json::to_value(baseline).unwrap();
        let mut binding = baseline.clone();
        mutate(&mut binding);
        let after = serde_json::to_value(&binding).unwrap();
        let changed_fields = before
            .as_object()
            .unwrap()
            .keys()
            .filter(|field| before.get(*field) != after.get(*field))
            .count();
        assert_eq!(changed_fields, 1, "{name} must mutate exactly one field");
        (name, binding)
    }

    fn assert_no_writes(store: &Store, attempt_id: &RecordId, name: &str) {
        assert!(
            store
                .execution_binding_revisions(attempt_id)
                .unwrap()
                .is_empty(),
            "{name} wrote an execution-binding revision"
        );
        assert_eq!(
            store.count_records(SchemaKind::ExecutionBinding).unwrap(),
            0,
            "{name} wrote an execution-binding record"
        );
        assert_eq!(
            store.total_record_count().unwrap(),
            0,
            "{name} wrote a canonical record"
        );
        assert_eq!(
            store.count_transitions().unwrap(),
            0,
            "{name} wrote a transition"
        );
    }

    let not_requested = fixture_execution_binding();
    let acknowledged_terminated = fixture_acknowledged_execution_binding();
    let mut acknowledged_already_terminal = fixture_acknowledged_execution_binding();
    acknowledged_already_terminal.cancellation_state =
        CancellationState::AcknowledgedAlreadyTerminal;
    acknowledged_already_terminal
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .kind = CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
    let termination_unknown = fixture_unresolved_execution_binding();

    for baseline in [
        &not_requested,
        &acknowledged_terminated,
        &acknowledged_already_terminal,
        &termination_unknown,
    ] {
        baseline.validate().unwrap();
    }

    let cases = vec![
        mutation(
            "acknowledged terminated without evidence",
            &acknowledged_terminated,
            |binding| binding.cancellation_acknowledgement = None,
        ),
        mutation(
            "acknowledged already terminal without evidence",
            &acknowledged_already_terminal,
            |binding| binding.cancellation_acknowledgement = None,
        ),
        mutation(
            "termination unknown without evidence",
            &termination_unknown,
            |binding| binding.cancellation_unresolved = None,
        ),
        mutation(
            "acknowledged terminated with wrong kind",
            &acknowledged_terminated,
            |binding| {
                binding.cancellation_acknowledgement.as_mut().unwrap().kind =
                    CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
            },
        ),
        mutation(
            "acknowledged already terminal with wrong kind",
            &acknowledged_already_terminal,
            |binding| {
                binding.cancellation_acknowledgement.as_mut().unwrap().kind =
                    CancellationAcknowledgementKind::Terminated;
            },
        ),
        mutation(
            "acknowledged terminated with unresolved evidence",
            &acknowledged_terminated,
            |binding| binding.cancellation_unresolved = Some(unresolved(binding)),
        ),
        mutation(
            "acknowledged already terminal with unresolved evidence",
            &acknowledged_already_terminal,
            |binding| binding.cancellation_unresolved = Some(unresolved(binding)),
        ),
        mutation(
            "termination unknown with acknowledgement evidence",
            &termination_unknown,
            |binding| {
                binding.cancellation_acknowledgement = Some(acknowledgement(binding));
            },
        ),
        mutation(
            "acknowledgement with wrong session",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .session_id = "session-b".to_owned();
            },
        ),
        mutation(
            "acknowledgement with empty session",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .session_id = String::new();
            },
        ),
        mutation(
            "acknowledgement with oversized session",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .session_id = "s".repeat(256);
            },
        ),
        mutation(
            "empty acknowledgement id",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .acknowledgement_id = String::new();
            },
        ),
        mutation(
            "oversized acknowledgement id",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .acknowledgement_id = "a".repeat(256);
            },
        ),
        mutation(
            "acknowledgement with wrong termination request id",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .termination_request_id = fixture_other_request_id();
            },
        ),
        mutation(
            "acknowledgement with wrong execution request id",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .execution_request_id = fixture_other_request_id();
            },
        ),
        mutation(
            "acknowledgement with wrong execution digest",
            &acknowledged_terminated,
            |binding| {
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .execution_request_digest = fixture_other_digest();
            },
        ),
        mutation(
            "termination request id reused as execution request id",
            &acknowledged_terminated,
            |binding| {
                binding
                    .termination_request
                    .as_mut()
                    .unwrap()
                    .termination_request_id = binding.request_id.clone();
            },
        ),
        mutation(
            "missing termination correlation",
            &acknowledged_terminated,
            |binding| binding.termination_request = None,
        ),
        mutation(
            "mismatched termination correlation",
            &acknowledged_terminated,
            |binding| {
                binding
                    .termination_request
                    .as_mut()
                    .unwrap()
                    .termination_request_id = fixture_other_request_id();
            },
        ),
        mutation(
            "empty termination window",
            &acknowledged_terminated,
            |binding| {
                let created_at = binding.termination_request.as_ref().unwrap().created_at;
                binding.termination_request.as_mut().unwrap().valid_until = created_at;
            },
        ),
        mutation(
            "inverted termination window",
            &acknowledged_terminated,
            |binding| {
                let created_at = binding.termination_request.as_ref().unwrap().created_at;
                binding.termination_request.as_mut().unwrap().valid_until =
                    created_at - time::Duration::nanoseconds(1);
            },
        ),
        mutation(
            "termination before execution request",
            &acknowledged_terminated,
            |binding| {
                binding.termination_request.as_mut().unwrap().created_at =
                    binding.request_created_at - time::Duration::nanoseconds(1);
            },
        ),
        mutation(
            "absent termination reason",
            &acknowledged_terminated,
            |binding| binding.termination_reason_code = None,
        ),
        mutation("unexpected termination reason", &not_requested, |binding| {
            binding.termination_reason_code = Some("operator_request".to_owned());
        }),
        mutation(
            "empty termination reason",
            &acknowledged_terminated,
            |binding| binding.termination_reason_code = Some(String::new()),
        ),
        mutation(
            "oversized termination reason",
            &acknowledged_terminated,
            |binding| binding.termination_reason_code = Some("a".repeat(129)),
        ),
        mutation(
            "invalid termination reason",
            &acknowledged_terminated,
            |binding| binding.termination_reason_code = Some("OperatorRequest".to_owned()),
        ),
        mutation(
            "acknowledgement before termination start",
            &acknowledged_terminated,
            |binding| {
                let before_start = binding.termination_request.as_ref().unwrap().created_at
                    - time::Duration::nanoseconds(1);
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .acknowledged_at = before_start;
            },
        ),
        mutation(
            "acknowledgement after termination deadline",
            &acknowledged_terminated,
            |binding| {
                let after_deadline = binding.termination_request.as_ref().unwrap().valid_until
                    + time::Duration::nanoseconds(1);
                binding
                    .cancellation_acknowledgement
                    .as_mut()
                    .unwrap()
                    .acknowledged_at = after_deadline;
            },
        ),
        mutation(
            "unresolved evidence with wrong session",
            &termination_unknown,
            |binding| {
                binding.cancellation_unresolved.as_mut().unwrap().session_id =
                    "session-b".to_owned();
            },
        ),
        mutation(
            "unresolved evidence with empty session",
            &termination_unknown,
            |binding| {
                binding.cancellation_unresolved.as_mut().unwrap().session_id = String::new();
            },
        ),
        mutation(
            "unresolved evidence with oversized session",
            &termination_unknown,
            |binding| {
                binding.cancellation_unresolved.as_mut().unwrap().session_id = "s".repeat(256);
            },
        ),
        mutation("empty disposition id", &termination_unknown, |binding| {
            binding
                .cancellation_unresolved
                .as_mut()
                .unwrap()
                .disposition_id = String::new();
        }),
        mutation(
            "oversized disposition id",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .disposition_id = "d".repeat(256);
            },
        ),
        mutation(
            "unresolved evidence with wrong termination request id",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .termination_request_id = fixture_other_request_id();
            },
        ),
        mutation(
            "unresolved evidence with wrong execution request id",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .execution_request_id = fixture_other_request_id();
            },
        ),
        mutation(
            "unresolved evidence with wrong execution digest",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .execution_request_digest = fixture_other_digest();
            },
        ),
        mutation(
            "unresolved evidence with empty reason",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .reason_code = String::new();
            },
        ),
        mutation(
            "unresolved evidence with oversized reason",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .reason_code = "r".repeat(129);
            },
        ),
        mutation(
            "unresolved evidence with invalid reason",
            &termination_unknown,
            |binding| {
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .reason_code = "TimedOut".to_owned();
            },
        ),
        mutation(
            "unresolved evidence before termination start",
            &termination_unknown,
            |binding| {
                let before_start = binding.termination_request.as_ref().unwrap().created_at
                    - time::Duration::nanoseconds(1);
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .recorded_at = before_start;
            },
        ),
        mutation(
            "unresolved evidence after termination deadline",
            &termination_unknown,
            |binding| {
                let after_deadline = binding.termination_request.as_ref().unwrap().valid_until
                    + time::Duration::nanoseconds(1);
                binding
                    .cancellation_unresolved
                    .as_mut()
                    .unwrap()
                    .recorded_at = after_deadline;
            },
        ),
    ];

    for (name, binding) in cases {
        let (mut store, _dir) = test_store();
        let attempt_id = binding.attempt_id.clone();
        let result = store.insert(&CanonicalDocument::ExecutionBinding(binding));
        assert!(
            matches!(
                result,
                Err(StoreError::Contract(
                    ContractError::CancellationEvidenceMismatch
                ))
            ),
            "{name} returned {result:?}"
        );
        assert_no_writes(&store, &attempt_id, name);
    }

    // `Sha256Digest` rejects malformed text at construction, so mutate its wire
    // field while retaining the same store-boundary and no-write assertions.
    let invalid_authority_digest = format!("sha256:{}", "g".repeat(64));
    assert!(Sha256Digest::parse(&invalid_authority_digest).is_err());
    let mut invalid_authority = serde_json::to_value(&acknowledged_terminated).unwrap();
    invalid_authority["cancellation_acknowledgement"]["authority_evidence_digest"] =
        json!(invalid_authority_digest);
    let invalid_authority = serde_json::to_vec(&invalid_authority).unwrap();
    let (mut store, _dir) = test_store();
    let result = store.ingest(&invalid_authority);
    assert!(
        matches!(
            result,
            Err(StoreError::Contract(
                ContractError::CancellationEvidenceMismatch
            ))
        ),
        "invalid authority digest returned {result:?}"
    );
    assert_no_writes(
        &store,
        &acknowledged_terminated.attempt_id,
        "invalid authority digest",
    );
}

#[test]
fn direct_insert_rejects_wrong_termination_request_id() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    binding
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .termination_request_id = fixture_other_request_id();
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_termination_before_execution_request() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    let before_execution = binding.request_created_at - time::Duration::nanoseconds(1);
    binding.termination_request.as_mut().unwrap().created_at = before_execution;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_acknowledgement_outside_termination_window() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    let after_deadline =
        binding.termination_request.as_ref().unwrap().valid_until + time::Duration::nanoseconds(1);
    binding
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .acknowledged_at = after_deadline;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_acknowledgement_before_termination_window() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    let before_start =
        binding.termination_request.as_ref().unwrap().created_at - time::Duration::nanoseconds(1);
    binding
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .acknowledged_at = before_start;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_unresolved_outside_termination_window() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_unresolved_execution_binding();
    let after_deadline =
        binding.termination_request.as_ref().unwrap().valid_until + time::Duration::nanoseconds(1);
    binding
        .cancellation_unresolved
        .as_mut()
        .unwrap()
        .recorded_at = after_deadline;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_rejects_unresolved_before_termination_window() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_unresolved_execution_binding();
    let before_start =
        binding.termination_request.as_ref().unwrap().created_at - time::Duration::nanoseconds(1);
    binding
        .cancellation_unresolved
        .as_mut()
        .unwrap()
        .recorded_at = before_start;
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(
            ContractError::CancellationEvidenceMismatch
        ))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_insert_accepts_acknowledgement_at_termination_window_boundaries() {
    for use_deadline in [false, true] {
        let (mut store, _dir) = test_store();
        let mut binding = fixture_acknowledged_execution_binding();
        let termination = binding.termination_request.as_ref().unwrap();
        let evidence_time = if use_deadline {
            termination.valid_until
        } else {
            termination.created_at
        };
        binding
            .cancellation_acknowledgement
            .as_mut()
            .unwrap()
            .acknowledged_at = evidence_time;
        let attempt_id = binding.attempt_id.clone();
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding))
            .unwrap();
        assert_eq!(
            store
                .execution_binding_revisions(&attempt_id)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn direct_insert_accepts_unresolved_at_termination_window_boundaries() {
    for use_deadline in [false, true] {
        let (mut store, _dir) = test_store();
        let mut binding = fixture_unresolved_execution_binding();
        let termination = binding.termination_request.as_ref().unwrap();
        let evidence_time = if use_deadline {
            termination.valid_until
        } else {
            termination.created_at
        };
        binding
            .cancellation_unresolved
            .as_mut()
            .unwrap()
            .recorded_at = evidence_time;
        let attempt_id = binding.attempt_id.clone();
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding))
            .unwrap();
        assert_eq!(
            store
                .execution_binding_revisions(&attempt_id)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn direct_insert_accepts_termination_window_after_execution_deadline() {
    let (mut store, _dir) = test_store();
    let binding = fixture_acknowledged_binding_after_execution_deadline();
    assert!(binding.termination_request.as_ref().unwrap().created_at > binding.request_valid_until);
    let attempt_id = binding.attempt_id.clone();
    store
        .insert(&CanonicalDocument::ExecutionBinding(binding))
        .unwrap();
    assert_eq!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn direct_insert_accepts_termination_at_execution_creation_boundary() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_acknowledged_execution_binding();
    let execution_created_at = binding.request_created_at;
    binding.termination_request.as_mut().unwrap().created_at = execution_created_at;
    let attempt_id = binding.attempt_id.clone();
    store
        .insert(&CanonicalDocument::ExecutionBinding(binding))
        .unwrap();
    assert_eq!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn direct_insert_rejects_revision_u64_overflow_without_writing() {
    let (mut store, _dir) = test_store();
    let mut binding = fixture_execution_binding_revision_1();
    binding.revision = u64::MAX;
    binding.previous_revision_digest = Some(fixture_digest('a'));
    let attempt_id = binding.attempt_id.clone();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(binding)),
        Err(StoreError::Contract(_))
    ));
    assert!(
        store
            .execution_binding_revisions(&attempt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn execution_binding_revision_appends_termination_outcomes_without_record_conflict() {
    let (mut acknowledged_store, _acknowledged_dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    acknowledged_store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    let requested = fixture_termination_requested_revision(&initial);
    acknowledged_store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    let acknowledged = fixture_acknowledged_revision(&requested);
    acknowledged_store
        .insert(&CanonicalDocument::ExecutionBinding(acknowledged.clone()))
        .unwrap();
    assert_eq!(
        acknowledged_store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial.clone(), requested.clone(), acknowledged.clone()]
    );
    assert_eq!(
        acknowledged_store
            .load(SchemaKind::ExecutionBinding, &initial.attempt_id)
            .unwrap(),
        Some(CanonicalDocument::ExecutionBinding(acknowledged))
    );

    let (mut unresolved_store, _unresolved_dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    unresolved_store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    let requested = fixture_termination_requested_revision(&initial);
    unresolved_store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    let unresolved = fixture_unresolved_revision(&requested);
    unresolved_store
        .insert(&CanonicalDocument::ExecutionBinding(unresolved.clone()))
        .unwrap();
    assert_eq!(
        unresolved_store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested, unresolved]
    );
}

#[test]
fn execution_binding_revision_rejects_forks_gaps_and_changed_correlation() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();

    let mut gap = fixture_termination_requested_revision(&initial);
    gap.revision = 3;
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(gap)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));

    let mut wrong_previous = fixture_termination_requested_revision(&initial);
    wrong_previous.previous_revision_digest = Some(fixture_other_digest());
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(wrong_previous)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));

    let mut changed_correlation = fixture_termination_requested_revision(&initial);
    changed_correlation.request_digest = fixture_other_digest();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(changed_correlation)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));

    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial]
    );
}

#[test]
fn execution_binding_revision_replay_is_idempotent() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    for revision in [&initial, &requested] {
        store
            .insert(&CanonicalDocument::ExecutionBinding((*revision).clone()))
            .unwrap();
    }
    for revision in [&initial, &requested] {
        store
            .insert(&CanonicalDocument::ExecutionBinding((*revision).clone()))
            .unwrap();
    }
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested]
    );
}

#[test]
fn execution_binding_revision_rejects_same_revision_changed_bytes() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    for revision in [&initial, &requested] {
        let mut changed = (*revision).clone();
        changed.event_cursor = Some("cursor:changed".to_owned());
        assert!(matches!(
            store.insert(&CanonicalDocument::ExecutionBinding(changed)),
            Err(StoreError::ExecutionBindingRevisionConflict { .. })
        ));
    }
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested]
    );
}

#[test]
fn execution_binding_revision_rejects_changed_reason_replay() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    let mut changed_reason = requested.clone();
    changed_reason.termination_reason_code = Some("different_reason".to_owned());
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(changed_reason)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested]
    );
}

fn assert_next_revision_conflict(mutate: impl FnOnce(&mut ExecutionBinding)) {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    let mut candidate = fixture_next_not_requested_revision(&initial);
    mutate(&mut candidate);
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(candidate)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial]
    );
}

fn with_different_offset(timestamp: OffsetDateTime) -> OffsetDateTime {
    timestamp.to_offset(UtcOffset::from_hms(1, 0, 0).unwrap())
}

fn assert_offset_only_next_revision_conflict(
    persisted: Vec<ExecutionBinding>,
    candidate: ExecutionBinding,
    original_timestamp: OffsetDateTime,
    changed_timestamp: OffsetDateTime,
) {
    assert_eq!(original_timestamp, changed_timestamp);
    assert_ne!(original_timestamp.offset(), changed_timestamp.offset());
    assert_ne!(
        original_timestamp.format(&Rfc3339).unwrap(),
        changed_timestamp.format(&Rfc3339).unwrap()
    );
    candidate.validate().unwrap();
    let previous = persisted.last().unwrap();
    assert_eq!(
        candidate.previous_revision_digest.as_ref(),
        Some(&digest(previous).unwrap())
    );

    let (mut store, _dir) = test_store();
    for binding in &persisted {
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
            .unwrap();
    }
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(candidate)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        store
            .execution_binding_revisions(&previous.attempt_id)
            .unwrap(),
        persisted
    );
}

#[test]
fn execution_binding_revision_freezes_request_timestamp_offsets() {
    let initial = fixture_execution_binding_revision_1();
    let mut changed_created_at = next_revision(&initial);
    let original_created_at = changed_created_at.request_created_at;
    changed_created_at.request_created_at = with_different_offset(original_created_at);
    assert_offset_only_next_revision_conflict(
        vec![initial.clone()],
        changed_created_at,
        original_created_at,
        with_different_offset(original_created_at),
    );

    let mut changed_valid_until = next_revision(&initial);
    let original_valid_until = changed_valid_until.request_valid_until;
    changed_valid_until.request_valid_until = with_different_offset(original_valid_until);
    assert_offset_only_next_revision_conflict(
        vec![initial],
        changed_valid_until,
        original_valid_until,
        with_different_offset(original_valid_until),
    );
}

#[test]
fn execution_binding_revision_freezes_termination_timestamp_offsets() {
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    let mut changed_created_at = next_revision(&requested);
    let original_created_at = changed_created_at
        .termination_request
        .as_ref()
        .unwrap()
        .created_at;
    changed_created_at
        .termination_request
        .as_mut()
        .unwrap()
        .created_at = with_different_offset(original_created_at);
    assert_offset_only_next_revision_conflict(
        vec![initial.clone(), requested.clone()],
        changed_created_at,
        original_created_at,
        with_different_offset(original_created_at),
    );

    let mut changed_valid_until = next_revision(&requested);
    let original_valid_until = changed_valid_until
        .termination_request
        .as_ref()
        .unwrap()
        .valid_until;
    changed_valid_until
        .termination_request
        .as_mut()
        .unwrap()
        .valid_until = with_different_offset(original_valid_until);
    assert_offset_only_next_revision_conflict(
        vec![initial, requested],
        changed_valid_until,
        original_valid_until,
        with_different_offset(original_valid_until),
    );
}

#[test]
fn execution_binding_revision_freezes_acknowledgement_timestamp_offset() {
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    let acknowledged = fixture_acknowledged_revision(&requested);
    let mut candidate = next_revision(&acknowledged);
    let original = candidate
        .cancellation_acknowledgement
        .as_ref()
        .unwrap()
        .acknowledged_at;
    candidate
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .acknowledged_at = with_different_offset(original);
    assert_offset_only_next_revision_conflict(
        vec![initial, requested, acknowledged],
        candidate,
        original,
        with_different_offset(original),
    );
}

#[test]
fn execution_binding_revision_freezes_unresolved_timestamp_offset() {
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    let unresolved = fixture_unresolved_revision(&requested);
    let mut candidate = next_revision(&unresolved);
    let original = candidate
        .cancellation_unresolved
        .as_ref()
        .unwrap()
        .recorded_at;
    candidate
        .cancellation_unresolved
        .as_mut()
        .unwrap()
        .recorded_at = with_different_offset(original);
    assert_offset_only_next_revision_conflict(
        vec![initial, requested, unresolved],
        candidate,
        original,
        with_different_offset(original),
    );
}

#[test]
fn execution_binding_revision_rejects_every_frozen_execution_field_change() {
    assert_next_revision_conflict(|revision| {
        revision.attempt_id = fixture_other_attempt_id();
    });
    assert_next_revision_conflict(|revision| {
        revision.familiar_snapshot_id = fixture_other_snapshot_id();
    });
    assert_next_revision_conflict(|revision| {
        revision.project_id = fixture_other_project_id();
    });
    assert_next_revision_conflict(|revision| {
        revision.request_id = fixture_other_request_id();
    });
    assert_next_revision_conflict(|revision| {
        revision.request_digest = fixture_other_digest();
    });
    assert_next_revision_conflict(|revision| {
        revision.request_created_at += time::Duration::nanoseconds(1);
    });
    assert_next_revision_conflict(|revision| {
        revision.request_valid_until += time::Duration::nanoseconds(1);
    });
    assert_next_revision_conflict(|revision| {
        revision.coven_contract_version = fixture_other_coven_contract_version();
    });
}

#[test]
fn execution_binding_revision_rejects_session_and_termination_rebinding() {
    let (mut session_store, _session_dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let bound = fixture_session_bound_revision(&initial, "session-a");
    session_store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    session_store
        .insert(&CanonicalDocument::ExecutionBinding(bound.clone()))
        .unwrap();
    let rebound = fixture_session_bound_revision(&bound, "session-b");
    assert!(matches!(
        session_store.insert(&CanonicalDocument::ExecutionBinding(rebound)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    let mut cleared = fixture_next_not_requested_revision(&bound);
    cleared.coven_session_id = None;
    assert!(matches!(
        session_store.insert(&CanonicalDocument::ExecutionBinding(cleared)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        session_store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, bound]
    );

    let (mut termination_store, _termination_dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    termination_store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    termination_store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    let mut changed_id = fixture_next_termination_requested_revision(&requested);
    changed_id
        .termination_request
        .as_mut()
        .unwrap()
        .termination_request_id = fixture_other_request_id();
    assert!(matches!(
        termination_store.insert(&CanonicalDocument::ExecutionBinding(changed_id)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    let mut changed_created_at = fixture_next_termination_requested_revision(&requested);
    changed_created_at
        .termination_request
        .as_mut()
        .unwrap()
        .created_at += time::Duration::nanoseconds(1);
    assert!(matches!(
        termination_store.insert(&CanonicalDocument::ExecutionBinding(changed_created_at)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    let mut changed_valid_until = fixture_next_termination_requested_revision(&requested);
    changed_valid_until
        .termination_request
        .as_mut()
        .unwrap()
        .valid_until += time::Duration::nanoseconds(1);
    assert!(matches!(
        termination_store.insert(&CanonicalDocument::ExecutionBinding(changed_valid_until)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    let mut changed_reason = fixture_next_termination_requested_revision(&requested);
    changed_reason.termination_reason_code = Some("different_reason".to_owned());
    assert!(matches!(
        termination_store.insert(&CanonicalDocument::ExecutionBinding(changed_reason)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        termination_store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested]
    );
}

#[test]
fn execution_binding_revision_rejects_termination_correlation_removal() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    let cleared = fixture_not_requested_revision_after(&requested);
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(cleared)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial, requested]
    );
}

#[test]
fn execution_binding_revision_rejects_timestamp_regression() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    for non_increasing in [
        initial.revision_created_at,
        initial.revision_created_at - time::Duration::nanoseconds(1),
    ] {
        let mut regressed = fixture_termination_requested_revision(&initial);
        regressed.revision_created_at = non_increasing;
        assert!(matches!(
            store.insert(&CanonicalDocument::ExecutionBinding(regressed)),
            Err(StoreError::ExecutionBindingRevisionConflict { .. })
        ));
    }
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap(),
        vec![initial]
    );
}

#[test]
fn concurrent_execution_binding_forks_have_one_durable_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    let mut first = Store::open(&path).unwrap();
    let initial = fixture_execution_binding_revision_1();
    first
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    let second = Store::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let left = fixture_termination_requested_revision(&initial);
    let mut right = fixture_termination_requested_revision(&initial);
    right.event_cursor = Some("cursor:competing".to_owned());

    let left_barrier = Arc::clone(&barrier);
    let left_thread = std::thread::spawn(move || {
        let mut store = first;
        left_barrier.wait();
        store.insert(&CanonicalDocument::ExecutionBinding(left))
    });
    let right_barrier = Arc::clone(&barrier);
    let right_thread = std::thread::spawn(move || {
        let mut store = second;
        right_barrier.wait();
        store.insert(&CanonicalDocument::ExecutionBinding(right))
    });

    let results = [left_thread.join().unwrap(), right_thread.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(StoreError::ExecutionBindingRevisionConflict { .. })
            ))
            .count(),
        1
    );
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .execution_binding_revisions(&initial.attempt_id)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn transition_versions_are_monotonic_and_append_only() {
    let (mut store, _dir) = test_store();
    store
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    assert!(matches!(
        store.append_transition(&transition(1, None, "running")),
        Err(StoreError::TransitionConflict { .. })
    ));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

#[test]
fn transition_validation_rejects_wrong_id_kind_and_digest_without_writing() {
    let (mut store, _dir) = test_store();
    let mut wrong_kind = transition(1, None, "admitted");
    wrong_kind.record_id =
        RecordId::parse(RecordKind::Intent, "int_01J00000000000000000000000").unwrap();
    assert!(matches!(
        store.append_transition(&wrong_kind),
        Err(StoreError::Contract(ContractError::WrongRecordKind { .. }))
    ));

    let mut wrong_digest = transition(1, None, "admitted");
    wrong_digest.transition_digest = fixture_digest('f');
    assert!(matches!(
        store.append_transition(&wrong_digest),
        Err(StoreError::Contract(ContractError::DigestMismatch { .. }))
    ));
    assert_eq!(store.count_transitions().unwrap(), 0);
}

#[test]
fn transition_append_requires_exact_version_and_prior_state() {
    let (mut store, _dir) = test_store();
    store
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    assert!(matches!(
        store.append_transition(&transition(3, Some("admitted"), "running")),
        Err(StoreError::TransitionConflict { .. })
    ));
    assert!(matches!(
        store.append_transition(&transition(2, Some("draft"), "running")),
        Err(StoreError::TransitionConflict { .. })
    ));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

#[test]
fn concurrent_transition_forks_have_one_durable_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    let mut first = Store::open(&path).unwrap();
    first
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    let second = Store::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let left_barrier = Arc::clone(&barrier);
    let left_thread = std::thread::spawn(move || {
        let mut store = first;
        left_barrier.wait();
        store.append_transition(&transition(2, Some("admitted"), "running"))
    });
    let right_barrier = Arc::clone(&barrier);
    let right_thread = std::thread::spawn(move || {
        let mut store = second;
        right_barrier.wait();
        store.append_transition(&transition(2, Some("admitted"), "cancelled"))
    });

    let results = [left_thread.join().unwrap(), right_thread.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::TransitionConflict { .. })))
            .count(),
        1
    );
    assert_eq!(Store::open(&path).unwrap().count_transitions().unwrap(), 2);
}

#[test]
fn transition_contract_rejects_invalid_states_and_version_overflow_without_writing() {
    let (store, _dir) = test_store();
    let invalid = [
        Transition::new(
            SchemaKind::ExecutionBinding,
            fixture_attempt_id(),
            1,
            Some("prior".to_owned()),
            "UPPER".to_owned(),
            at("2026-08-05T12:00:00Z"),
        ),
        Transition::new(
            SchemaKind::ExecutionBinding,
            fixture_attempt_id(),
            2,
            None,
            "running".to_owned(),
            at("2026-08-05T12:00:00Z"),
        ),
        Transition::new(
            SchemaKind::ExecutionBinding,
            fixture_attempt_id(),
            1,
            Some("draft".to_owned()),
            "admitted".to_owned(),
            at("2026-08-05T12:00:00Z"),
        ),
        Transition::new(
            SchemaKind::ExecutionBinding,
            fixture_attempt_id(),
            2,
            Some("running".to_owned()),
            "running".to_owned(),
            at("2026-08-05T12:00:00Z"),
        ),
        Transition::new(
            SchemaKind::ExecutionBinding,
            fixture_attempt_id(),
            u64::MAX,
            Some("running".to_owned()),
            "done".to_owned(),
            at("2026-08-05T12:00:00Z"),
        ),
    ];
    assert!(invalid.into_iter().all(|result| result.is_err()));
    assert_eq!(store.count_transitions().unwrap(), 0);
}

#[derive(Serialize)]
struct ExpectedTransitionDigestInput<'a> {
    kind: SchemaKind,
    record_id: &'a RecordId,
    record_version: u64,
    from_state: &'a Option<String>,
    to_state: &'a str,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

#[test]
fn transition_digest_uses_the_exact_owned_canonical_contract() {
    let transition = transition(2, Some("admitted"), "running");
    assert_eq!(
        transition.transition_digest,
        digest(&ExpectedTransitionDigestInput {
            kind: transition.kind,
            record_id: &transition.record_id,
            record_version: transition.record_version,
            from_state: &transition.from_state,
            to_state: &transition.to_state,
            created_at: transition.created_at,
        })
        .unwrap()
    );
    transition.validate().unwrap();
}

#[test]
fn execution_cancellation_legal_forward_paths_and_stable_terminal_revisions_append() {
    for terminal_kind in [
        CancellationState::AcknowledgedTerminated,
        CancellationState::AcknowledgedAlreadyTerminal,
        CancellationState::TerminationUnknown,
    ] {
        let (mut store, _dir) = test_store();
        let initial = fixture_execution_binding_revision_1();
        let unchanged = fixture_next_not_requested_revision(&initial);
        let requested = fixture_termination_requested_revision(&unchanged);
        let requested_again = fixture_next_termination_requested_revision(&requested);
        for binding in [&initial, &unchanged, &requested, &requested_again] {
            store
                .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
                .unwrap();
        }

        let terminal = match terminal_kind {
            CancellationState::AcknowledgedTerminated => {
                fixture_acknowledged_revision(&requested_again)
            }
            CancellationState::AcknowledgedAlreadyTerminal => {
                fixture_already_terminal_revision(&requested_again)
            }
            CancellationState::TerminationUnknown => fixture_unresolved_revision(&requested_again),
            _ => unreachable!(),
        };
        store
            .insert(&CanonicalDocument::ExecutionBinding(terminal.clone()))
            .unwrap();

        let mut stable = next_revision(&terminal);
        stable.event_cursor = Some("cursor:after-terminal".to_owned());
        store
            .insert(&CanonicalDocument::ExecutionBinding(stable.clone()))
            .unwrap();
        assert_eq!(
            store
                .execution_binding_revisions(&initial.attempt_id)
                .unwrap(),
            vec![
                initial,
                unchanged,
                requested,
                requested_again,
                terminal,
                stable
            ]
        );
    }
}

#[test]
fn execution_cancellation_rejects_direct_not_requested_to_any_terminal_state() {
    for terminal_kind in [
        CancellationState::AcknowledgedTerminated,
        CancellationState::AcknowledgedAlreadyTerminal,
        CancellationState::TerminationUnknown,
    ] {
        let (mut store, _dir) = test_store();
        let initial = fixture_execution_binding_revision_1();
        store
            .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
            .unwrap();
        let mut terminal = fixture_termination_requested_revision(&initial);
        terminal.cancellation_state = terminal_kind;
        match terminal_kind {
            CancellationState::AcknowledgedTerminated => {
                terminal.cancellation_acknowledgement = Some(acknowledgement(&terminal));
            }
            CancellationState::AcknowledgedAlreadyTerminal => {
                let mut evidence = acknowledgement(&terminal);
                evidence.kind = CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
                terminal.cancellation_acknowledgement = Some(evidence);
            }
            CancellationState::TerminationUnknown => {
                terminal.cancellation_unresolved = Some(unresolved(&terminal));
            }
            _ => unreachable!(),
        }
        assert!(matches!(
            store.insert(&CanonicalDocument::ExecutionBinding(terminal)),
            Err(StoreError::ExecutionBindingRevisionConflict { .. })
        ));
        assert_eq!(
            store
                .execution_binding_revisions(&initial.attempt_id)
                .unwrap(),
            vec![initial]
        );
    }
}

#[test]
fn execution_cancellation_rejects_requested_and_terminal_regressions() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    for binding in [&initial, &requested] {
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
            .unwrap();
    }
    let requested_regression = fixture_not_requested_revision_after(&requested);
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(requested_regression)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));

    let terminal = fixture_acknowledged_revision(&requested);
    store
        .insert(&CanonicalDocument::ExecutionBinding(terminal.clone()))
        .unwrap();
    let mut terminal_regression = next_revision(&terminal);
    terminal_regression.cancellation_state = CancellationState::TerminationRequested;
    terminal_regression.cancellation_acknowledgement = None;
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(terminal_regression)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
    let terminal_removal = fixture_not_requested_revision_after(&terminal);
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(terminal_removal)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
}

#[test]
fn execution_cancellation_rejects_terminal_switches_and_evidence_mutation() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    let terminal = fixture_acknowledged_revision(&requested);
    for binding in [&initial, &requested, &terminal] {
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
            .unwrap();
    }

    let mut switched = next_revision(&terminal);
    switched.cancellation_state = CancellationState::AcknowledgedAlreadyTerminal;
    switched.cancellation_acknowledgement.as_mut().unwrap().kind =
        CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(switched)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));

    let mut mutated = next_revision(&terminal);
    mutated
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .authority_evidence_digest = fixture_other_digest();
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(mutated)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
}

#[test]
fn execution_cancellation_rejects_unknown_to_acknowledged() {
    let (mut store, _dir) = test_store();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    let unknown = fixture_unresolved_revision(&requested);
    for binding in [&initial, &requested, &unknown] {
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
            .unwrap();
    }
    let mut acknowledged = next_revision(&unknown);
    acknowledged.cancellation_state = CancellationState::AcknowledgedTerminated;
    acknowledged.cancellation_unresolved = None;
    acknowledged.cancellation_acknowledgement = Some(acknowledgement(&acknowledged));
    assert!(matches!(
        store.insert(&CanonicalDocument::ExecutionBinding(acknowledged)),
        Err(StoreError::ExecutionBindingRevisionConflict { .. })
    ));
}

#[test]
fn transition_exact_replay_is_idempotent_and_divergent_identity_conflicts() {
    let (mut store, _dir) = test_store();
    let original = transition(1, None, "admitted");
    store.append_transition(&original).unwrap();
    store.append_transition(&original).unwrap();
    assert_eq!(store.count_transitions().unwrap(), 1);

    let changed_field = transition(1, None, "running");
    assert!(matches!(
        store.append_transition(&changed_field),
        Err(StoreError::TransitionConflict { .. })
    ));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

#[test]
fn transition_exact_replay_authenticates_the_stored_history() {
    let (mut store, _dir, path) = test_store_with_path();
    let original = transition(1, None, "admitted");
    store.append_transition(&original).unwrap();
    raw_connection(&path)
        .execute(
            "UPDATE transitions SET transition_digest = ?1 WHERE record_version = 1",
            [fixture_other_digest().as_str()],
        )
        .unwrap();
    assert_database_corruption(store.append_transition(&original));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

#[test]
fn transition_append_rejects_prior_digest_corruption_without_writing() {
    let (mut store, _dir, path) = test_store_with_path();
    store
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    raw_connection(&path)
        .execute(
            "UPDATE transitions SET transition_digest = ?1 WHERE record_version = 1",
            [fixture_other_digest().as_str()],
        )
        .unwrap();

    assert_database_corruption(store.append_transition(&transition(
        2,
        Some("admitted"),
        "running",
    )));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

#[test]
fn transition_append_rejects_prior_state_corruption_without_writing() {
    for (column, version, value, next) in [
        (
            "to_state",
            1,
            "queued",
            transition(2, Some("admitted"), "running"),
        ),
        (
            "from_state",
            2,
            "queued",
            transition(3, Some("running"), "completed"),
        ),
    ] {
        let (mut store, _dir, path) = test_store_with_path();
        store
            .append_transition(&transition(1, None, "admitted"))
            .unwrap();
        if version == 2 {
            store
                .append_transition(&transition(2, Some("admitted"), "running"))
                .unwrap();
        }
        raw_connection(&path)
            .execute(
                &format!("UPDATE transitions SET {column} = ?1 WHERE record_version = ?2"),
                rusqlite::params![value, version],
            )
            .unwrap();
        let count = store.count_transitions().unwrap();

        assert_database_corruption(store.append_transition(&next));
        assert_eq!(store.count_transitions().unwrap(), count);
    }
}

#[test]
fn transition_append_rejects_stored_version_gap_without_writing() {
    let (mut store, _dir, path) = test_store_with_path();
    store
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    store
        .append_transition(&transition(2, Some("admitted"), "running"))
        .unwrap();
    raw_connection(&path)
        .execute(
            "UPDATE transitions SET record_version = 3 WHERE record_version = 2",
            [],
        )
        .unwrap();

    assert_database_corruption(store.append_transition(&transition(
        4,
        Some("running"),
        "completed",
    )));
    assert_eq!(store.count_transitions().unwrap(), 2);
}

#[test]
fn transition_append_rejects_noncanonical_stored_timestamp_without_writing() {
    let (mut store, _dir, path) = test_store_with_path();
    store
        .append_transition(&transition(1, None, "admitted"))
        .unwrap();
    raw_connection(&path)
        .execute(
            "
            UPDATE transitions
            SET created_at = '2026-08-05T12:00:01+00:00'
            WHERE record_version = 1
            ",
            [],
        )
        .unwrap();

    assert_database_corruption(store.append_transition(&transition(
        2,
        Some("admitted"),
        "running",
    )));
    assert_eq!(store.count_transitions().unwrap(), 1);
}

fn insert_intent_for_tamper() -> (
    Store,
    tempfile::TempDir,
    PathBuf,
    CanonicalDocument,
    RecordId,
) {
    let (mut store, dir, path) = test_store_with_path();
    let intent = fixture_intent("Review A");
    let id = intent.intent_id.clone();
    let document = CanonicalDocument::Intent(intent);
    store.insert(&document).unwrap();
    (store, dir, path, document, id)
}

fn assert_canonical_corruption_across_access_paths(
    store: &mut Store,
    document: &CanonicalDocument,
    id: &RecordId,
) {
    assert_database_corruption(store.load(SchemaKind::Intent, id));
    assert_database_corruption(store.load_canonical_bytes(SchemaKind::Intent, id));
    assert_database_corruption(store.insert(document));
}

#[test]
fn canonical_record_detects_digest_only_bytes_noncanonical_and_malformed_tamper() {
    for case in ["digest", "bytes", "noncanonical", "malformed"] {
        let (mut store, _dir, path, document, id) = insert_intent_for_tamper();
        let connection = raw_connection(&path);
        match case {
            "digest" => {
                connection
                    .execute(
                        "UPDATE canonical_records SET digest = ?1",
                        [fixture_other_digest().as_str()],
                    )
                    .unwrap();
            }
            "bytes" => {
                let changed =
                    canonical_bytes(&CanonicalDocument::Intent(fixture_intent("Review B")))
                        .unwrap();
                connection
                    .execute(
                        "UPDATE canonical_records SET canonical_json = ?1",
                        [changed],
                    )
                    .unwrap();
            }
            "noncanonical" => {
                let mut bytes = vec![b' '];
                bytes.extend(canonical_bytes(&document).unwrap());
                connection
                    .execute("UPDATE canonical_records SET canonical_json = ?1", [bytes])
                    .unwrap();
            }
            "malformed" => {
                connection
                    .execute(
                        "UPDATE canonical_records SET canonical_json = ?1",
                        [b"{".as_slice()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);
        assert_canonical_corruption_across_access_paths(&mut store, &document, &id);
    }
}

#[test]
fn canonical_record_detects_schema_kind_and_record_id_metadata_tamper() {
    let (mut schema_store, _dir, schema_path, document, id) = insert_intent_for_tamper();
    raw_connection(&schema_path)
        .execute(
            "UPDATE canonical_records SET schema_version = 'psyche.graph.v1'",
            [],
        )
        .unwrap();
    assert_canonical_corruption_across_access_paths(&mut schema_store, &document, &id);

    let (mut id_store, _dir, id_path, _document, _id) = insert_intent_for_tamper();
    let other_id = record_id(RecordKind::Intent, "01J00000000000000000000009");
    raw_connection(&id_path)
        .execute(
            "UPDATE canonical_records SET record_id = ?1",
            [other_id.as_str()],
        )
        .unwrap();
    let mut other_intent = fixture_intent("Review A");
    other_intent.intent_id = other_id.clone();
    let other_document = CanonicalDocument::Intent(other_intent);
    assert_canonical_corruption_across_access_paths(&mut id_store, &other_document, &other_id);

    let (kind_store, _dir, kind_path, _document, _id) = insert_intent_for_tamper();
    let graph_id = record_id(RecordKind::Graph, "01J00000000000000000000010");
    raw_connection(&kind_path)
        .execute(
            "UPDATE canonical_records SET kind = 'graph', record_id = ?1",
            [graph_id.as_str()],
        )
        .unwrap();
    assert_database_corruption(kind_store.load(SchemaKind::Graph, &graph_id));
    assert_database_corruption(kind_store.load_canonical_bytes(SchemaKind::Graph, &graph_id));
}

fn binding_chain_for_tamper() -> (
    Store,
    tempfile::TempDir,
    PathBuf,
    ExecutionBinding,
    ExecutionBinding,
) {
    let (mut store, dir, path) = test_store_with_path();
    let initial = fixture_execution_binding_revision_1();
    let requested = fixture_termination_requested_revision(&initial);
    for binding in [&initial, &requested] {
        store
            .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
            .unwrap();
    }
    (store, dir, path, initial, requested)
}

fn assert_execution_corruption_across_access_paths(
    store: &mut Store,
    initial: &ExecutionBinding,
    requested: &ExecutionBinding,
) {
    assert_database_corruption(store.execution_binding_revisions(&initial.attempt_id));
    assert_database_corruption(store.load(SchemaKind::ExecutionBinding, &initial.attempt_id));
    assert_database_corruption(
        store.load_canonical_bytes(SchemaKind::ExecutionBinding, &initial.attempt_id),
    );
    assert_database_corruption(store.insert(&CanonicalDocument::ExecutionBinding(initial.clone())));
    let append = fixture_next_termination_requested_revision(requested);
    assert_database_corruption(store.insert(&CanonicalDocument::ExecutionBinding(append)));
}

#[test]
fn execution_revision_chain_detects_blob_digest_link_schema_timestamp_and_gap_tamper() {
    for case in [
        "revision_blob",
        "digest",
        "previous_digest",
        "schema",
        "timestamp",
        "gap",
    ] {
        let (mut store, _dir, path, initial, requested) = binding_chain_for_tamper();
        let connection = raw_connection(&path);
        connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        match case {
            "revision_blob" => {
                let first: Vec<u8> = connection
                    .query_row(
                        "SELECT canonical_json FROM execution_binding_revisions WHERE revision = 1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET canonical_json = ?1 WHERE revision = 2",
                        [first],
                    )
                    .unwrap();
            }
            "digest" => {
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET digest = ?1 WHERE revision = 2",
                        [fixture_other_digest().as_str()],
                    )
                    .unwrap();
            }
            "previous_digest" => {
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET previous_revision_digest = ?1 WHERE revision = 2",
                        [fixture_other_digest().as_str()],
                    )
                    .unwrap();
            }
            "schema" => {
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET schema_version = 'psyche.intent.v1' WHERE revision = 2",
                        [],
                    )
                    .unwrap();
            }
            "timestamp" => {
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET created_at = '2026-08-05T12:00:00Z' WHERE revision = 2",
                        [],
                    )
                    .unwrap();
            }
            "gap" => {
                connection
                    .execute(
                        "UPDATE execution_binding_revisions SET revision = 3 WHERE revision = 2",
                        [],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);
        assert_execution_corruption_across_access_paths(&mut store, &initial, &requested);
    }
}

#[test]
fn execution_revision_chain_detects_attempt_metadata_tamper() {
    let (store, _dir, path, _initial, _requested) = binding_chain_for_tamper();
    let other_attempt = fixture_other_attempt_id();
    let connection = raw_connection(&path);
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection
        .execute(
            "UPDATE execution_binding_revisions SET attempt_id = ?1 WHERE revision = 2",
            [other_attempt.as_str()],
        )
        .unwrap();
    drop(connection);
    assert_database_corruption(store.execution_binding_revisions(&other_attempt));
    assert_database_corruption(store.load(SchemaKind::ExecutionBinding, &other_attempt));
    assert_database_corruption(
        store.load_canonical_bytes(SchemaKind::ExecutionBinding, &other_attempt),
    );
}

proptest::proptest! {
    #[test]
    fn reinsertion_never_changes_stored_bytes(outcome in "[a-zA-Z0-9 ]{1,80}") {
        let (mut store, _dir) = test_store();
        let intent = fixture_intent(&outcome);
        let id = intent.record_id().clone();
        let before = canonical_bytes(&intent).unwrap();
        store.insert(&CanonicalDocument::Intent(intent.clone())).unwrap();
        store.insert(&CanonicalDocument::Intent(intent)).unwrap();
        let after = store.load_canonical_bytes(SchemaKind::Intent, &id).unwrap().unwrap();
        proptest::prop_assert_eq!(before, after);
    }
}
