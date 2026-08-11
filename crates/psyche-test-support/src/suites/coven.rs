//! Reusable Coven-boundary assertions and deterministic G2 fixture.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, CancellationUnresolvedEvidence, ExecutionBinding,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::{RecordKind, SchemaVersion};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use psyche_core::id::{RecordId, RequestId};
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, ArtifactReference, Capability, CapabilityProfile,
    ContentAddressedReference, CovenEvent, CovenPort, EventCursor, EventPage, ExecutionCorrelation,
    ExecutionRequestInput, NegotiateRequest, PortError, ReconciliationDisposition,
    ReconciliationRequest, ResultBundle, SessionSnapshot, TerminationDispatchError,
    TerminationDisposition, TerminationPersistence, TerminationPersistenceFailure,
    TerminationRequest, derive_termination_outcome_revision, persist_then_terminate,
};

use super::ConformanceOutcome;
use crate::coven::{
    CovenConformanceCase, CovenConformanceFixture, CovenConformanceObservations, CovenFaultPoint,
    DurableDispositionKind, DurableDispositionObservation, FixtureAvailability,
    FixtureControlError, RedispatchEligibility,
};

const CONTRACT: &str = "coven.daemon.v1";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_CONTENT_SIZE_BYTES: u64 = i64::MAX as u64;
const LAUNCH_GOLDEN: &[u8] =
    include_bytes!("../../../psyche-coven/tests/fixtures/execution-request-launch.json");
const INPUT_GOLDEN: &[u8] =
    include_bytes!("../../../psyche-coven/tests/fixtures/execution-request-input.json");
const RESULT_GOLDEN: &[u8] =
    include_bytes!("../../../psyche-coven/tests/fixtures/result-bundle.json");
const RAW_LEDGER_STATES: [&str; 7] = [
    "created",
    "running",
    "idle",
    "completed",
    "failed",
    "killed",
    "orphaned",
];

#[derive(Debug, Clone)]
struct DurableAdoption {
    request_digest: Sha256Digest,
    canonical_input: Vec<u8>,
    correlation: ExecutionCorrelation,
    disposition: AdoptionDisposition,
}

#[derive(Debug, Clone, Default)]
struct SessionState {
    correlations: Vec<ExecutionCorrelation>,
    authoritative_terminal: bool,
}

#[derive(Debug, Clone)]
struct DurableReconciliation {
    request: ReconciliationRequest,
    disposition: ReconciliationDisposition,
}

#[derive(Debug, Clone)]
struct DurableTermination {
    binding: ExecutionBinding,
    disposition: TerminationDisposition,
}

#[derive(Debug, Clone)]
struct DurableTerminationAcknowledgement {
    binding: ExecutionBinding,
    disposition: TerminationDisposition,
}

#[derive(Debug, Default)]
struct ScriptedDurableState {
    adoptions: BTreeMap<String, DurableAdoption>,
    lookup_dispositions: BTreeMap<String, AdoptionDisposition>,
    sessions: BTreeMap<String, SessionState>,
    reconciliations: BTreeMap<String, DurableReconciliation>,
    latest_reconciliation: Option<String>,
    event_pages: BTreeMap<(String, u64), EventPage>,
    event_high_water: BTreeMap<String, u64>,
    primary_results: BTreeMap<String, ContentAddressedReference>,
    results: BTreeMap<String, ResultBundle>,
    termination_acknowledgements: BTreeMap<String, DurableTerminationAcknowledgement>,
    terminations: BTreeMap<String, DurableTermination>,
}

#[derive(Debug, Default)]
struct ScriptedRuntimeState {
    selected_fault: Option<CovenFaultPoint>,
    inspect_indices: BTreeMap<String, usize>,
    adoption_calls: u64,
    reconciliation_calls: u64,
}

#[derive(Debug, Clone)]
struct ScriptedG2Port {
    durable: Arc<Mutex<ScriptedDurableState>>,
    runtime: Arc<Mutex<ScriptedRuntimeState>>,
    initial_session_id: String,
}

/// Deterministic, restartable fixture used for the scripted G2 evidence rows.
#[derive(Debug, Clone)]
pub struct ScriptedG2Fixture {
    port: ScriptedG2Port,
}

/// Builds a clean scripted fixture supporting all twelve G2 cases.
pub fn scripted_fixture() -> ScriptedG2Fixture {
    scripted_fixture_with_session_id("session-1")
}

/// Builds a clean scripted fixture with a caller-selected opaque session ID.
pub fn scripted_fixture_with_session_id(session_id: impl Into<String>) -> ScriptedG2Fixture {
    ScriptedG2Fixture {
        port: ScriptedG2Port {
            durable: Arc::new(Mutex::new(ScriptedDurableState::default())),
            runtime: Arc::new(Mutex::new(ScriptedRuntimeState::default())),
            initial_session_id: session_id.into(),
        },
    }
}

impl ScriptedG2Port {
    fn durable(&self) -> Result<MutexGuard<'_, ScriptedDurableState>, PortError> {
        self.durable.lock().map_err(|_| PortError::Unavailable)
    }

    fn runtime(&self) -> Result<MutexGuard<'_, ScriptedRuntimeState>, PortError> {
        self.runtime.lock().map_err(|_| PortError::Unavailable)
    }

    fn session_for_launch(&self, state: &ScriptedDurableState) -> String {
        if state.sessions.is_empty() {
            self.initial_session_id.clone()
        } else {
            format!(
                "{}-{}",
                self.initial_session_id,
                state.sessions.len().saturating_add(1)
            )
        }
    }

    fn adoption_fault(
        selected_fault: Option<CovenFaultPoint>,
        input: &ExecutionRequestInput,
    ) -> Option<CovenFaultPoint> {
        match (input, selected_fault) {
            (
                ExecutionRequestInput::Launch { .. },
                Some(
                    point @ (CovenFaultPoint::AdoptionBeforeCommit
                    | CovenFaultPoint::AdoptionAfterCommit),
                ),
            )
            | (
                ExecutionRequestInput::Input { .. },
                Some(
                    point
                    @ (CovenFaultPoint::InputBeforeCommit | CovenFaultPoint::InputAfterCommit),
                ),
            ) => Some(point),
            _ => None,
        }
    }

    fn disposition_for_missing_lookup(request_id: &RequestId) -> AdoptionDisposition {
        if request_id.as_str().ends_with("01") {
            AdoptionDisposition::ProvenNotAdopted
        } else {
            AdoptionDisposition::Unknown
        }
    }

    fn same_correlation_except_request_id(
        stored: &ExecutionCorrelation,
        candidate: &ExecutionCorrelation,
    ) -> bool {
        stored.request_id != candidate.request_id
            && stored.request_digest == candidate.request_digest
            && stored.familiar_snapshot_id == candidate.familiar_snapshot_id
            && stored.project_id == candidate.project_id
            && stored.graph_id == candidate.graph_id
            && stored.node_id == candidate.node_id
            && stored.attempt_id == candidate.attempt_id
            && stored.created_at == candidate.created_at
            && stored.valid_until == candidate.valid_until
    }

    fn result_for(
        session_id: &str,
        correlation: &ExecutionCorrelation,
    ) -> Result<ResultBundle, PortError> {
        if session_id == "session-1" {
            let golden: ResultBundle =
                serde_json::from_slice(RESULT_GOLDEN).map_err(|_| PortError::InvalidResponse)?;
            if &golden.correlation == correlation {
                return Ok(golden);
            }
        }
        let result = ContentAddressedReference {
            digest: digest_of('b'),
            media_type: "application/json".to_owned(),
            size_bytes: 2,
            expires_at: correlation.created_at + time::Duration::minutes(4),
        };
        let artifact = ArtifactReference {
            artifact_id: "artifact-1".to_owned(),
            session_id: session_id.to_owned(),
            correlation: correlation.clone(),
            content: ContentAddressedReference {
                digest: digest_of('c'),
                media_type: "text/plain".to_owned(),
                size_bytes: 5,
                expires_at: correlation.created_at + time::Duration::minutes(3),
            },
        };
        let bundle = ResultBundle {
            session_id: session_id.to_owned(),
            correlation: correlation.clone(),
            result,
            artifacts: vec![artifact],
        };
        bundle.validate().map_err(|_| PortError::InvalidResponse)?;
        Ok(bundle)
    }

    fn termination_disposition(
        binding: &ExecutionBinding,
    ) -> Result<TerminationDisposition, PortError> {
        let termination = binding
            .termination_request
            .as_ref()
            .ok_or(PortError::InvalidRequest)?;
        let session_id = binding
            .coven_session_id
            .as_ref()
            .ok_or(PortError::InvalidRequest)?;
        if binding.termination_reason_code.as_deref() == Some("force_unresolved") {
            return Ok(TerminationDisposition::Unresolved {
                evidence: CancellationUnresolvedEvidence {
                    disposition_id: "unresolved-1".to_owned(),
                    termination_request_id: termination.termination_request_id.clone(),
                    session_id: session_id.clone(),
                    execution_request_id: binding.request_id.clone(),
                    execution_request_digest: binding.request_digest.clone(),
                    reason_code: "authority_silent".to_owned(),
                    recorded_at: termination.created_at,
                },
            });
        }
        Ok(TerminationDisposition::Acknowledged {
            evidence: CancellationAcknowledgementEvidence {
                acknowledgement_id: "acknowledgement-1".to_owned(),
                termination_request_id: termination.termination_request_id.clone(),
                session_id: session_id.clone(),
                execution_request_id: binding.request_id.clone(),
                execution_request_digest: binding.request_digest.clone(),
                kind: CancellationAcknowledgementKind::Terminated,
                authority_evidence_digest: digest_of('e'),
                acknowledged_at: termination.created_at,
            },
        })
    }
}

#[async_trait::async_trait]
impl CovenPort for ScriptedG2Port {
    async fn negotiate(&self, request: NegotiateRequest) -> Result<CapabilityProfile, PortError> {
        request.validate()?;
        if request.required_api_version != CONTRACT {
            return Err(PortError::ContractUnsupported {});
        }
        let capabilities = capability_names();
        if !request.required_capabilities.is_subset(&capabilities) {
            return Err(PortError::CapabilityMissing {});
        }
        Ok(CapabilityProfile {
            api_version: CONTRACT.to_owned(),
            capabilities,
        })
    }

    async fn adopt(&self, request: AdoptionRequest) -> Result<AdoptionDisposition, PortError> {
        request.validate_digest()?;
        let correlation = request.correlation();
        if correlation.valid_until < at("2026-08-05T14:00:00Z") {
            return Err(PortError::InvalidRequest);
        }
        let canonical_input =
            canonical_bytes(request.input()).map_err(|_| PortError::InvalidRequest)?;
        let key = correlation.request_id.as_str().to_owned();
        {
            let state = self.durable()?;
            if let Some(stored) = state.adoptions.get(&key) {
                return if stored.request_digest == *request.request_digest()
                    && stored.canonical_input == canonical_input
                {
                    Ok(stored.disposition.clone())
                } else {
                    Err(PortError::IntentConflict)
                };
            }
        }
        let selected_fault = {
            let mut runtime = self.runtime()?;
            runtime.adoption_calls = runtime.adoption_calls.saturating_add(1);
            runtime.selected_fault
        };
        if selected_fault == Some(CovenFaultPoint::AdoptionPolicyDenied) {
            return Err(PortError::PolicyDenied);
        }
        let mut state = self.durable()?;
        // Recheck after acquiring the runtime observation in case another caller committed first.
        if let Some(stored) = state.adoptions.get(&key) {
            return if stored.request_digest == *request.request_digest()
                && stored.canonical_input == canonical_input
            {
                Ok(stored.disposition.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }

        let fault = Self::adoption_fault(selected_fault, request.input());
        if matches!(
            fault,
            Some(CovenFaultPoint::AdoptionBeforeCommit | CovenFaultPoint::InputBeforeCommit)
        ) {
            return Err(PortError::Unavailable);
        }

        let disposition = match request.input() {
            ExecutionRequestInput::Launch { .. } => AdoptionDisposition::Adopted {
                session_id: self.session_for_launch(&state),
            },
            ExecutionRequestInput::Input { session_id, .. } => {
                if !state.sessions.contains_key(session_id) {
                    return Err(PortError::NotFound);
                }
                AdoptionDisposition::Adopted {
                    session_id: session_id.clone(),
                }
            }
        };
        let AdoptionDisposition::Adopted { session_id } = &disposition else {
            return Err(PortError::InvalidResponse);
        };
        let session = state.sessions.entry(session_id.clone()).or_default();
        if !session.correlations.contains(&correlation) {
            session.correlations.push(correlation.clone());
        }
        state.adoptions.insert(
            key,
            DurableAdoption {
                request_digest: request.request_digest().clone(),
                canonical_input,
                correlation,
                disposition: disposition.clone(),
            },
        );
        if matches!(
            fault,
            Some(CovenFaultPoint::AdoptionAfterCommit | CovenFaultPoint::InputAfterCommit)
        ) {
            Err(PortError::Unavailable)
        } else {
            Ok(disposition)
        }
    }

    async fn lookup(&self, request_id: &RequestId) -> Result<AdoptionDisposition, PortError> {
        let selected_fault = self.runtime()?.selected_fault;
        let mut state = self.durable()?;
        if selected_fault == Some(CovenFaultPoint::LookupBeforeRead) {
            return Err(PortError::Unavailable);
        }
        let disposition = state
            .adoptions
            .get(request_id.as_str())
            .map(|stored| stored.disposition.clone())
            .or_else(|| state.lookup_dispositions.get(request_id.as_str()).cloned())
            .unwrap_or_else(|| Self::disposition_for_missing_lookup(request_id));
        state
            .lookup_dispositions
            .entry(request_id.as_str().to_owned())
            .or_insert_with(|| disposition.clone());
        if selected_fault == Some(CovenFaultPoint::LookupAfterRead) {
            Err(PortError::Unavailable)
        } else {
            Ok(disposition)
        }
    }

    async fn reconcile(
        &self,
        request: ReconciliationRequest,
    ) -> Result<ReconciliationDisposition, PortError> {
        request.validate()?;
        let selected_fault = {
            let mut runtime = self.runtime()?;
            runtime.reconciliation_calls = runtime.reconciliation_calls.saturating_add(1);
            runtime.selected_fault
        };
        let mut state = self.durable()?;
        match selected_fault {
            Some(CovenFaultPoint::ReconcileBeforeDisposition) => {
                return Err(PortError::Unavailable);
            }
            Some(CovenFaultPoint::ReconcileStall) => return Err(PortError::Stalled),
            _ => {}
        }
        let key = request.correlation.request_id.as_str().to_owned();
        if let Some(stored) = state.reconciliations.get(&key) {
            return if stored.request == request {
                Ok(stored.disposition.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        if state.adoptions.values().any(|stored| {
            Self::same_correlation_except_request_id(&stored.correlation, &request.correlation)
        }) {
            return Err(PortError::IntentConflict);
        }
        let adoption = state.adoptions.get(&key).ok_or(PortError::Unavailable)?;
        if adoption.correlation != request.correlation {
            return Err(PortError::IntentConflict);
        }
        let disposition = match request.reason_code.as_str() {
            "return_original" => {
                let AdoptionDisposition::Adopted { session_id } = &adoption.disposition else {
                    return Err(PortError::InvalidResponse);
                };
                ReconciliationDisposition::Returned {
                    disposition_id: format!("return-{}", request.correlation.request_id.as_str()),
                    session_id: session_id.clone(),
                    correlation: request.correlation.clone(),
                    ambiguity_digest: request.ambiguity_digest.clone(),
                    recorded_at: request.correlation.created_at + time::Duration::minutes(1),
                }
            }
            "fence_ambiguous" => ReconciliationDisposition::Fenced {
                disposition_id: format!("fence-{}", request.correlation.request_id.as_str()),
                fence_token: "fence-token-1".to_owned(),
                correlation: request.correlation.clone(),
                ambiguity_digest: request.ambiguity_digest.clone(),
                recorded_at: request.correlation.created_at + time::Duration::minutes(1),
            },
            _ => ReconciliationDisposition::Unresolved,
        };
        disposition.validate_for(&request)?;
        if disposition == ReconciliationDisposition::Unresolved {
            return Ok(disposition);
        }
        state.reconciliations.insert(
            key.clone(),
            DurableReconciliation {
                request,
                disposition: disposition.clone(),
            },
        );
        state.latest_reconciliation = Some(key);
        if selected_fault == Some(CovenFaultPoint::ReconcileAfterDisposition) {
            Err(PortError::Unavailable)
        } else {
            Ok(disposition)
        }
    }

    async fn inspect(&self, session_id: &str) -> Result<SessionSnapshot, PortError> {
        if session_id.is_empty() || session_id.len() > 255 {
            return Err(PortError::InvalidRequest);
        }
        let (correlation, authoritative_terminal) = {
            let state = self.durable()?;
            let session = state.sessions.get(session_id).ok_or(PortError::NotFound)?;
            (
                session
                    .correlations
                    .first()
                    .cloned()
                    .ok_or(PortError::InvalidResponse)?,
                session.authoritative_terminal,
            )
        };
        if self.runtime()?.selected_fault == Some(CovenFaultPoint::InspectAuthorityLost) {
            return Err(PortError::Unavailable);
        }
        let terminal_state = if authoritative_terminal {
            Some("authoritatively_terminated".to_owned())
        } else {
            let mut runtime = self.runtime()?;
            let inspect_index = runtime
                .inspect_indices
                .entry(session_id.to_owned())
                .or_default();
            let status = RAW_LEDGER_STATES[*inspect_index % RAW_LEDGER_STATES.len()];
            *inspect_index = inspect_index.saturating_add(1);
            Some(status.to_owned())
        };
        Ok(SessionSnapshot {
            session_id: session_id.to_owned(),
            correlation,
            terminal_state,
        })
    }

    async fn events(&self, cursor: EventCursor) -> Result<EventPage, PortError> {
        cursor.validate()?;
        let selected_fault = self.runtime()?.selected_fault;
        let mut state = self.durable()?;
        if !state.sessions.contains_key(&cursor.session_id) {
            return Err(PortError::CorrelationMismatch);
        }
        if selected_fault == Some(CovenFaultPoint::CursorBeforePage) {
            return Err(PortError::Unavailable);
        }
        let key = (cursor.session_id.clone(), cursor.after_sequence);
        if let Some(page) = state.event_pages.get(&key).cloned() {
            return if selected_fault == Some(CovenFaultPoint::CursorAfterPage) {
                Err(PortError::Unavailable)
            } else {
                Ok(page)
            };
        }
        if state
            .event_high_water
            .get(&cursor.session_id)
            .is_some_and(|high| cursor.after_sequence < *high)
        {
            return Err(PortError::IntentConflict);
        }
        if cursor.after_sequence > RAW_LEDGER_STATES.len() as u64 {
            return Err(PortError::InvalidRequest);
        }
        let start =
            usize::try_from(cursor.after_sequence).map_err(|_| PortError::InvalidRequest)?;
        let end = start.saturating_add(3).min(RAW_LEDGER_STATES.len());
        let mut events = Vec::with_capacity(end.saturating_sub(start));
        for (index, terminal_state) in RAW_LEDGER_STATES[start..end].iter().enumerate() {
            let sequence = u64::try_from(start.saturating_add(index).saturating_add(1))
                .map_err(|_| PortError::InvalidResponse)?;
            events.push(CovenEvent {
                sequence,
                event_digest: digest_for_sequence(sequence),
                terminal_state: Some((*terminal_state).to_owned()),
            });
        }
        let next = events
            .last()
            .map_or(cursor.after_sequence, |event| event.sequence);
        let page = EventPage {
            events,
            next_cursor: EventCursor {
                session_id: cursor.session_id.clone(),
                after_sequence: next,
            },
        };
        page.validate_for(&cursor)?;
        state.event_pages.insert(key, page.clone());
        state
            .event_high_water
            .insert(cursor.session_id.clone(), next);
        if selected_fault == Some(CovenFaultPoint::CursorAfterPage) {
            Err(PortError::Unavailable)
        } else {
            Ok(page)
        }
    }

    async fn result(&self, session_id: &str) -> Result<ResultBundle, PortError> {
        if session_id.is_empty() || session_id.len() > 255 {
            return Err(PortError::InvalidRequest);
        }
        let selected_fault = self.runtime()?.selected_fault;
        let mut state = self.durable()?;
        if let Some(bundle) = state.results.get(session_id) {
            return Ok(bundle.clone());
        }
        let correlation = state
            .sessions
            .get(session_id)
            .and_then(|session| session.correlations.first())
            .cloned()
            .ok_or(PortError::NotFound)?;
        let bundle = Self::result_for(session_id, &correlation)?;
        if !state.primary_results.contains_key(session_id) {
            if selected_fault == Some(CovenFaultPoint::ResultBeforePersistence) {
                return Err(PortError::Unavailable);
            }
            state
                .primary_results
                .insert(session_id.to_owned(), bundle.result.clone());
        }
        if state
            .primary_results
            .get(session_id)
            .is_some_and(|result| result != &bundle.result)
        {
            return Err(PortError::IntentConflict);
        }
        if selected_fault == Some(CovenFaultPoint::ArtifactBeforePersistence) {
            return Err(PortError::Unavailable);
        }
        state.results.insert(session_id.to_owned(), bundle.clone());
        Ok(bundle)
    }

    async fn terminate(
        &self,
        request: TerminationRequest,
    ) -> Result<TerminationDisposition, PortError> {
        let binding = request.binding().clone();
        let termination = binding
            .termination_request
            .as_ref()
            .ok_or(PortError::InvalidRequest)?;
        let key = termination.termination_request_id.as_str().to_owned();
        let selected_fault = self.runtime()?.selected_fault;
        let mut state = self.durable()?;
        if let Some(stored) = state.terminations.get(&key) {
            return if stored.binding == binding {
                Ok(stored.disposition.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        let session_id = binding
            .coven_session_id
            .clone()
            .ok_or(PortError::InvalidRequest)?;
        let session = state.sessions.get(&session_id).ok_or(PortError::NotFound)?;
        if !session.correlations.iter().any(|correlation| {
            correlation.request_id == binding.request_id
                && correlation.request_digest == binding.request_digest
        }) {
            return Err(PortError::CorrelationMismatch);
        }
        let disposition = if let Some(stored) = state.termination_acknowledgements.get(&key) {
            if stored.binding != binding {
                return Err(PortError::IntentConflict);
            }
            stored.disposition.clone()
        } else {
            if selected_fault == Some(CovenFaultPoint::CancellationBeforeAcknowledgement) {
                return Err(PortError::Unavailable);
            }
            let disposition = Self::termination_disposition(&binding)?;
            state.termination_acknowledgements.insert(
                key.clone(),
                DurableTerminationAcknowledgement {
                    binding: binding.clone(),
                    disposition: disposition.clone(),
                },
            );
            if selected_fault == Some(CovenFaultPoint::CancellationAfterAcknowledgement) {
                return Err(PortError::Unavailable);
            }
            disposition
        };
        if selected_fault == Some(CovenFaultPoint::TerminalBeforePersistence) {
            return Err(PortError::Unavailable);
        }
        state.terminations.insert(
            key,
            DurableTermination {
                binding,
                disposition: disposition.clone(),
            },
        );
        if matches!(disposition, TerminationDisposition::Acknowledged { .. }) {
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.authoritative_terminal = true;
            }
        }
        Ok(disposition)
    }
}

#[async_trait::async_trait]
impl CovenConformanceFixture for ScriptedG2Fixture {
    fn port(&self) -> &dyn CovenPort {
        &self.port
    }

    fn availability(&self, _case: CovenConformanceCase) -> FixtureAvailability {
        FixtureAvailability::Supported
    }

    async fn restart(&mut self) {
        self.port = ScriptedG2Port {
            durable: Arc::clone(&self.port.durable),
            runtime: Arc::new(Mutex::new(ScriptedRuntimeState::default())),
            initial_session_id: self.port.initial_session_id.clone(),
        };
    }

    async fn select_fault(&mut self, point: CovenFaultPoint) -> Result<(), FixtureControlError> {
        let mut runtime = self
            .port
            .runtime
            .lock()
            .map_err(|_| FixtureControlError::Unavailable)?;
        runtime.selected_fault = Some(point);
        Ok(())
    }

    async fn clear_fault(&mut self) -> Result<(), FixtureControlError> {
        let mut runtime = self
            .port
            .runtime
            .lock()
            .map_err(|_| FixtureControlError::Unavailable)?;
        runtime.selected_fault = None;
        Ok(())
    }

    async fn reset(&mut self) {
        if let Ok(mut state) = self.port.durable.lock() {
            *state = ScriptedDurableState::default();
        }
        if let Ok(mut runtime) = self.port.runtime.lock() {
            *runtime = ScriptedRuntimeState::default();
        }
    }

    async fn observations(&self) -> CovenConformanceObservations {
        let Ok(state) = self.port.durable.lock() else {
            return CovenConformanceObservations::default();
        };
        let Ok(runtime) = self.port.runtime.lock() else {
            return CovenConformanceObservations::default();
        };
        let durable_reconciliation = state
            .latest_reconciliation
            .as_ref()
            .and_then(|key| state.reconciliations.get(key))
            .and_then(|stored| disposition_observation(&stored.disposition));
        CovenConformanceObservations {
            adoption_calls: runtime.adoption_calls,
            reconciliation_calls: runtime.reconciliation_calls,
            durable_reconciliation,
        }
    }

    async fn redispatch_eligibility(
        &self,
        correlation: &ExecutionCorrelation,
    ) -> Result<RedispatchEligibility, FixtureControlError> {
        let state = self
            .port
            .durable
            .lock()
            .map_err(|_| FixtureControlError::Unavailable)?;
        Ok(
            match state
                .reconciliations
                .get(correlation.request_id.as_str())
                .filter(|stored| stored.request.correlation == *correlation)
                .map(|stored| &stored.disposition)
            {
                Some(ReconciliationDisposition::Fenced { .. }) => {
                    RedispatchEligibility::EligibleAfterFence
                }
                Some(
                    ReconciliationDisposition::Returned { .. }
                    | ReconciliationDisposition::Unresolved,
                )
                | None => RedispatchEligibility::Blocked,
            },
        )
    }
}

fn disposition_observation(
    disposition: &ReconciliationDisposition,
) -> Option<DurableDispositionObservation> {
    match disposition {
        ReconciliationDisposition::Returned {
            disposition_id,
            session_id,
            correlation,
            ambiguity_digest,
            recorded_at,
        } => Some(DurableDispositionObservation {
            disposition_id: disposition_id.clone(),
            correlation: correlation.clone(),
            ambiguity_digest: ambiguity_digest.clone(),
            kind: DurableDispositionKind::Returned {
                session_id: session_id.clone(),
            },
            recorded_at: *recorded_at,
        }),
        ReconciliationDisposition::Fenced {
            disposition_id,
            fence_token,
            correlation,
            ambiguity_digest,
            recorded_at,
        } => Some(DurableDispositionObservation {
            disposition_id: disposition_id.clone(),
            correlation: correlation.clone(),
            ambiguity_digest: ambiguity_digest.clone(),
            kind: DurableDispositionKind::Fenced {
                fence_token: fence_token.clone(),
            },
            recorded_at: *recorded_at,
        }),
        ReconciliationDisposition::Unresolved => None,
    }
}

#[derive(Clone)]
struct UnsupportedPort {
    error: PortError,
}

impl fmt::Debug for UnsupportedPort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UnsupportedPort")
    }
}

/// Fixture used to execute and verify the structured unsupported path.
#[derive(Debug, Clone)]
pub struct UnsupportedCovenFixture {
    code: String,
    port: UnsupportedPort,
}

/// Builds a fixture that denies every public operation with one exact code.
pub fn unsupported_fixture(code: &str) -> UnsupportedCovenFixture {
    let error = denial_for_code(code);
    UnsupportedCovenFixture {
        code: code.to_owned(),
        port: UnsupportedPort { error },
    }
}

#[async_trait::async_trait]
impl CovenPort for UnsupportedPort {
    async fn negotiate(&self, _request: NegotiateRequest) -> Result<CapabilityProfile, PortError> {
        Err(self.error)
    }

    async fn adopt(&self, _request: AdoptionRequest) -> Result<AdoptionDisposition, PortError> {
        Err(self.error)
    }

    async fn lookup(&self, _request_id: &RequestId) -> Result<AdoptionDisposition, PortError> {
        Err(self.error)
    }

    async fn reconcile(
        &self,
        _request: ReconciliationRequest,
    ) -> Result<ReconciliationDisposition, PortError> {
        Err(self.error)
    }

    async fn inspect(&self, _session_id: &str) -> Result<SessionSnapshot, PortError> {
        Err(self.error)
    }

    async fn events(&self, _cursor: EventCursor) -> Result<EventPage, PortError> {
        Err(self.error)
    }

    async fn result(&self, _session_id: &str) -> Result<ResultBundle, PortError> {
        Err(self.error)
    }

    async fn terminate(
        &self,
        _request: TerminationRequest,
    ) -> Result<TerminationDisposition, PortError> {
        Err(self.error)
    }
}

#[async_trait::async_trait]
impl CovenConformanceFixture for UnsupportedCovenFixture {
    fn port(&self) -> &dyn CovenPort {
        &self.port
    }

    fn availability(&self, _case: CovenConformanceCase) -> FixtureAvailability {
        FixtureAvailability::ExpectedUnsupported {
            code: self.code.clone(),
        }
    }

    async fn restart(&mut self) {}

    async fn select_fault(&mut self, _point: CovenFaultPoint) -> Result<(), FixtureControlError> {
        Err(FixtureControlError::UnsupportedFault)
    }

    async fn clear_fault(&mut self) -> Result<(), FixtureControlError> {
        Ok(())
    }

    async fn reset(&mut self) {}

    async fn observations(&self) -> CovenConformanceObservations {
        CovenConformanceObservations::default()
    }

    async fn redispatch_eligibility(
        &self,
        _correlation: &ExecutionCorrelation,
    ) -> Result<RedispatchEligibility, FixtureControlError> {
        Ok(RedispatchEligibility::Blocked)
    }
}

#[derive(Debug, Clone, Copy)]
enum UnsupportedCall {
    Negotiate,
    Adopt,
    Lookup,
    Reconcile,
    Inspect,
    Events,
    Result,
    Terminate,
}

async fn require_fault(fixture: &mut dyn CovenConformanceFixture, point: CovenFaultPoint) {
    fixture
        .select_fault(point)
        .await
        .unwrap_or_else(|error| panic!("{point:?} must be controllable: {error}"));
}

async fn require_clear_fault(fixture: &mut dyn CovenConformanceFixture) {
    fixture
        .clear_fault()
        .await
        .unwrap_or_else(|error| panic!("fixture fault must clear: {error}"));
}

async fn expected_unsupported(
    fixture: &mut dyn CovenConformanceFixture,
    case: CovenConformanceCase,
    call: UnsupportedCall,
) -> Option<ConformanceOutcome> {
    let FixtureAvailability::ExpectedUnsupported { code } = fixture.availability(case) else {
        return None;
    };
    fixture.reset().await;
    let before = fixture.observations().await;
    let expected = denial_for_code(&code);
    match call {
        UnsupportedCall::Negotiate => {
            assert_eq!(
                fixture.port().negotiate(exact_negotiation_request()).await,
                Err(expected)
            );
        }
        UnsupportedCall::Adopt => {
            assert_eq!(fixture.port().adopt(launch_request()).await, Err(expected));
        }
        UnsupportedCall::Lookup => {
            assert_eq!(fixture.port().lookup(&request_id(1)).await, Err(expected));
        }
        UnsupportedCall::Reconcile => {
            let request = ReconciliationRequest {
                correlation: launch_request().correlation(),
                ambiguity_digest: digest_of('a'),
                reason_code: "return_original".to_owned(),
            };
            assert_eq!(fixture.port().reconcile(request).await, Err(expected));
        }
        UnsupportedCall::Inspect => {
            assert_eq!(fixture.port().inspect("session-1").await, Err(expected));
        }
        UnsupportedCall::Events => {
            assert_eq!(
                fixture
                    .port()
                    .events(EventCursor {
                        session_id: "session-1".to_owned(),
                        after_sequence: 0,
                    })
                    .await,
                Err(expected)
            );
        }
        UnsupportedCall::Result => {
            assert_eq!(fixture.port().result("session-1").await, Err(expected));
        }
        UnsupportedCall::Terminate => {
            let requested =
                termination_requested_binding(&launch_request(), "session-1", "operator_request");
            let mut persistence = MemoryTerminationPersistence::default();
            assert!(matches!(
                persist_then_terminate(&mut persistence, fixture.port(), requested).await,
                Err(TerminationDispatchError::Port(error)) if error == expected
            ));
        }
    }
    assert_eq!(fixture.observations().await, before);
    Some(ConformanceOutcome::ExpectedUnsupported { code })
}

fn denial_for_code(code: &str) -> PortError {
    match code {
        "ContractUnsupported" => PortError::ContractUnsupported {},
        "CapabilityMissing" => PortError::CapabilityMissing {},
        _ => panic!("unsupported fixture declared an unstable denial code"),
    }
}

fn capability_names() -> BTreeSet<String> {
    [
        Capability::StableAdoption,
        Capability::AmbiguityFence,
        Capability::OrderedEvents,
        Capability::AuthoritativeTermination,
        Capability::ContentAddressedResults,
    ]
    .into_iter()
    .map(|capability| capability.as_str().to_owned())
    .collect()
}

fn exact_negotiation_request() -> NegotiateRequest {
    [
        Capability::StableAdoption,
        Capability::AmbiguityFence,
        Capability::OrderedEvents,
        Capability::AuthoritativeTermination,
        Capability::ContentAddressedResults,
    ]
    .into_iter()
    .fold(NegotiateRequest::new(CONTRACT), |request, capability| {
        request.requiring(capability)
    })
}

fn launch_input() -> ExecutionRequestInput {
    match serde_json::from_slice(LAUNCH_GOLDEN) {
        Ok(input) => input,
        Err(error) => panic!("canonical launch fixture must decode: {error}"),
    }
}

fn launch_request() -> AdoptionRequest {
    match AdoptionRequest::new(launch_input()) {
        Ok(request) => request,
        Err(error) => panic!("canonical launch fixture must validate: {error}"),
    }
}

fn request_with_id(request: &AdoptionRequest, request_id: &str) -> AdoptionRequest {
    let mut input = match serde_json::to_value(request.input()) {
        Ok(input) => input,
        Err(error) => panic!("canonical adoption input must encode: {error}"),
    };
    input["request_id"] = serde_json::json!(request_id);
    let input = match serde_json::from_value(input) {
        Ok(input) => input,
        Err(error) => panic!("changed request identity must remain typed: {error}"),
    };
    match AdoptionRequest::new(input) {
        Ok(request) => request,
        Err(error) => panic!("changed request identity must remain valid: {error}"),
    }
}

fn same_id_conflicting_request(request: &AdoptionRequest) -> AdoptionRequest {
    let mut input = match serde_json::to_value(request.input()) {
        Ok(input) => input,
        Err(error) => panic!("canonical adoption input must encode: {error}"),
    };
    input["principal_id"] = serde_json::json!("principal:conflicting-intent");
    let input = match serde_json::from_value(input) {
        Ok(input) => input,
        Err(error) => panic!("conflicting adoption input must remain typed: {error}"),
    };
    match AdoptionRequest::new(input) {
        Ok(request) => request,
        Err(error) => panic!("conflicting adoption input must remain valid: {error}"),
    }
}

async fn adopt_session(
    fixture: &dyn CovenConformanceFixture,
    request: AdoptionRequest,
    context: &str,
) -> String {
    match fixture.port().adopt(request).await {
        Ok(AdoptionDisposition::Adopted { session_id }) => session_id,
        other => panic!("{context}: {other:?}"),
    }
}

fn session_input_request(session_id: &str) -> AdoptionRequest {
    let mut value: serde_json::Value = match serde_json::from_slice(INPUT_GOLDEN) {
        Ok(value) => value,
        Err(error) => panic!("canonical input fixture must decode: {error}"),
    };
    value["request_id"] = serde_json::json!("req_01J00000000000000000000003");
    value["session_id"] = serde_json::json!(session_id);
    let input: ExecutionRequestInput = match serde_json::from_value(value) {
        Ok(value) => value,
        Err(error) => panic!("session input fixture must remain typed: {error}"),
    };
    match AdoptionRequest::new(input) {
        Ok(request) => request,
        Err(error) => panic!("session input fixture must validate: {error}"),
    }
}

fn stale_digest_mutations(request: &AdoptionRequest) -> Vec<(&'static str, AdoptionRequest)> {
    let launch = matches!(request.input(), ExecutionRequestInput::Launch { .. });
    let mut mutations: Vec<(&str, serde_json::Value)> = if launch {
        vec![
            (
                "/input/schema_version",
                serde_json::json!("psyche.execution_request.v2"),
            ),
            (
                "/input/request_id",
                serde_json::json!("req_01J00000000000000000000011"),
            ),
            (
                "/input/graph_id",
                serde_json::json!("grf_01J00000000000000000000011"),
            ),
            (
                "/input/node_id",
                serde_json::json!("nod_01J00000000000000000000011"),
            ),
            (
                "/input/attempt_id",
                serde_json::json!("att_01J00000000000000000000011"),
            ),
            (
                "/input/principal_id",
                serde_json::json!("principal:changed"),
            ),
            (
                "/input/familiar_snapshot_id",
                serde_json::json!("ids_01J00000000000000000000011"),
            ),
            (
                "/input/project_id",
                serde_json::json!("project:sha256:changed"),
            ),
            ("/input/project_root", serde_json::json!("/workspace/other")),
            (
                "/input/cwd",
                serde_json::json!("/workspace/project/subdirectory"),
            ),
            ("/input/harness", serde_json::json!("future_harness")),
            (
                "/input/context_manifest_digest",
                serde_json::json!(digest_of('7').as_str()),
            ),
            (
                "/input/delegation_digest",
                serde_json::json!(digest_of('8').as_str()),
            ),
            (
                "/input/budget_digest",
                serde_json::json!(digest_of('9').as_str()),
            ),
            (
                "/input/required_artifact_bindings/0/artifact_id",
                serde_json::json!("artifact-changed"),
            ),
            (
                "/input/required_artifact_bindings/0/digest",
                serde_json::json!(digest_of('a').as_str()),
            ),
            (
                "/input/required_artifact_bindings/0/media_type",
                serde_json::json!("application/json"),
            ),
            (
                "/input/required_artifact_bindings/0/size",
                serde_json::json!(13),
            ),
            (
                "/input/required_artifact_bindings",
                serde_json::json!([
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
            (
                "/input/payload_digest",
                serde_json::json!(digest_of('b').as_str()),
            ),
            (
                "/input/created_at",
                serde_json::json!("2026-08-05T14:00:01Z"),
            ),
            (
                "/input/valid_until",
                serde_json::json!("2026-08-05T14:04:59Z"),
            ),
        ]
    } else {
        vec![
            (
                "/input/schema_version",
                serde_json::json!("psyche.execution_request.v2"),
            ),
            (
                "/input/request_id",
                serde_json::json!("req_01J00000000000000000000011"),
            ),
            (
                "/input/graph_id",
                serde_json::json!("grf_01J00000000000000000000011"),
            ),
            (
                "/input/node_id",
                serde_json::json!("nod_01J00000000000000000000011"),
            ),
            (
                "/input/attempt_id",
                serde_json::json!("att_01J00000000000000000000011"),
            ),
            (
                "/input/principal_id",
                serde_json::json!("principal:changed"),
            ),
            (
                "/input/familiar_snapshot_id",
                serde_json::json!("ids_01J00000000000000000000011"),
            ),
            (
                "/input/project_id",
                serde_json::json!("project:sha256:changed"),
            ),
            ("/input/session_id", serde_json::json!("session-changed")),
            (
                "/input/input_digest",
                serde_json::json!(digest_of('7').as_str()),
            ),
            (
                "/input/context_manifest_digest",
                serde_json::json!(digest_of('8').as_str()),
            ),
            (
                "/input/required_artifact_bindings",
                serde_json::json!([{
                    "artifact_id": "artifact-new",
                    "digest": digest_of('9').as_str(),
                    "media_type": "text/plain",
                    "size": 1
                }]),
            ),
            (
                "/input/payload_digest",
                serde_json::json!(digest_of('a').as_str()),
            ),
            (
                "/input/created_at",
                serde_json::json!("2026-08-05T14:01:01Z"),
            ),
            (
                "/input/valid_until",
                serde_json::json!("2026-08-05T14:05:59Z"),
            ),
        ]
    };
    let mut other_input: serde_json::Value = if launch {
        match serde_json::from_slice(INPUT_GOLDEN) {
            Ok(value) => value,
            Err(error) => panic!("canonical input fixture must decode: {error}"),
        }
    } else {
        match serde_json::from_slice(LAUNCH_GOLDEN) {
            Ok(value) => value,
            Err(error) => panic!("canonical launch fixture must decode: {error}"),
        }
    };
    other_input["request_id"] = serde_json::json!(request.correlation().request_id.as_str());
    mutations.push(("/input", other_input));
    mutations
        .into_iter()
        .map(|(pointer, mut replacement)| {
            let mut value = match serde_json::to_value(request) {
                Ok(value) => value,
                Err(error) => panic!("typed adoption request must serialize: {error}"),
            };
            let Some(field) = value.pointer_mut(pointer) else {
                panic!("static request mutation pointer must exist: {pointer}");
            };
            if pointer == "/input/session_id" {
                let Some(session_id) = field.as_str() else {
                    panic!("session input mutation requires a string session id");
                };
                replacement = serde_json::json!(distinct_session_id(session_id, "session-changed"));
            }
            *field = replacement;
            let forged = match serde_json::from_value(value) {
                Ok(value) => value,
                Err(error) => panic!("stale-digest request must remain typed: {error}"),
            };
            (pointer, forged)
        })
        .collect()
}

fn changed_correlations(
    correlation: &ExecutionCorrelation,
) -> Vec<(&'static str, ExecutionCorrelation)> {
    let mut changed = Vec::new();
    let mut candidate = correlation.clone();
    candidate.request_id = request_id(11);
    changed.push(("request_id", candidate));
    let mut candidate = correlation.clone();
    candidate.request_digest = digest_of('a');
    changed.push(("request_digest", candidate));
    let mut candidate = correlation.clone();
    candidate.familiar_snapshot_id = record_id(RecordKind::IdentitySnapshot, 11);
    changed.push(("familiar_snapshot_id", candidate));
    let mut candidate = correlation.clone();
    candidate.project_id = "project:sha256:changed".to_owned();
    changed.push(("project_id", candidate));
    let mut candidate = correlation.clone();
    candidate.graph_id = record_id(RecordKind::Graph, 11);
    changed.push(("graph_id", candidate));
    let mut candidate = correlation.clone();
    candidate.node_id = record_id(RecordKind::GraphNode, 11);
    changed.push(("node_id", candidate));
    let mut candidate = correlation.clone();
    candidate.attempt_id = record_id(RecordKind::Attempt, 11);
    changed.push(("attempt_id", candidate));
    let mut candidate = correlation.clone();
    candidate.created_at += time::Duration::seconds(1);
    changed.push(("created_at", candidate));
    let mut candidate = correlation.clone();
    candidate.valid_until -= time::Duration::seconds(1);
    changed.push(("validity_window", candidate));
    changed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryPersistenceError {
    Conflict,
    Canonicalization,
}

#[derive(Debug, Default)]
struct MemoryTerminationPersistence {
    revisions: BTreeMap<(String, u64), Vec<u8>>,
}

impl TerminationPersistence for MemoryTerminationPersistence {
    type Error = MemoryPersistenceError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.persist(requested)
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.persist(outcome)
    }
}

impl MemoryTerminationPersistence {
    fn persist(
        &mut self,
        binding: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<MemoryPersistenceError>> {
        let bytes = canonical_bytes(&binding).map_err(|_| {
            TerminationPersistenceFailure::Write(MemoryPersistenceError::Canonicalization)
        })?;
        let key = (binding.attempt_id.as_str().to_owned(), binding.revision);
        if let Some(stored) = self.revisions.get(&key) {
            return if stored == &bytes {
                Ok(stored.clone())
            } else {
                Err(TerminationPersistenceFailure::Conflict(
                    MemoryPersistenceError::Conflict,
                ))
            };
        }
        self.revisions.insert(key, bytes.clone());
        Ok(bytes)
    }
}

fn termination_requested_binding(
    adoption: &AdoptionRequest,
    session_id: &str,
    reason: &str,
) -> ExecutionBinding {
    let correlation = adoption.correlation();
    ExecutionBinding {
        schema_version: schema("psyche.execution_binding.v1"),
        attempt_id: correlation.attempt_id,
        revision: 2,
        previous_revision_digest: Some(digest_of('f')),
        revision_created_at: correlation.created_at + time::Duration::minutes(2),
        familiar_snapshot_id: correlation.familiar_snapshot_id,
        project_id: correlation.project_id,
        request_id: correlation.request_id,
        request_digest: correlation.request_digest,
        request_created_at: correlation.created_at,
        request_valid_until: correlation.valid_until,
        coven_contract_version: CONTRACT.to_owned(),
        coven_session_id: Some(session_id.to_owned()),
        adoption_state: AdoptionState::Adopted,
        event_cursor: Some("cursor:0".to_owned()),
        cancellation_state: CancellationState::TerminationRequested,
        termination_request: Some(TerminationRequestCorrelation {
            termination_request_id: request_id(9),
            created_at: at("2026-08-05T14:02:00Z"),
            valid_until: at("2026-08-05T14:04:00Z"),
        }),
        termination_reason_code: Some(reason.to_owned()),
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    }
}

fn at(value: &str) -> time::OffsetDateTime {
    use time::format_description::well_known::Rfc3339;

    match time::OffsetDateTime::parse(value, &Rfc3339) {
        Ok(value) => value,
        Err(error) => panic!("static RFC 3339 timestamp is valid: {error}"),
    }
}

fn digest_of(character: char) -> Sha256Digest {
    let value = format!("sha256:{}", character.to_string().repeat(64));
    match Sha256Digest::parse(&value) {
        Ok(value) => value,
        Err(error) => panic!("static SHA-256 digest is valid: {error}"),
    }
}

fn digest_for_sequence(sequence: u64) -> Sha256Digest {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let index = usize::try_from(sequence % 16).unwrap_or(0);
    digest_of(char::from(HEX[index]))
}

fn assert_cursor_page_progress(cursor: &EventCursor, page: &EventPage, terminal_high_water: u64) {
    assert!(
        cursor.after_sequence <= terminal_high_water
            && page.next_cursor.after_sequence <= terminal_high_water,
        "cursor advanced beyond terminal high-water mark"
    );
    assert!(
        cursor.after_sequence == terminal_high_water
            || page.next_cursor.after_sequence > cursor.after_sequence,
        "cursor page did not advance before terminal high-water mark"
    );
}

fn distinct_session_id(session_id: &str, candidate: &str) -> String {
    if session_id == candidate {
        format!("{candidate}:distinct")
    } else {
        candidate.to_owned()
    }
}

fn assert_returned_session_matches_adoption(
    returned_session_id: &str,
    disposition: &AdoptionDisposition,
) {
    match disposition {
        AdoptionDisposition::Adopted { session_id } => assert_eq!(
            returned_session_id, session_id,
            "returned reconciliation session does not match durable adoption"
        ),
        AdoptionDisposition::ProvenNotAdopted | AdoptionDisposition::Unknown => {
            panic!("original adoption is not durably adopted")
        }
    }
}

async fn lookup_durable_adoption(
    fixture: &mut dyn CovenConformanceFixture,
    correlation: &ExecutionCorrelation,
) -> AdoptionDisposition {
    fixture
        .port()
        .lookup(&correlation.request_id)
        .await
        .unwrap_or_else(|error| panic!("durable adoption lookup must succeed: {error}"))
}

fn schema(value: &str) -> SchemaVersion {
    match SchemaVersion::parse(value) {
        Ok(value) => value,
        Err(error) => panic!("static schema version is valid: {error}"),
    }
}

fn record_id(kind: RecordKind, value: u8) -> RecordId {
    let suffix = format!("01J000000000000000000000{value:02}");
    match RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())) {
        Ok(value) => value,
        Err(error) => panic!("static record identity is valid: {error}"),
    }
}

fn request_id(value: u8) -> RequestId {
    let value = format!("req_01J000000000000000000000{value:02}");
    match RequestId::parse(&value) {
        Ok(value) => value,
        Err(error) => panic!("static request identity is valid: {error}"),
    }
}

/// Verifies exact contract negotiation and fail-closed capability handling.
pub async fn assert_c_s1_contract_negotiation(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S1,
        UnsupportedCall::Negotiate,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    let before = fixture.observations().await;
    let profile = fixture
        .port()
        .negotiate(exact_negotiation_request())
        .await
        .unwrap_or_else(|error| panic!("exact G2 contract must negotiate: {error}"));
    assert_eq!(
        profile,
        CapabilityProfile {
            api_version: CONTRACT.to_owned(),
            capabilities: capability_names(),
        }
    );
    match fixture.port().adopt(launch_request()).await {
        Ok(AdoptionDisposition::Adopted { .. }) => {}
        Err(PortError::CapabilityMissing {}) => {
            assert_eq!(fixture.observations().await, before);
            panic!("stable_adoption was falsely advertised");
        }
        other => panic!("advertised stable_adoption method failed: {other:?}"),
    }
    fixture.reset().await;

    for unsupported in ["coven.daemon.v0", "coven.daemon.v2"] {
        assert_eq!(
            fixture
                .port()
                .negotiate(NegotiateRequest::new(unsupported))
                .await,
            Err(PortError::ContractUnsupported {})
        );
    }
    let mut missing = NegotiateRequest::new(CONTRACT);
    missing
        .required_capabilities
        .insert("future_capability".to_owned());
    assert_eq!(
        fixture.port().negotiate(missing).await,
        Err(PortError::CapabilityMissing {})
    );
    let mut unknown_method_capability = exact_negotiation_request();
    unknown_method_capability
        .required_capabilities
        .insert("falsely_advertised_method".to_owned());
    assert_eq!(
        fixture.port().negotiate(unknown_method_capability).await,
        Err(PortError::CapabilityMissing {})
    );
    assert_eq!(fixture.observations().await, before);
    ConformanceOutcome::Verified
}

/// Verifies launch, input attachment, observation, close, and lifecycle rejection.
pub async fn assert_c_s2_session_lifecycle(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) =
        expected_unsupported(fixture, CovenConformanceCase::C_S2, UnsupportedCall::Adopt).await
    {
        return outcome;
    }
    fixture.reset().await;
    let launch = launch_request();
    let launch_correlation = launch.correlation();
    let session_id = adopt_session(fixture, launch.clone(), "session launch must be adopted").await;
    let adopted = AdoptionDisposition::Adopted {
        session_id: session_id.clone(),
    };
    assert_eq!(
        fixture
            .port()
            .adopt(session_input_request(&session_id))
            .await,
        Ok(adopted)
    );
    let snapshot = fixture
        .port()
        .inspect(&session_id)
        .await
        .unwrap_or_else(|error| panic!("adopted session must be observable: {error}"));
    assert_eq!(snapshot.session_id, session_id);
    assert_eq!(snapshot.correlation, launch_correlation);

    let requested =
        termination_requested_binding(&launch, &snapshot.session_id, "operator_request");
    let mut persistence = MemoryTerminationPersistence::default();
    let disposition = persist_then_terminate(&mut persistence, fixture.port(), requested.clone())
        .await
        .unwrap_or_else(|error| panic!("persisted session close must succeed: {error}"));
    assert!(matches!(
        disposition,
        TerminationDisposition::Acknowledged { .. }
    ));
    let closed = fixture
        .port()
        .inspect(&snapshot.session_id)
        .await
        .unwrap_or_else(|error| panic!("closed session must remain observable: {error}"));
    assert_eq!(
        closed.terminal_state.as_deref(),
        Some("authoritatively_terminated")
    );

    for (pointer, replacement) in [
        ("/cwd", serde_json::json!("/outside/project")),
        ("/harness", serde_json::json!("unknown_harness")),
    ] {
        let mut value: serde_json::Value = match serde_json::from_slice(LAUNCH_GOLDEN) {
            Ok(value) => value,
            Err(error) => panic!("launch fixture must decode: {error}"),
        };
        let Some(field) = value.pointer_mut(pointer) else {
            panic!("static lifecycle mutation pointer must exist");
        };
        *field = replacement;
        let input: ExecutionRequestInput = match serde_json::from_value(value) {
            Ok(value) => value,
            Err(error) => panic!("invalid lifecycle input must remain typed: {error}"),
        };
        assert_eq!(AdoptionRequest::new(input), Err(PortError::InvalidRequest));
    }

    fixture.reset().await;
    assert_eq!(
        fixture
            .port()
            .adopt(session_input_request(&snapshot.session_id))
            .await,
        Err(PortError::NotFound)
    );
    assert_eq!(
        fixture.port().inspect(&snapshot.session_id).await,
        Err(PortError::NotFound)
    );
    let mut persistence = MemoryTerminationPersistence::default();
    assert!(matches!(
        persist_then_terminate(
            &mut persistence,
            fixture.port(),
            termination_requested_binding(&launch, &snapshot.session_id, "operator_request"),
        )
        .await,
        Err(TerminationDispatchError::Port(PortError::NotFound))
    ));
    ConformanceOutcome::Verified
}

/// Verifies exact snapshot echo and the complete independent correlation matrix.
pub async fn assert_c_s3_snapshot_attempt_binding(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S3,
        UnsupportedCall::Inspect,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    let adoption = launch_request();
    let correlation = adoption.correlation();
    let session_id = adopt_session(fixture, adoption, "snapshot setup must adopt").await;
    let snapshot = fixture
        .port()
        .inspect(&session_id)
        .await
        .unwrap_or_else(|error| panic!("snapshot must round-trip: {error}"));
    assert_eq!(snapshot.correlation, correlation);
    assert_eq!(snapshot.session_id, session_id);
    let valid_reconciliation = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('d'),
        reason_code: "return_original".to_owned(),
    };
    let valid_disposition = fixture
        .port()
        .reconcile(valid_reconciliation.clone())
        .await
        .unwrap_or_else(|error| panic!("valid snapshot correlation must reconcile: {error}"));
    valid_disposition
        .validate_for(&valid_reconciliation)
        .unwrap_or_else(|error| panic!("valid snapshot correlation must echo exactly: {error}"));
    assert_eq!(
        lookup_durable_adoption(fixture, &correlation).await,
        AdoptionDisposition::Adopted {
            session_id: session_id.clone(),
        },
        "reconciliation must not alter the original adoption"
    );

    let changed_correlations = changed_correlations(&correlation);
    let changed_count = u64::try_from(changed_correlations.len())
        .unwrap_or_else(|_| panic!("correlation matrix length must fit u64"));
    for (field, changed) in changed_correlations {
        let request = ReconciliationRequest {
            correlation: changed,
            ambiguity_digest: digest_of('d'),
            reason_code: "return_original".to_owned(),
        };
        assert_eq!(
            fixture.port().reconcile(request).await,
            Err(PortError::IntentConflict),
            "{field}"
        );
    }
    let observations = fixture.observations().await;
    assert_eq!(observations.adoption_calls, 1);
    assert_eq!(observations.reconciliation_calls, changed_count + 1);
    assert_eq!(
        observations.durable_reconciliation,
        disposition_observation(&valid_disposition)
    );
    ConformanceOutcome::Verified
}

/// Verifies stable adoption, full digest recomputation, and every-field binding.
pub async fn assert_c_s4_stable_adoption(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) =
        expected_unsupported(fixture, CovenConformanceCase::C_S4, UnsupportedCall::Adopt).await
    {
        return outcome;
    }
    fixture.reset().await;
    let request = launch_request();
    assert_eq!(
        request.recompute_digest(),
        Ok(request.request_digest().clone())
    );
    request
        .validate_digest()
        .unwrap_or_else(|error| panic!("authority must recompute the canonical request: {error}"));
    require_fault(fixture, CovenFaultPoint::AdoptionAfterCommit).await;
    assert_eq!(
        fixture.port().adopt(request.clone()).await,
        Err(PortError::Unavailable)
    );
    assert_eq!(fixture.observations().await.adoption_calls, 1);
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let disposition = fixture
        .port()
        .adopt(request.clone())
        .await
        .unwrap_or_else(|error| panic!("durable adoption must replay: {error}"));
    let AdoptionDisposition::Adopted { session_id } = &disposition else {
        panic!("durable adoption replay must remain adopted");
    };
    assert_eq!(fixture.observations().await.adoption_calls, 0);
    assert_eq!(
        fixture.port().adopt(request.clone()).await,
        Ok(disposition.clone())
    );
    assert_eq!(fixture.observations().await.adoption_calls, 0);

    let input = session_input_request(session_id);
    let input_disposition = fixture
        .port()
        .adopt(input.clone())
        .await
        .unwrap_or_else(|error| panic!("valid session input must adopt: {error}"));
    for (typed, expected_disposition) in [(request, disposition), (input, input_disposition)] {
        let request_id = typed.correlation().request_id;
        let durable_before = fixture
            .port()
            .lookup(&request_id)
            .await
            .unwrap_or_else(|error| panic!("durable adoption snapshot must succeed: {error}"));
        assert_eq!(durable_before, expected_disposition);
        let conflicting = same_id_conflicting_request(&typed);
        assert_eq!(conflicting.correlation().request_id, request_id);
        assert_ne!(conflicting.request_digest(), typed.request_digest());
        let before = fixture.observations().await;
        assert_eq!(
            fixture.port().adopt(conflicting).await,
            Err(PortError::IntentConflict)
        );
        assert_eq!(fixture.observations().await, before);
        assert_eq!(
            fixture
                .port()
                .lookup(&request_id)
                .await
                .unwrap_or_else(|error| panic!("conflict lookup must remain durable: {error}")),
            durable_before
        );
        assert_eq!(
            fixture.port().adopt(typed.clone()).await,
            Ok(expected_disposition.clone())
        );
        assert_eq!(fixture.observations().await, before);
        for (field, forged) in stale_digest_mutations(&typed) {
            let forged_request_id = forged.correlation().request_id;
            let forged_durable_before = fixture
                .port()
                .lookup(&forged_request_id)
                .await
                .unwrap_or_else(|error| panic!("forged adoption snapshot failed: {error}"));
            let before = fixture.observations().await;
            assert_eq!(
                fixture.port().adopt(forged).await,
                Err(PortError::RequestDigestMismatch),
                "{field}"
            );
            assert_eq!(fixture.observations().await, before, "{field}");
            assert_eq!(
                fixture
                    .port()
                    .lookup(&request_id)
                    .await
                    .unwrap_or_else(|error| panic!("durable adoption lookup failed: {error}")),
                durable_before,
                "{field}"
            );
            assert_eq!(
                fixture
                    .port()
                    .lookup(&forged_request_id)
                    .await
                    .unwrap_or_else(|error| panic!("forged adoption lookup failed: {error}")),
                forged_durable_before,
                "{field}"
            );
            assert_eq!(
                fixture.port().adopt(typed.clone()).await,
                Ok(expected_disposition.clone()),
                "{field}"
            );
            assert_eq!(fixture.observations().await, before, "{field}");
        }
    }
    ConformanceOutcome::Verified
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedispatchDecision {
    Blocked,
    RedispatchEligible,
}

/// Verifies adopted, proven-not-adopted, and unknown lookup authority.
pub async fn assert_c_s5_non_adoption_proof(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) =
        expected_unsupported(fixture, CovenConformanceCase::C_S5, UnsupportedCall::Lookup).await
    {
        return outcome;
    }
    fixture.reset().await;
    let launch = launch_request();
    let session_id = adopt_session(fixture, launch.clone(), "lookup setup must adopt").await;
    let adopted = AdoptionDisposition::Adopted {
        session_id: session_id.clone(),
    };
    assert_eq!(
        fixture
            .port()
            .lookup(&launch.correlation().request_id)
            .await,
        Ok(adopted)
    );
    let not_adopted = fixture
        .port()
        .lookup(&request_id(1))
        .await
        .unwrap_or_else(|error| panic!("durable non-adoption proof must be available: {error}"));
    let unknown = fixture
        .port()
        .lookup(&request_id(2))
        .await
        .unwrap_or_else(|error| panic!("unknown adoption must be explicit: {error}"));
    assert_eq!(not_adopted, AdoptionDisposition::ProvenNotAdopted);
    assert_eq!(unknown, AdoptionDisposition::Unknown);
    fixture.restart().await;
    assert_eq!(
        fixture.port().lookup(&request_id(1)).await,
        Ok(AdoptionDisposition::ProvenNotAdopted)
    );
    assert_eq!(
        fixture.port().lookup(&request_id(2)).await,
        Ok(AdoptionDisposition::Unknown)
    );
    assert_eq!(
        redispatch_decision(&AdoptionDisposition::ProvenNotAdopted),
        RedispatchDecision::RedispatchEligible
    );
    assert_eq!(redispatch_decision(&unknown), RedispatchDecision::Blocked);
    assert_eq!(
        redispatch_decision(&AdoptionDisposition::Adopted { session_id }),
        RedispatchDecision::Blocked
    );
    ConformanceOutcome::Verified
}

fn redispatch_decision(disposition: &AdoptionDisposition) -> RedispatchDecision {
    if disposition == &AdoptionDisposition::ProvenNotAdopted {
        RedispatchDecision::RedispatchEligible
    } else {
        RedispatchDecision::Blocked
    }
}

/// Verifies correlation-bound return-or-fence recovery without redispatch.
pub async fn assert_c_s6_ambiguity_fence(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S6,
        UnsupportedCall::Reconcile,
    )
    .await
    {
        return outcome;
    }

    assert_reconciliation_terminal(fixture, false).await;
    assert_reconciliation_terminal(fixture, true).await;

    for point in [
        CovenFaultPoint::ReconcileBeforeDisposition,
        CovenFaultPoint::ReconcileStall,
    ] {
        fixture.reset().await;
        let correlation = mark_ambiguous(fixture).await;
        let original_adoption = lookup_durable_adoption(fixture, &correlation).await;
        let request = reconciliation_request(correlation, false);
        require_fault(fixture, point).await;
        let expected_error = if point == CovenFaultPoint::ReconcileStall {
            PortError::Stalled
        } else {
            PortError::Unavailable
        };
        assert_eq!(
            fixture.port().reconcile(request.clone()).await,
            Err(expected_error)
        );
        assert_eq!(
            fixture.observations().await,
            CovenConformanceObservations {
                adoption_calls: 1,
                reconciliation_calls: 1,
                durable_reconciliation: None,
            }
        );
        fixture.restart().await;
        let blocked = fixture.observations().await;
        assert_eq!(blocked.adoption_calls, 0);
        assert_eq!(blocked.reconciliation_calls, 0);
        assert!(blocked.durable_reconciliation.is_none());
        assert_eq!(
            fixture
                .redispatch_eligibility(&request.correlation)
                .await
                .unwrap_or_else(|error| panic!("ambiguous eligibility lookup failed: {error}")),
            RedispatchEligibility::Blocked
        );
        assert_eq!(
            fixture.observations().await.adoption_calls,
            blocked.adoption_calls,
            "ambiguous eligibility observation must not dispatch"
        );
        let recovered = fixture
            .port()
            .reconcile(request.clone())
            .await
            .unwrap_or_else(|error| panic!("cleared reconciliation must recover: {error}"));
        let ReconciliationDisposition::Returned { session_id, .. } = &recovered else {
            panic!("cleared reconciliation must return the original session");
        };
        assert_returned_session_matches_adoption(session_id, &original_adoption);
        assert_eq!(
            lookup_durable_adoption(fixture, &request.correlation).await,
            original_adoption,
            "recovered reconciliation must not alter the original adoption"
        );
        let recovered_observations = fixture.observations().await;
        assert_eq!(recovered_observations.adoption_calls, 0);
        assert_eq!(recovered_observations.reconciliation_calls, 1);
    }

    for fenced in [false, true] {
        fixture.reset().await;
        let correlation = mark_ambiguous(fixture).await;
        let original_adoption = lookup_durable_adoption(fixture, &correlation).await;
        let request = reconciliation_request(correlation, fenced);
        require_fault(fixture, CovenFaultPoint::ReconcileAfterDisposition).await;
        assert_eq!(
            fixture.port().reconcile(request.clone()).await,
            Err(PortError::Unavailable)
        );
        let committed = fixture.observations().await;
        assert_eq!(committed.adoption_calls, 1);
        assert_eq!(committed.reconciliation_calls, 1);
        let committed_observation = committed
            .durable_reconciliation
            .clone()
            .unwrap_or_else(|| panic!("after-disposition fault must retain a terminal outcome"));
        assert_eq!(
            matches!(
                committed_observation.kind,
                DurableDispositionKind::Fenced { .. }
            ),
            fenced
        );
        fixture.restart().await;
        require_clear_fault(fixture).await;
        let replay = fixture
            .port()
            .reconcile(request.clone())
            .await
            .unwrap_or_else(|error| {
                panic!("durable terminal outcome must replay after restart: {error}")
            });
        assert_eq!(
            disposition_observation(&replay),
            Some(committed_observation)
        );
        if let ReconciliationDisposition::Returned { session_id, .. } = &replay {
            assert_returned_session_matches_adoption(session_id, &original_adoption);
        }
        assert_eq!(
            lookup_durable_adoption(fixture, &request.correlation).await,
            original_adoption,
            "replayed reconciliation must not alter the original adoption"
        );
        let replayed = fixture.observations().await;
        assert_eq!(replayed.adoption_calls, 0);
        assert_eq!(replayed.reconciliation_calls, 1);
    }
    ConformanceOutcome::Verified
}

async fn mark_ambiguous(fixture: &mut dyn CovenConformanceFixture) -> ExecutionCorrelation {
    let adoption = launch_request();
    let correlation = adoption.correlation();
    require_fault(fixture, CovenFaultPoint::AdoptionAfterCommit).await;
    assert_eq!(
        fixture.port().adopt(adoption).await,
        Err(PortError::Unavailable)
    );
    require_clear_fault(fixture).await;
    require_fault(fixture, CovenFaultPoint::LookupAfterRead).await;
    let local_disposition = match fixture.port().lookup(&correlation.request_id).await {
        Err(PortError::Unavailable) => AdoptionDisposition::Unknown,
        other => panic!("lost lookup response must remain locally unknown: {other:?}"),
    };
    assert_eq!(local_disposition, AdoptionDisposition::Unknown);
    require_clear_fault(fixture).await;
    assert_eq!(fixture.observations().await.adoption_calls, 1);
    correlation
}

fn reconciliation_request(
    correlation: ExecutionCorrelation,
    fenced: bool,
) -> ReconciliationRequest {
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

async fn assert_reconciliation_terminal(fixture: &mut dyn CovenConformanceFixture, fenced: bool) {
    fixture.reset().await;
    let correlation = mark_ambiguous(fixture).await;
    let original_adoption = lookup_durable_adoption(fixture, &correlation).await;
    let request = reconciliation_request(correlation.clone(), fenced);
    let disposition = fixture
        .port()
        .reconcile(request.clone())
        .await
        .unwrap_or_else(|error| panic!("terminal reconciliation must succeed: {error}"));
    disposition
        .validate_for(&request)
        .unwrap_or_else(|error| panic!("terminal reconciliation must be correlated: {error}"));
    match &disposition {
        ReconciliationDisposition::Returned {
            session_id,
            correlation: echoed,
            ambiguity_digest,
            disposition_id,
            recorded_at,
        } => {
            assert!(!fenced);
            assert!(!session_id.is_empty());
            assert_eq!(echoed, &correlation);
            assert_eq!(ambiguity_digest, &request.ambiguity_digest);
            assert!(!disposition_id.is_empty());
            assert!(*recorded_at >= correlation.created_at);
            assert_returned_session_matches_adoption(session_id, &original_adoption);
            let resumed = fixture
                .port()
                .inspect(session_id)
                .await
                .unwrap_or_else(|error| panic!("returned session must resume: {error}"));
            assert_eq!(resumed.correlation, correlation);
            assert_eq!(
                redispatch_decision(&AdoptionDisposition::Adopted {
                    session_id: session_id.clone(),
                }),
                RedispatchDecision::Blocked
            );
            assert_eq!(
                reconciliation_redispatch_decision(&disposition),
                RedispatchDecision::Blocked
            );
        }
        ReconciliationDisposition::Fenced {
            fence_token,
            correlation: echoed,
            ambiguity_digest,
            disposition_id,
            recorded_at,
        } => {
            assert!(fenced);
            assert!(!fence_token.is_empty());
            assert_eq!(echoed, &correlation);
            assert_eq!(ambiguity_digest, &request.ambiguity_digest);
            assert!(!disposition_id.is_empty());
            assert!(*recorded_at >= correlation.created_at);
            assert_eq!(
                reconciliation_redispatch_decision(&disposition),
                RedispatchDecision::RedispatchEligible
            );
        }
        ReconciliationDisposition::Unresolved => {
            panic!("terminal script must not return unresolved")
        }
    }
    assert_eq!(
        lookup_durable_adoption(fixture, &correlation).await,
        original_adoption,
        "terminal reconciliation must not alter the original adoption"
    );

    let first_observation = fixture
        .observations()
        .await
        .durable_reconciliation
        .unwrap_or_else(|| panic!("terminal disposition must be durably observable"));
    assert_eq!(
        disposition_observation(&disposition),
        Some(first_observation.clone())
    );
    let adoption_calls_before_eligibility = fixture.observations().await.adoption_calls;
    assert_eq!(
        adoption_calls_before_eligibility, 1,
        "reconciliation must not dispatch a second adoption"
    );
    let eligibility = fixture
        .redispatch_eligibility(&correlation)
        .await
        .unwrap_or_else(|error| panic!("redispatch eligibility must be observable: {error}"));
    assert_eq!(
        eligibility,
        if fenced {
            RedispatchEligibility::EligibleAfterFence
        } else {
            RedispatchEligibility::Blocked
        }
    );
    assert_eq!(
        fixture.observations().await.adoption_calls,
        adoption_calls_before_eligibility,
        "eligibility observation must not dispatch"
    );
    fixture.restart().await;
    let replay = fixture
        .port()
        .reconcile(request.clone())
        .await
        .unwrap_or_else(|error| panic!("terminal disposition must replay: {error}"));
    assert_eq!(replay, disposition);
    assert_eq!(
        fixture.observations().await.durable_reconciliation,
        Some(first_observation.clone())
    );

    let changed = changed_correlations(&correlation);
    let changed_count = u64::try_from(changed.len())
        .unwrap_or_else(|_| panic!("correlation matrix length must fit u64"));
    for (field, changed) in changed {
        let changed_request = ReconciliationRequest {
            correlation: changed,
            ..request.clone()
        };
        assert_eq!(
            fixture.port().reconcile(changed_request).await,
            Err(PortError::IntentConflict),
            "{field}"
        );
    }
    let changed_digest = ReconciliationRequest {
        ambiguity_digest: digest_of('e'),
        ..request
    };
    assert_eq!(
        fixture.port().reconcile(changed_digest).await,
        Err(PortError::IntentConflict),
        "ambiguity_digest"
    );
    let observations = fixture.observations().await;
    assert_eq!(observations.adoption_calls, 0);
    assert_eq!(observations.reconciliation_calls, 2 + changed_count);
    assert_eq!(observations.durable_reconciliation, Some(first_observation));
}

fn reconciliation_redispatch_decision(
    disposition: &ReconciliationDisposition,
) -> RedispatchDecision {
    if matches!(disposition, ReconciliationDisposition::Fenced { .. }) {
        RedispatchDecision::RedispatchEligible
    } else {
        RedispatchDecision::Blocked
    }
}

/// Verifies ordered, restart-stable cursors without gaps, duplicates, or drift.
pub async fn assert_c_s7_ordered_cursor(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) =
        expected_unsupported(fixture, CovenConformanceCase::C_S7, UnsupportedCall::Events).await
    {
        return outcome;
    }
    fixture.reset().await;
    let session_id = adopt_session(
        fixture,
        launch_request(),
        "cursor setup must adopt a session",
    )
    .await;
    let initial = EventCursor {
        session_id: session_id.clone(),
        after_sequence: 0,
    };
    require_fault(fixture, CovenFaultPoint::CursorBeforePage).await;
    assert_eq!(
        fixture.port().events(initial.clone()).await,
        Err(PortError::Unavailable)
    );
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let first = fixture
        .port()
        .events(initial.clone())
        .await
        .unwrap_or_else(|error| panic!("cursor must recover before-page fault: {error}"));
    first
        .validate_for(&initial)
        .unwrap_or_else(|error| panic!("first cursor page must validate: {error}"));
    assert_cursor_page_progress(&initial, &first, RAW_LEDGER_STATES.len() as u64);
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    fixture.restart().await;
    let mut cursor = first.next_cursor.clone();
    let mut all = first.events;
    while cursor.after_sequence < RAW_LEDGER_STATES.len() as u64 {
        let page = fixture
            .port()
            .events(cursor.clone())
            .await
            .unwrap_or_else(|error| panic!("ordered cursor page must succeed: {error}"));
        page.validate_for(&cursor)
            .unwrap_or_else(|error| panic!("ordered cursor page must validate: {error}"));
        assert_cursor_page_progress(&cursor, &page, RAW_LEDGER_STATES.len() as u64);
        all.extend(page.events);
        cursor = page.next_cursor;
    }
    assert_eq!(
        all.iter().map(|event| event.sequence).collect::<Vec<_>>(),
        (1..=RAW_LEDGER_STATES.len() as u64).collect::<Vec<_>>()
    );
    let unique = all
        .iter()
        .map(|event| event.sequence)
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), all.len());
    assert_eq!(
        all.iter()
            .map(|event| event.terminal_state.as_deref())
            .collect::<Vec<_>>(),
        RAW_LEDGER_STATES
            .iter()
            .copied()
            .map(Some)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fixture
            .port()
            .events(EventCursor {
                session_id: session_id.clone(),
                after_sequence: RAW_LEDGER_STATES.len() as u64 + 1,
            })
            .await,
        Err(PortError::InvalidRequest)
    );
    assert_eq!(
        fixture
            .port()
            .events(EventCursor {
                session_id: session_id.clone(),
                after_sequence: 1,
            })
            .await,
        Err(PortError::IntentConflict)
    );
    let foreign_session_id = distinct_session_id(&session_id, "foreign-session");
    assert_eq!(
        fixture
            .port()
            .events(EventCursor {
                session_id: foreign_session_id,
                after_sequence: 0,
            })
            .await,
        Err(PortError::CorrelationMismatch)
    );

    fixture.reset().await;
    let reset_session_id =
        adopt_session(fixture, launch_request(), "cursor fault setup must adopt").await;
    let reset_initial = EventCursor {
        session_id: reset_session_id,
        after_sequence: 0,
    };
    require_fault(fixture, CovenFaultPoint::CursorAfterPage).await;
    assert_eq!(
        fixture.port().events(reset_initial.clone()).await,
        Err(PortError::Unavailable)
    );
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let replayed = fixture
        .port()
        .events(reset_initial.clone())
        .await
        .unwrap_or_else(|error| panic!("committed page must replay: {error}"));
    replayed
        .validate_for(&reset_initial)
        .unwrap_or_else(|error| panic!("replayed cursor page must validate: {error}"));
    assert_cursor_page_progress(&reset_initial, &replayed, RAW_LEDGER_STATES.len() as u64);
    assert_eq!(replayed.next_cursor.after_sequence, 3);
    ConformanceOutcome::Verified
}

/// Verifies that only durable typed authority can establish terminal state.
pub async fn assert_c_s8_terminal_authority(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S8,
        UnsupportedCall::Terminate,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    let launch = launch_request();
    let session_id = adopt_session(
        fixture,
        launch.clone(),
        "terminal setup must adopt a session",
    )
    .await;
    let snapshot = fixture
        .port()
        .inspect(&session_id)
        .await
        .unwrap_or_else(|error| panic!("raw session status must be readable: {error}"));
    assert_eq!(snapshot.terminal_state.as_deref(), Some("created"));
    let raw_page = fixture
        .port()
        .events(EventCursor {
            session_id: session_id.clone(),
            after_sequence: 0,
        })
        .await
        .unwrap_or_else(|error| panic!("raw ledger events must be readable: {error}"));
    for raw in std::iter::once(snapshot.terminal_state.as_deref()).chain(
        raw_page
            .events
            .iter()
            .map(|event| event.terminal_state.as_deref()),
    ) {
        assert!(raw.is_some());
        assert_ne!(raw, Some("authoritatively_terminated"));
    }

    let requested = termination_requested_binding(&launch, &session_id, "operator_request");
    let mut unproven = requested.clone();
    unproven.cancellation_state = CancellationState::AcknowledgedTerminated;
    unproven.terminal_state = Some("process_exited".to_owned());
    assert!(unproven.validate().is_err());
    unproven.terminal_state = Some("disconnected".to_owned());
    assert!(unproven.validate().is_err());

    require_fault(fixture, CovenFaultPoint::TerminalBeforePersistence).await;
    let mut persistence = MemoryTerminationPersistence::default();
    assert!(matches!(
        persist_then_terminate(&mut persistence, fixture.port(), requested.clone(),).await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    fixture.restart().await;
    let still_raw = fixture
        .port()
        .inspect(&session_id)
        .await
        .unwrap_or_else(|error| {
            panic!("unpersisted terminal must remain observable only as raw: {error}")
        });
    assert_ne!(
        still_raw.terminal_state.as_deref(),
        Some("authoritatively_terminated")
    );
    require_clear_fault(fixture).await;
    let acknowledged = persist_then_terminate(&mut persistence, fixture.port(), requested.clone())
        .await
        .unwrap_or_else(|error| panic!("durable terminal acknowledgement must succeed: {error}"));
    assert!(matches!(
        acknowledged,
        TerminationDisposition::Acknowledged { .. }
    ));
    fixture.restart().await;
    let replay = persist_then_terminate(&mut persistence, fixture.port(), requested)
        .await
        .unwrap_or_else(|error| panic!("durable terminal acknowledgement must replay: {error}"));
    assert_eq!(replay, acknowledged);
    assert_eq!(
        fixture
            .port()
            .inspect(&session_id)
            .await
            .unwrap_or_else(|error| panic!("durable terminal must survive restart: {error}"))
            .terminal_state
            .as_deref(),
        Some("authoritatively_terminated")
    );
    ConformanceOutcome::Verified
}

/// Verifies core-owned, correlated, durable cancellation acknowledgement.
pub async fn assert_c_s9_cancellation_acknowledgement(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S9,
        UnsupportedCall::Terminate,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    let launch = launch_request();
    let session_id = adopt_session(
        fixture,
        launch.clone(),
        "cancellation setup must adopt a session",
    )
    .await;

    let mut snapshot_states = Vec::new();
    for _ in 0..RAW_LEDGER_STATES.len() {
        snapshot_states.push(
            fixture
                .port()
                .inspect(&session_id)
                .await
                .unwrap_or_else(|error| panic!("raw snapshot must be readable: {error}"))
                .terminal_state
                .unwrap_or_else(|| panic!("scripted snapshot must name its raw state")),
        );
    }
    assert_eq!(snapshot_states.last().map(String::as_str), Some("orphaned"));
    fixture.restart().await;
    assert_eq!(
        fixture
            .port()
            .inspect(&session_id)
            .await
            .unwrap_or_else(|error| panic!("restart snapshot must be readable: {error}"))
            .terminal_state
            .as_deref(),
        Some("created"),
        "volatile raw-status cursor must restart"
    );
    assert_eq!(
        snapshot_states,
        RAW_LEDGER_STATES
            .iter()
            .map(|state| (*state).to_owned())
            .collect::<Vec<_>>()
    );

    let mut event_states = Vec::new();
    let mut cursor = EventCursor {
        session_id: session_id.clone(),
        after_sequence: 0,
    };
    while cursor.after_sequence < RAW_LEDGER_STATES.len() as u64 {
        let page = fixture
            .port()
            .events(cursor.clone())
            .await
            .unwrap_or_else(|error| panic!("raw event table must be readable: {error}"));
        assert_cursor_page_progress(&cursor, &page, RAW_LEDGER_STATES.len() as u64);
        event_states.extend(page.events.into_iter().map(|event| {
            event
                .terminal_state
                .unwrap_or_else(|| panic!("scripted raw event must name its state"))
        }));
        cursor = page.next_cursor;
    }
    assert_eq!(event_states, snapshot_states);
    let requested = termination_requested_binding(&launch, &session_id, "operator_request");
    for state in &snapshot_states {
        let mut raw_only = requested.clone();
        raw_only.cancellation_state = CancellationState::AcknowledgedTerminated;
        raw_only.terminal_state = Some(state.clone());
        assert!(raw_only.validate().is_err(), "{state}");
    }

    fixture.reset().await;
    let unresolved_session_id =
        adopt_session(fixture, launch.clone(), "unresolved setup must adopt").await;
    let unresolved_requested =
        termination_requested_binding(&launch, &unresolved_session_id, "force_unresolved");
    require_fault(fixture, CovenFaultPoint::CancellationBeforeAcknowledgement).await;
    let mut unresolved_persistence = MemoryTerminationPersistence::default();
    assert!(matches!(
        persist_then_terminate(
            &mut unresolved_persistence,
            fixture.port(),
            unresolved_requested.clone(),
        )
        .await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let unresolved = persist_then_terminate(
        &mut unresolved_persistence,
        fixture.port(),
        unresolved_requested.clone(),
    )
    .await
    .unwrap_or_else(|error| panic!("silence must resolve only to typed unresolved: {error}"));
    let TerminationDisposition::Unresolved { evidence } = &unresolved else {
        panic!("silence cannot produce acknowledgement");
    };
    evidence
        .validate()
        .unwrap_or_else(|error| panic!("unresolved evidence must be valid: {error}"));
    let unresolved_binding =
        derive_termination_outcome_revision(&unresolved_requested, &unresolved)
            .unwrap_or_else(|error| panic!("unresolved evidence must derive an outcome: {error}"));
    assert_eq!(
        unresolved_binding.cancellation_state,
        CancellationState::TerminationUnknown
    );
    fixture.restart().await;
    assert_eq!(
        persist_then_terminate(
            &mut unresolved_persistence,
            fixture.port(),
            unresolved_requested,
        )
        .await
        .unwrap_or_else(|error| panic!("unresolved disposition must replay: {error}")),
        unresolved
    );

    fixture.reset().await;
    let acknowledged_session_id =
        adopt_session(fixture, launch.clone(), "acknowledgement setup must adopt").await;
    let acknowledged_requested =
        termination_requested_binding(&launch, &acknowledged_session_id, "operator_request");
    require_fault(fixture, CovenFaultPoint::CancellationAfterAcknowledgement).await;
    let mut acknowledged_persistence = MemoryTerminationPersistence::default();
    assert!(matches!(
        persist_then_terminate(
            &mut acknowledged_persistence,
            fixture.port(),
            acknowledged_requested.clone(),
        )
        .await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let mut conflicting_requested = acknowledged_requested.clone();
    conflicting_requested.termination_reason_code = Some("policy_override".to_owned());
    let mut conflicting_persistence = MemoryTerminationPersistence::default();
    assert!(matches!(
        persist_then_terminate(
            &mut conflicting_persistence,
            fixture.port(),
            conflicting_requested,
        )
        .await,
        Err(TerminationDispatchError::Port(PortError::IntentConflict))
    ));
    let acknowledged = persist_then_terminate(
        &mut acknowledged_persistence,
        fixture.port(),
        acknowledged_requested.clone(),
    )
    .await
    .unwrap_or_else(|error| panic!("durable acknowledgement must recover: {error}"));
    let TerminationDisposition::Acknowledged { evidence } = &acknowledged else {
        panic!("authority acknowledgement must remain typed");
    };
    evidence
        .validate()
        .unwrap_or_else(|error| panic!("authority acknowledgement must validate: {error}"));
    let acknowledged_binding =
        derive_termination_outcome_revision(&acknowledged_requested, &acknowledged)
            .unwrap_or_else(|error| panic!("acknowledgement must derive an outcome: {error}"));
    assert_eq!(
        acknowledged_binding.cancellation_state,
        CancellationState::AcknowledgedTerminated
    );
    assert_invalid_acknowledgements(&acknowledged_requested, evidence);
    fixture.restart().await;
    assert_eq!(
        persist_then_terminate(
            &mut acknowledged_persistence,
            fixture.port(),
            acknowledged_requested.clone(),
        )
        .await
        .unwrap_or_else(|error| panic!("acknowledgement must replay idempotently: {error}")),
        acknowledged
    );

    let mut already = evidence.clone();
    already.kind = CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
    let already = TerminationDisposition::Acknowledged { evidence: already };
    assert_eq!(
        derive_termination_outcome_revision(&acknowledged_requested, &already)
            .unwrap_or_else(|error| panic!(
                "typed already-terminal evidence must validate: {error}"
            ))
            .cancellation_state,
        CancellationState::AcknowledgedAlreadyTerminal
    );
    ConformanceOutcome::Verified
}

fn assert_invalid_acknowledgements(
    requested: &ExecutionBinding,
    valid: &CancellationAcknowledgementEvidence,
) {
    let mut mutations = Vec::new();
    let mut changed = valid.clone();
    changed.acknowledgement_id.clear();
    mutations.push(("acknowledgement_id", changed));
    let mut changed = valid.clone();
    changed.termination_request_id = request_id(8);
    mutations.push(("termination_request_id", changed));
    let mut changed = valid.clone();
    changed.session_id = distinct_session_id(&valid.session_id, "session-other");
    mutations.push(("session_id", changed));
    let mut changed = valid.clone();
    changed.execution_request_id = request_id(8);
    mutations.push(("execution_request_id", changed));
    let mut changed = valid.clone();
    changed.execution_request_digest = digest_of('a');
    mutations.push(("execution_request_digest", changed));
    let mut changed = valid.clone();
    changed.authority_evidence_digest = digest_of('0');
    mutations.push(("authority_evidence_digest", changed));
    let mut changed = valid.clone();
    changed.acknowledged_at = at("2026-08-05T14:01:59Z");
    mutations.push(("acknowledged_at_before", changed));
    let mut changed = valid.clone();
    changed.acknowledged_at = at("2026-08-05T14:04:01Z");
    mutations.push(("acknowledged_at_after", changed));

    for (field, evidence) in mutations {
        assert!(
            derive_termination_outcome_revision(
                requested,
                &TerminationDisposition::Acknowledged { evidence },
            )
            .is_err(),
            "{field}"
        );
    }
}

/// Verifies the strict complete result and every independent content binding.
pub async fn assert_c_s10_result_artifact_binding(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S10,
        UnsupportedCall::Result,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    let launch = launch_request();
    let launch_correlation = launch.correlation();
    let session_id = adopt_session(fixture, launch, "result setup must adopt a session").await;
    let golden: ResultBundle = match serde_json::from_slice(RESULT_GOLDEN) {
        Ok(bundle) => bundle,
        Err(error) => panic!("strict result fixture must decode: {error}"),
    };
    golden
        .validate()
        .unwrap_or_else(|error| panic!("strict result fixture must validate: {error}"));
    assert_eq!(golden.correlation, launch_correlation);
    assert_eq!(
        canonical_bytes(&golden)
            .unwrap_or_else(|error| panic!("result fixture must canonicalize: {error}")),
        RESULT_GOLDEN
    );
    let mut expected = golden;
    expected.session_id = session_id.clone();
    for artifact in &mut expected.artifacts {
        artifact.session_id = session_id.clone();
    }
    expected
        .validate()
        .unwrap_or_else(|error| panic!("result fixture must accept the opaque session: {error}"));
    let actual = fixture
        .port()
        .result(&session_id)
        .await
        .unwrap_or_else(|error| panic!("complete result must be returned: {error}"));
    assert_eq!(actual, expected);
    assert_complete_result(&actual, &expected)
        .unwrap_or_else(|field| panic!("complete result mismatch: {field}"));

    for (field, correlation) in changed_correlations(&expected.correlation) {
        let mut bundle_changed = expected.clone();
        bundle_changed.correlation = correlation.clone();
        assert_complete_result_rejected(&bundle_changed, &expected, field);

        let mut artifact_changed = expected.clone();
        artifact_changed.artifacts[0].correlation = correlation;
        assert_complete_result_rejected(&artifact_changed, &expected, &format!("artifact_{field}"));
    }

    let wrong_session_id = distinct_session_id(&session_id, "session-other");
    let mut wrong_session = expected.clone();
    wrong_session.session_id = wrong_session_id.clone();
    assert_complete_result_rejected(&wrong_session, &expected, "session_id");
    let mut wrong_artifact_session = expected.clone();
    wrong_artifact_session.artifacts[0].session_id = wrong_session_id;
    assert_complete_result_rejected(&wrong_artifact_session, &expected, "artifact_session_id");

    for (field, mutate) in [
        (
            "result.digest",
            mutate_result_digest as fn(&mut ResultBundle),
        ),
        ("result.media_type", mutate_result_media_type),
        ("result.size_bytes", mutate_result_size),
        ("result.expires_at", mutate_result_expiry),
        ("artifact.content.digest", mutate_artifact_digest),
        ("artifact.content.media_type", mutate_artifact_media_type),
        ("artifact.content.size_bytes", mutate_artifact_size),
        ("artifact.content.expires_at", mutate_artifact_expiry),
    ] {
        let mut changed = expected.clone();
        mutate(&mut changed);
        assert_complete_result_rejected(&changed, &expected, field);
    }

    let mut zero_result = expected.clone();
    zero_result.result.size_bytes = 0;
    assert!(zero_result.validate().is_err());
    let mut oversized_result = expected.clone();
    oversized_result.result.size_bytes = MAX_CONTENT_SIZE_BYTES + 1;
    assert!(oversized_result.validate().is_err());
    let mut safe_result = expected.clone();
    safe_result.result.size_bytes = MAX_CONTENT_SIZE_BYTES;
    safe_result
        .validate()
        .unwrap_or_else(|error| panic!("maximum content size must validate: {error}"));
    let mut malformed_result = expected.clone();
    malformed_result.result.media_type = "Application/JSON".to_owned();
    assert!(malformed_result.validate().is_err());
    let mut late_result = expected.clone();
    late_result.result.expires_at =
        expected.correlation.valid_until + time::Duration::nanoseconds(1);
    assert!(late_result.validate().is_err());

    let mut zero_artifact = expected.clone();
    zero_artifact.artifacts[0].content.size_bytes = 0;
    assert!(zero_artifact.validate().is_err());
    let mut oversized_artifact = expected.clone();
    oversized_artifact.artifacts[0].content.size_bytes = MAX_CONTENT_SIZE_BYTES + 1;
    assert!(oversized_artifact.validate().is_err());
    let mut safe_artifact = expected.clone();
    safe_artifact.artifacts[0].content.size_bytes = MAX_CONTENT_SIZE_BYTES;
    safe_artifact
        .validate()
        .unwrap_or_else(|error| panic!("maximum artifact size must validate: {error}"));
    let mut malformed_artifact = expected.clone();
    malformed_artifact.artifacts[0].content.media_type = "text/plain; charset=utf-8".to_owned();
    assert!(malformed_artifact.validate().is_err());
    let mut beyond_result = expected.clone();
    beyond_result.artifacts[0].content.expires_at =
        expected.result.expires_at + time::Duration::nanoseconds(1);
    assert!(beyond_result.validate().is_err());
    let mut beyond_correlation = expected.clone();
    beyond_correlation.artifacts[0].content.expires_at =
        expected.correlation.valid_until + time::Duration::nanoseconds(1);
    assert!(beyond_correlation.validate().is_err());

    let mut duplicate = expected.clone();
    duplicate.artifacts.push(expected.artifacts[0].clone());
    assert!(duplicate.validate().is_err());
    let mut omitted = expected.clone();
    omitted.artifacts.clear();
    assert_complete_result_rejected(&omitted, &expected, "complete_artifact_association");

    fixture.restart().await;
    assert_eq!(
        fixture
            .port()
            .result(&session_id)
            .await
            .unwrap_or_else(|error| panic!("complete result must replay: {error}")),
        expected
    );
    ConformanceOutcome::Verified
}

fn assert_complete_result(
    candidate: &ResultBundle,
    expected: &ResultBundle,
) -> Result<(), &'static str> {
    candidate.validate().map_err(|_| "typed_validation")?;
    if candidate.session_id != expected.session_id {
        return Err("session_id");
    }
    if candidate.correlation != expected.correlation {
        return Err("correlation");
    }
    if candidate.result != expected.result {
        return Err("result_content_reference");
    }
    if candidate.artifacts != expected.artifacts {
        return Err("complete_artifact_association");
    }
    Ok(())
}

fn assert_complete_result_rejected(candidate: &ResultBundle, expected: &ResultBundle, field: &str) {
    assert!(
        assert_complete_result(candidate, expected).is_err(),
        "{field}"
    );
}

fn mutate_result_digest(bundle: &mut ResultBundle) {
    bundle.result.digest = digest_of('a');
}

fn mutate_result_media_type(bundle: &mut ResultBundle) {
    bundle.result.media_type = "text/plain".to_owned();
}

fn mutate_result_size(bundle: &mut ResultBundle) {
    bundle.result.size_bytes = bundle.result.size_bytes.saturating_add(1);
}

fn mutate_result_expiry(bundle: &mut ResultBundle) {
    bundle.result.expires_at -= time::Duration::seconds(1);
}

fn mutate_artifact_digest(bundle: &mut ResultBundle) {
    bundle.artifacts[0].content.digest = digest_of('a');
}

fn mutate_artifact_media_type(bundle: &mut ResultBundle) {
    bundle.artifacts[0].content.media_type = "application/json".to_owned();
}

fn mutate_artifact_size(bundle: &mut ResultBundle) {
    bundle.artifacts[0].content.size_bytes =
        bundle.artifacts[0].content.size_bytes.saturating_add(1);
}

fn mutate_artifact_expiry(bundle: &mut ResultBundle) {
    bundle.artifacts[0].content.expires_at -= time::Duration::seconds(1);
}

#[derive(Debug, Clone, Copy)]
enum FaultScenario {
    Adoption,
    Lookup,
    Cursor,
    Termination,
    Result,
    Reconciliation,
}

const MANDATORY_FAULTS: &[(CovenFaultPoint, FaultScenario)] = &[
    (
        CovenFaultPoint::AdoptionBeforeCommit,
        FaultScenario::Adoption,
    ),
    (
        CovenFaultPoint::AdoptionAfterCommit,
        FaultScenario::Adoption,
    ),
    (CovenFaultPoint::InputBeforeCommit, FaultScenario::Adoption),
    (CovenFaultPoint::InputAfterCommit, FaultScenario::Adoption),
    (CovenFaultPoint::LookupBeforeRead, FaultScenario::Lookup),
    (CovenFaultPoint::LookupAfterRead, FaultScenario::Lookup),
    (CovenFaultPoint::CursorBeforePage, FaultScenario::Cursor),
    (CovenFaultPoint::CursorAfterPage, FaultScenario::Cursor),
    (
        CovenFaultPoint::CancellationBeforeAcknowledgement,
        FaultScenario::Termination,
    ),
    (
        CovenFaultPoint::CancellationAfterAcknowledgement,
        FaultScenario::Termination,
    ),
    (
        CovenFaultPoint::TerminalBeforePersistence,
        FaultScenario::Termination,
    ),
    (
        CovenFaultPoint::ResultBeforePersistence,
        FaultScenario::Result,
    ),
    (
        CovenFaultPoint::ArtifactBeforePersistence,
        FaultScenario::Result,
    ),
    (
        CovenFaultPoint::ReconcileBeforeDisposition,
        FaultScenario::Reconciliation,
    ),
    (
        CovenFaultPoint::ReconcileAfterDisposition,
        FaultScenario::Reconciliation,
    ),
    (
        CovenFaultPoint::ReconcileStall,
        FaultScenario::Reconciliation,
    ),
];

/// Verifies durable-before/after semantics for every declared fixture fault.
pub async fn assert_c_s11_restart_persistence(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S11,
        UnsupportedCall::Negotiate,
    )
    .await
    {
        return outcome;
    }
    assert_restart_resets_runtime(fixture).await;
    for &(point, scenario) in MANDATORY_FAULTS {
        match scenario {
            FaultScenario::Adoption => assert_adoption_fault_recovery(fixture, point).await,
            FaultScenario::Lookup => assert_lookup_fault_recovery(fixture, point).await,
            FaultScenario::Cursor => assert_cursor_fault_recovery(fixture, point).await,
            FaultScenario::Termination => assert_termination_fault_recovery(fixture, point).await,
            FaultScenario::Result => assert_result_fault_recovery(fixture, point).await,
            FaultScenario::Reconciliation => {
                assert_reconciliation_fault_recovery(fixture, point).await;
            }
        }
    }
    ConformanceOutcome::Verified
}

async fn assert_restart_resets_runtime(fixture: &mut dyn CovenConformanceFixture) {
    fixture.reset().await;
    let request = launch_request();
    let request_id = request.correlation().request_id;
    let adopted = fixture
        .port()
        .adopt(request)
        .await
        .unwrap_or_else(|error| panic!("restart setup must adopt: {error}"));
    let AdoptionDisposition::Adopted { session_id } = &adopted else {
        panic!("restart setup must return an adopted session");
    };
    assert_eq!(
        fixture
            .port()
            .inspect(session_id)
            .await
            .unwrap_or_else(|error| panic!("restart setup must inspect: {error}"))
            .terminal_state
            .as_deref(),
        Some("created")
    );
    require_fault(fixture, CovenFaultPoint::LookupBeforeRead).await;
    fixture.restart().await;
    assert_eq!(
        fixture.observations().await,
        CovenConformanceObservations::default()
    );
    assert_eq!(
        fixture.port().lookup(&request_id).await,
        Ok(adopted.clone()),
        "durable adoption must survive while the selected fault is cleared"
    );
    assert_eq!(
        fixture
            .port()
            .inspect(session_id)
            .await
            .unwrap_or_else(|error| panic!("durable session must survive restart: {error}"))
            .terminal_state
            .as_deref(),
        Some("created"),
        "volatile inspection cursor must restart"
    );
}

async fn assert_adoption_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let input_fault = matches!(
        point,
        CovenFaultPoint::InputBeforeCommit | CovenFaultPoint::InputAfterCommit
    );
    let session_id = if input_fault {
        Some(adopt_session(fixture, launch_request(), "input fault setup must adopt").await)
    } else {
        None
    };
    let request = if input_fault {
        session_input_request(
            session_id
                .as_deref()
                .unwrap_or_else(|| panic!("input fault requires an adopted session")),
        )
    } else {
        launch_request()
    };
    require_fault(fixture, point).await;
    assert_eq!(
        fixture.port().adopt(request.clone()).await,
        Err(PortError::Unavailable)
    );
    let calls_after_fault = fixture.observations().await.adoption_calls;
    assert_eq!(calls_after_fault, 1 + u64::from(input_fault), "{point:?}");
    fixture.restart().await;
    let durable_lookup = fixture
        .port()
        .lookup(&request.correlation().request_id)
        .await
        .unwrap_or_else(|error| panic!("{point:?} lookup observation failed: {error}"));
    let after_commit = matches!(
        point,
        CovenFaultPoint::AdoptionAfterCommit | CovenFaultPoint::InputAfterCommit
    );
    if after_commit {
        assert!(matches!(
            durable_lookup,
            AdoptionDisposition::Adopted { .. }
        ));
    } else {
        assert_eq!(durable_lookup, AdoptionDisposition::Unknown);
    }
    require_clear_fault(fixture).await;
    let recovered = fixture
        .port()
        .adopt(request.clone())
        .await
        .unwrap_or_else(|error| panic!("{point:?} must recover after restart: {error}"));
    let calls_after_recovery = fixture.observations().await.adoption_calls;
    assert_eq!(calls_after_recovery, u64::from(!after_commit), "{point:?}");
    fixture.restart().await;
    assert_eq!(
        fixture
            .port()
            .adopt(request)
            .await
            .unwrap_or_else(|error| panic!("{point:?} must replay: {error}")),
        recovered
    );
    assert_eq!(fixture.observations().await.adoption_calls, 0, "{point:?}");
}

async fn assert_lookup_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let launch = launch_request();
    let request_id = launch.correlation().request_id;
    let adopted = fixture
        .port()
        .adopt(launch)
        .await
        .unwrap_or_else(|error| panic!("lookup setup must adopt: {error}"));
    require_fault(fixture, point).await;
    assert_eq!(
        fixture.port().lookup(&request_id).await,
        Err(PortError::Unavailable)
    );
    fixture.restart().await;
    require_clear_fault(fixture).await;
    assert_eq!(
        fixture
            .port()
            .lookup(&request_id)
            .await
            .unwrap_or_else(|error| panic!("{point:?} lookup must recover: {error}")),
        adopted
    );
}

async fn assert_cursor_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let session_id =
        adopt_session(fixture, launch_request(), "cursor fault setup must adopt").await;
    let mut cursor = EventCursor {
        session_id: session_id.clone(),
        after_sequence: 0,
    };
    require_fault(fixture, point).await;
    assert_eq!(
        fixture.port().events(cursor.clone()).await,
        Err(PortError::Unavailable)
    );
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let regression = EventCursor {
        session_id,
        after_sequence: 1,
    };
    if point == CovenFaultPoint::CursorAfterPage {
        assert_eq!(
            fixture.port().events(regression).await,
            Err(PortError::IntentConflict)
        );
    } else {
        let probe = fixture
            .port()
            .events(regression.clone())
            .await
            .unwrap_or_else(|error| panic!("before-page fault persisted a cursor: {error}"));
        probe
            .validate_for(&regression)
            .unwrap_or_else(|error| panic!("before-page recovery probe must validate: {error}"));
        assert_cursor_page_progress(&regression, &probe, RAW_LEDGER_STATES.len() as u64);
        assert_eq!(
            probe
                .events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        fixture.reset().await;
        let reset_session_id = adopt_session(
            fixture,
            launch_request(),
            "cursor recovery setup must adopt",
        )
        .await;
        cursor.session_id = reset_session_id;
    }
    let recovered = fixture
        .port()
        .events(cursor.clone())
        .await
        .unwrap_or_else(|error| panic!("{point:?} cursor must recover: {error}"));
    recovered
        .validate_for(&cursor)
        .unwrap_or_else(|error| panic!("{point:?} recovered cursor page must validate: {error}"));
    assert_cursor_page_progress(&cursor, &recovered, RAW_LEDGER_STATES.len() as u64);
    fixture.restart().await;
    let replayed = fixture
        .port()
        .events(cursor.clone())
        .await
        .unwrap_or_else(|error| panic!("{point:?} cursor must replay: {error}"));
    replayed
        .validate_for(&cursor)
        .unwrap_or_else(|error| panic!("{point:?} replayed cursor page must validate: {error}"));
    assert_cursor_page_progress(&cursor, &replayed, RAW_LEDGER_STATES.len() as u64);
    assert_eq!(replayed, recovered);
}

async fn assert_termination_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let launch = launch_request();
    let session_id = adopt_session(
        fixture,
        launch.clone(),
        "termination fault setup must adopt",
    )
    .await;
    let requested = termination_requested_binding(&launch, &session_id, "operator_request");
    let mut persistence = MemoryTerminationPersistence::default();
    require_fault(fixture, point).await;
    assert!(matches!(
        persist_then_terminate(&mut persistence, fixture.port(), requested.clone(),).await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    fixture.restart().await;
    let recovered = if point == CovenFaultPoint::CancellationBeforeAcknowledgement {
        require_fault(fixture, CovenFaultPoint::CancellationBeforeAcknowledgement).await;
        assert!(matches!(
            persist_then_terminate(&mut persistence, fixture.port(), requested.clone()).await,
            Err(TerminationDispatchError::Port(PortError::Unavailable))
        ));
        require_clear_fault(fixture).await;
        persist_then_terminate(&mut persistence, fixture.port(), requested.clone())
            .await
            .unwrap_or_else(|error| panic!("{point:?} termination must recover: {error}"))
    } else {
        require_fault(fixture, CovenFaultPoint::CancellationBeforeAcknowledgement).await;
        let disposition =
            persist_then_terminate(&mut persistence, fixture.port(), requested.clone())
                .await
                .unwrap_or_else(|error| {
                    panic!("{point:?} lost its durable acknowledgement: {error}")
                });
        require_clear_fault(fixture).await;
        disposition
    };
    fixture.restart().await;
    assert_eq!(
        persist_then_terminate(&mut persistence, fixture.port(), requested)
            .await
            .unwrap_or_else(|error| panic!("{point:?} termination must replay: {error}")),
        recovered
    );
}

async fn assert_result_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let session_id =
        adopt_session(fixture, launch_request(), "result fault setup must adopt").await;
    require_fault(fixture, point).await;
    assert_eq!(
        fixture.port().result(&session_id).await,
        Err(PortError::Unavailable)
    );
    fixture.restart().await;
    require_fault(fixture, CovenFaultPoint::ResultBeforePersistence).await;
    let recovered = if point == CovenFaultPoint::ResultBeforePersistence {
        assert_eq!(
            fixture.port().result(&session_id).await,
            Err(PortError::Unavailable)
        );
        require_clear_fault(fixture).await;
        fixture
            .port()
            .result(&session_id)
            .await
            .unwrap_or_else(|error| panic!("{point:?} result must recover: {error}"))
    } else {
        let bundle = fixture
            .port()
            .result(&session_id)
            .await
            .unwrap_or_else(|error| panic!("artifact fault lost primary result: {error}"));
        require_clear_fault(fixture).await;
        bundle
    };
    fixture.restart().await;
    assert_eq!(
        fixture
            .port()
            .result(&session_id)
            .await
            .unwrap_or_else(|error| panic!("{point:?} result must replay: {error}")),
        recovered
    );
}

async fn assert_reconciliation_fault_recovery(
    fixture: &mut dyn CovenConformanceFixture,
    point: CovenFaultPoint,
) {
    fixture.reset().await;
    let correlation = mark_ambiguous(fixture).await;
    let original_adoption = lookup_durable_adoption(fixture, &correlation).await;
    let request = reconciliation_request(correlation, true);
    require_fault(fixture, point).await;
    let expected = if point == CovenFaultPoint::ReconcileStall {
        PortError::Stalled
    } else {
        PortError::Unavailable
    };
    assert_eq!(
        fixture.port().reconcile(request.clone()).await,
        Err(expected)
    );
    let committed_before_restart = fixture.observations().await.durable_reconciliation;
    if point == CovenFaultPoint::ReconcileAfterDisposition {
        assert!(committed_before_restart.is_some());
    } else {
        assert!(committed_before_restart.is_none());
    }
    fixture.restart().await;
    require_clear_fault(fixture).await;
    let recovered = fixture
        .port()
        .reconcile(request.clone())
        .await
        .unwrap_or_else(|error| panic!("{point:?} reconciliation must recover: {error}"));
    let durable = fixture
        .observations()
        .await
        .durable_reconciliation
        .unwrap_or_else(|| panic!("{point:?} reconciliation must become durable"));
    assert_eq!(disposition_observation(&recovered), Some(durable.clone()));
    fixture.restart().await;
    assert_eq!(
        fixture
            .port()
            .reconcile(request.clone())
            .await
            .unwrap_or_else(|error| panic!("{point:?} reconciliation must replay: {error}")),
        recovered
    );
    assert_eq!(
        fixture.observations().await.durable_reconciliation,
        Some(durable)
    );
    assert_eq!(
        lookup_durable_adoption(fixture, &request.correlation).await,
        original_adoption,
        "fault recovery must not alter the original adoption"
    );
    assert_eq!(fixture.observations().await.adoption_calls, 0);
}

/// Verifies stable typed denials for invalid input, policy, and authority loss.
pub async fn assert_c_s12_structured_denial(
    fixture: &mut dyn CovenConformanceFixture,
) -> ConformanceOutcome {
    if let Some(outcome) = expected_unsupported(
        fixture,
        CovenConformanceCase::C_S12,
        UnsupportedCall::Negotiate,
    )
    .await
    {
        return outcome;
    }
    fixture.reset().await;
    assert_eq!(
        fixture
            .port()
            .negotiate(NegotiateRequest::new("coven.daemon.v2"))
            .await,
        Err(PortError::ContractUnsupported {})
    );
    let mut capability = NegotiateRequest::new(CONTRACT);
    capability
        .required_capabilities
        .insert("future_capability".to_owned());
    assert_eq!(
        fixture.port().negotiate(capability).await,
        Err(PortError::CapabilityMissing {})
    );

    let launch = launch_request();
    let correlation = launch.correlation();
    let session_id = adopt_session(fixture, launch, "structured denial setup must adopt").await;
    let mut changed = correlation.clone();
    changed.project_id = "project:sha256:other".to_owned();
    assert_eq!(
        fixture
            .port()
            .reconcile(ReconciliationRequest {
                correlation: changed,
                ambiguity_digest: digest_of('d'),
                reason_code: "return_original".to_owned(),
            })
            .await,
        Err(PortError::IntentConflict)
    );
    assert!(
        fixture
            .observations()
            .await
            .durable_reconciliation
            .is_none()
    );

    let foreign_session_id = distinct_session_id(&session_id, "foreign-session");
    assert_eq!(
        fixture
            .port()
            .events(EventCursor {
                session_id: foreign_session_id,
                after_sequence: 0,
            })
            .await,
        Err(PortError::CorrelationMismatch)
    );
    assert_eq!(
        fixture
            .port()
            .events(EventCursor {
                session_id: session_id.clone(),
                after_sequence: MAX_SAFE_INTEGER + 1,
            })
            .await,
        Err(PortError::InvalidRequest)
    );

    let launch_request_id = correlation.request_id.clone();
    let launch_adoption = fixture
        .port()
        .lookup(&launch_request_id)
        .await
        .unwrap_or_else(|error| panic!("structured denial adoption lookup failed: {error}"));
    let policy_request = request_with_id(&launch_request(), "req_01J00000000000000000000013");
    let policy_request_id = policy_request.correlation().request_id;
    let policy_before = fixture
        .port()
        .lookup(&policy_request_id)
        .await
        .unwrap_or_else(|error| panic!("policy-denial lookup snapshot failed: {error}"));
    let before = fixture.observations().await;
    require_fault(fixture, CovenFaultPoint::AdoptionPolicyDenied).await;
    assert_eq!(
        fixture.port().adopt(policy_request).await,
        Err(PortError::PolicyDenied)
    );
    require_clear_fault(fixture).await;
    let after = fixture.observations().await;
    assert_eq!(after.adoption_calls, before.adoption_calls + 1);
    assert_eq!(after.reconciliation_calls, before.reconciliation_calls);
    assert_eq!(after.durable_reconciliation, before.durable_reconciliation);
    assert_eq!(
        fixture
            .port()
            .lookup(&policy_request_id)
            .await
            .unwrap_or_else(|error| panic!("policy-denial lookup failed: {error}")),
        policy_before
    );
    assert_eq!(
        fixture
            .port()
            .lookup(&launch_request_id)
            .await
            .unwrap_or_else(|error| panic!("adoption lookup after policy denial failed: {error}")),
        launch_adoption
    );

    let before = fixture.observations().await;
    require_fault(fixture, CovenFaultPoint::InspectAuthorityLost).await;
    assert_eq!(
        fixture.port().inspect(&session_id).await,
        Err(PortError::Unavailable)
    );
    require_clear_fault(fixture).await;
    assert_eq!(fixture.observations().await, before);
    let recovered = fixture
        .port()
        .inspect(&session_id)
        .await
        .unwrap_or_else(|error| panic!("inspection must recover after authority loss: {error}"));
    assert_eq!(recovered.session_id, session_id);
    assert_eq!(recovered.correlation, correlation);
    assert_eq!(
        fixture
            .port()
            .lookup(&launch_request_id)
            .await
            .unwrap_or_else(|error| panic!("adoption lookup after authority loss failed: {error}")),
        launch_adoption
    );

    fixture.reset().await;
    assert_eq!(
        fixture
            .port()
            .adopt(session_input_request(&session_id))
            .await,
        Err(PortError::NotFound)
    );
    assert_eq!(
        fixture.port().inspect("missing-session").await,
        Err(PortError::NotFound)
    );

    let correlation = launch_request().correlation();
    let base = ContentAddressedReference {
        digest: digest_of('a'),
        media_type: "application/json".to_owned(),
        size_bytes: 1,
        expires_at: correlation.created_at + time::Duration::minutes(1),
    };
    let mut zero = base.clone();
    zero.size_bytes = 0;
    assert_eq!(zero.validate(), Err(PortError::InvalidRequest));
    let mut oversized = base.clone();
    oversized.size_bytes = MAX_CONTENT_SIZE_BYTES + 1;
    assert_eq!(oversized.validate(), Err(PortError::InvalidRequest));
    let mut malformed = base;
    malformed.media_type = "free form error".to_owned();
    assert_eq!(malformed.validate(), Err(PortError::InvalidRequest));

    let structured = [
        PortError::ContractUnsupported {},
        PortError::CapabilityMissing {},
        PortError::IntentConflict,
        PortError::CorrelationMismatch,
        PortError::InvalidRequest,
        PortError::NotFound,
        PortError::PolicyDenied,
        PortError::Unavailable,
    ];
    for error in structured {
        assert!(!error.to_string().is_empty());
        assert!(!format!("{error:?}").contains("free form error"));
    }
    ConformanceOutcome::Verified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "cursor page did not advance before terminal high-water mark")]
    fn cursor_page_must_advance_before_terminal_high_water() {
        let cursor = EventCursor {
            session_id: "opaque-session".to_owned(),
            after_sequence: 3,
        };
        let page = EventPage {
            events: Vec::new(),
            next_cursor: cursor.clone(),
        };

        assert_cursor_page_progress(&cursor, &page, 7);
    }

    #[test]
    #[should_panic(expected = "cursor advanced beyond terminal high-water mark")]
    fn cursor_page_must_reject_terminal_high_water_overshoot() {
        let cursor = EventCursor {
            session_id: "opaque-session".to_owned(),
            after_sequence: 8,
        };
        let page = EventPage {
            events: Vec::new(),
            next_cursor: cursor.clone(),
        };

        assert_cursor_page_progress(&cursor, &page, 7);
    }

    #[test]
    #[should_panic(expected = "returned reconciliation session does not match durable adoption")]
    fn returned_reconciliation_must_name_original_session() {
        assert_returned_session_matches_adoption(
            "returned-session",
            &AdoptionDisposition::Adopted {
                session_id: "original-session".to_owned(),
            },
        );
    }
}
