#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, CancellationUnresolvedEvidence, ExecutionBinding,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::{CanonicalDocument, RecordKind, SchemaVersion};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, Capability, CapabilityProfile, CovenEvent, CovenPort,
    EventCursor, EventPage, ExecutionRequestInput, NegotiateRequest, PortError,
    ReconciliationDisposition, ReconciliationRequest, ResultBundle, SessionSnapshot,
    TerminationDispatchError, TerminationDisposition, TerminationPersistence,
    TerminationPersistenceFailure, derive_termination_outcome_revision, persist_then_terminate,
};
use psyche_store::{Store, StoreError};
use psyche_surfaces::{DeliveryDisposition, SurfaceAcceptance, SurfacePort};
use psyche_test_support::{
    CovenConformanceCase, CovenConformanceFixture, CovenConformanceObservations, CovenFaultPoint,
    CovenScriptReturn, CovenScriptStep, DurableDispositionKind, DurableDispositionObservation,
    FakeBuildError, FakeCoven, FakeOperation, FakeSurface, FixtureAvailability,
    FixtureControlError, StoreTerminationPersistence, SurfaceScriptReturn, SurfaceScriptStep,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const LAUNCH_GOLDEN: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json");
const INPUT_GOLDEN: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-input.json");
const RESULT_GOLDEN: &[u8] = include_bytes!("../../psyche-coven/tests/fixtures/result-bundle.json");

fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn digest_of(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn record_id(kind: RecordKind, suffix: &str) -> RecordId {
    RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())).unwrap()
}

fn session_bound() -> ExecutionBinding {
    ExecutionBinding {
        schema_version: SchemaVersion::parse("psyche.execution_binding.v1").unwrap(),
        attempt_id: record_id(RecordKind::Attempt, "01J00000000000000000000000"),
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: at("2026-08-05T14:00:00Z"),
        familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000000"),
        project_id: "project:sha256:abc".to_owned(),
        request_id: RequestId::parse("req_01J00000000000000000000000").unwrap(),
        request_digest: digest_of('a'),
        request_created_at: at("2026-08-05T13:59:00Z"),
        request_valid_until: at("2026-08-05T14:05:00Z"),
        coven_contract_version: "coven.daemon.v1".to_owned(),
        coven_session_id: Some("session-1".to_owned()),
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

fn requested_after(previous: &ExecutionBinding) -> ExecutionBinding {
    let mut requested = previous.clone();
    requested.revision += 1;
    requested.previous_revision_digest = Some(digest(previous).unwrap());
    requested.revision_created_at += time::Duration::minutes(1);
    requested.cancellation_state = CancellationState::TerminationRequested;
    requested.termination_request = Some(TerminationRequestCorrelation {
        termination_request_id: RequestId::parse("req_01J00000000000000000000001").unwrap(),
        created_at: at("2026-08-05T14:01:00Z"),
        valid_until: at("2026-08-05T14:03:00Z"),
    });
    requested.termination_reason_code = Some("operator_request".to_owned());
    requested
}

fn acknowledgement_for(requested: &ExecutionBinding) -> CancellationAcknowledgementEvidence {
    CancellationAcknowledgementEvidence {
        acknowledgement_id: "ack-1".to_owned(),
        termination_request_id: requested
            .termination_request
            .as_ref()
            .unwrap()
            .termination_request_id
            .clone(),
        session_id: requested.coven_session_id.clone().unwrap(),
        execution_request_id: requested.request_id.clone(),
        execution_request_digest: requested.request_digest.clone(),
        kind: CancellationAcknowledgementKind::Terminated,
        authority_evidence_digest: digest_of('d'),
        acknowledged_at: at("2026-08-05T14:02:00Z"),
    }
}

fn unresolved_for(requested: &ExecutionBinding) -> CancellationUnresolvedEvidence {
    CancellationUnresolvedEvidence {
        disposition_id: "unresolved-1".to_owned(),
        termination_request_id: requested
            .termination_request
            .as_ref()
            .unwrap()
            .termination_request_id
            .clone(),
        session_id: requested.coven_session_id.clone().unwrap(),
        execution_request_id: requested.request_id.clone(),
        execution_request_digest: requested.request_digest.clone(),
        reason_code: "timeout".to_owned(),
        recorded_at: at("2026-08-05T14:02:00Z"),
    }
}

fn create_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    (dir, path)
}

fn seed(path: &Path) -> ExecutionBinding {
    let initial = session_bound();
    let mut store = Store::open(path).unwrap();
    store
        .insert(&CanonicalDocument::ExecutionBinding(initial.clone()))
        .unwrap();
    requested_after(&initial)
}

fn persistence(path: &Path) -> StoreTerminationPersistence {
    StoreTerminationPersistence::new(Store::open(path).unwrap())
}

fn revisions(path: &Path, attempt_id: &RecordId) -> Vec<ExecutionBinding> {
    Store::open(path)
        .unwrap()
        .execution_binding_revisions(attempt_id)
        .unwrap()
}

fn surface_effect() -> psyche_core::contracts::surface::SurfaceEffect {
    serde_json::from_value(serde_json::json!({
        "schema_version":"psyche.surface_effect.v1",
        "surface_effect_id":"sfx_01J00000000000000000000000",
        "intent_id":"int_01J00000000000000000000000",
        "graph_id":"grf_01J00000000000000000000000",
        "node_id":"nod_01J00000000000000000000000",
        "attempt_id":"att_01J00000000000000000000000",
        "familiar_snapshot_id":"ids_01J00000000000000000000000",
        "project_id":"project:sha256:abc",
        "action_class":"send_message",
        "account_id":"account-1",
        "locator":{},
        "effect":{"text":"hello"},
        "effect_digest":"sha256:cbbbdcd27692344de5dbab3abcaba413fb0f45307267de7081401576df1cb176",
        "created_at":"2026-08-05T14:00:00Z"
    }))
    .unwrap()
}

fn surface_event() -> psyche_core::contracts::surface::SurfaceEvent {
    serde_json::from_value(serde_json::json!({
        "schema_version":"psyche.surface_event.v1",
        "surface_event_id":"sev_01J00000000000000000000000",
        "adapter_id":"telegram",
        "account_id":"account-1",
        "actor":{"type":"user","id":"123"},
        "locator":{"type":"message","chat_id":"123","message_id":"42"},
        "adapter_event_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "received_at":"2026-08-05T14:00:00Z",
        "content":{"type":"text","text":"hello"}
    }))
    .unwrap()
}

fn alternate_utc_spelling(bytes: Vec<u8>) -> Vec<u8> {
    let canonical = String::from_utf8(bytes).unwrap();
    canonical.replacen("Z\"", "+00:00\"", 1).into_bytes()
}

#[tokio::test]
async fn advertised_adoption_requires_a_scripted_adoption_step() {
    let fake = FakeCoven::builder()
        .capability(Capability::StableAdoption)
        .build();
    assert!(matches!(
        fake,
        Err(FakeBuildError::UnscriptedCapability { .. })
    ));
}

#[tokio::test]
async fn unknown_contract_fails_before_adoption() {
    let adoption = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap()).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".into(),
    };
    let fake = FakeCoven::builder()
        .contract("coven.daemon.v1")
        .adoption(disposition.clone())
        .build()
        .unwrap();
    let result = fake
        .negotiate(NegotiateRequest::new("coven.daemon.v2"))
        .await;
    assert!(matches!(result, Err(PortError::ContractUnsupported { .. })));
    assert_eq!(fake.adopt(adoption).await.unwrap(), disposition);
}

#[tokio::test]
async fn supported_negotiation_requires_and_consumes_a_matching_script_step() {
    let unscripted = FakeCoven::builder().build().unwrap();
    assert!(matches!(
        unscripted
            .negotiate(NegotiateRequest::new("coven.daemon.v1"))
            .await,
        Err(PortError::UnexpectedCall)
    ));
    let profile = CapabilityProfile {
        api_version: "coven.daemon.v1".to_owned(),
        capabilities: [Capability::StableAdoption.as_str().to_owned()]
            .into_iter()
            .collect(),
    };
    let fake = FakeCoven::builder()
        .capability(Capability::StableAdoption)
        .step(CovenScriptStep::Return(CovenScriptReturn::Negotiate(
            profile.clone(),
        )))
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .build()
        .unwrap();
    let request = NegotiateRequest::new("coven.daemon.v1").requiring(Capability::StableAdoption);
    let adoption = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap()).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };

    assert_eq!(fake.negotiate(request.clone()).await.unwrap(), profile);
    assert!(matches!(
        fake.negotiate(request).await,
        Err(PortError::UnexpectedCall)
    ));
    assert_eq!(fake.adopt(adoption).await.unwrap(), disposition);
}

#[tokio::test]
async fn explicit_script_steps_are_consumed_in_order() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .step(CovenScriptStep::Return(CovenScriptReturn::Negotiate(
            CapabilityProfile {
                api_version: "coven.daemon.v1".to_owned(),
                capabilities: std::collections::BTreeSet::new(),
            },
        )))
        .adoption(disposition.clone())
        .build()
        .unwrap();
    assert_eq!(
        fake.negotiate(NegotiateRequest::new("coven.daemon.v1"))
            .await
            .unwrap()
            .api_version,
        "coven.daemon.v1"
    );
    assert_eq!(fake.adopt(request).await.unwrap(), disposition);
}

#[tokio::test]
async fn reconcile_after_commit_replays_and_changed_correlation_conflicts() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let disposition = ReconciliationDisposition::Returned {
        disposition_id: "disposition-1".to_owned(),
        session_id: "session-1".to_owned(),
        correlation: correlation.clone(),
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: at("2026-08-05T14:02:00Z"),
    };
    let mut fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Reconcile(disposition.clone()),
        ))
        .build()
        .unwrap();
    let fixture: &mut dyn CovenConformanceFixture = &mut fake;
    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Err(PortError::Unavailable)
    ));
    fixture.restart().await;
    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Ok(ReconciliationDisposition::Returned { .. })
    ));
    let mut changed = request;
    changed.correlation.request_digest = digest_of('f');
    assert!(matches!(
        fixture.port().reconcile(changed).await,
        Err(PortError::IntentConflict)
    ));
    assert_eq!(fixture.observations().await.reconciliation_calls, 3);
}

#[test]
fn reconciliation_resolution_may_be_recorded_after_correlation_deadline() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let recorded_at = request.correlation.valid_until + time::Duration::nanoseconds(1);

    for disposition in [
        ReconciliationDisposition::Returned {
            disposition_id: "disposition-1".to_owned(),
            session_id: "session-1".to_owned(),
            correlation: correlation.clone(),
            ambiguity_digest: request.ambiguity_digest.clone(),
            recorded_at,
        },
        ReconciliationDisposition::Fenced {
            disposition_id: "disposition-2".to_owned(),
            fence_token: "fence-1".to_owned(),
            correlation: correlation.clone(),
            ambiguity_digest: request.ambiguity_digest.clone(),
            recorded_at,
        },
    ] {
        disposition.validate_for(&request).unwrap();
    }
}

#[test]
fn reconciliation_resolution_requires_exact_correlation_digest_utc_and_lower_bound() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let disposition =
        |correlation, ambiguity_digest, recorded_at| ReconciliationDisposition::Returned {
            disposition_id: "disposition-1".to_owned(),
            session_id: "session-1".to_owned(),
            correlation,
            ambiguity_digest,
            recorded_at,
        };

    let mut mismatched_correlation = correlation.clone();
    mismatched_correlation.project_id = "project:sha256:def".to_owned();
    let non_utc = correlation
        .created_at
        .to_offset(time::UtcOffset::from_hms(1, 0, 0).unwrap());
    for invalid in [
        disposition(
            mismatched_correlation,
            request.ambiguity_digest.clone(),
            correlation.valid_until,
        ),
        disposition(correlation.clone(), digest_of('f'), correlation.valid_until),
        disposition(
            correlation.clone(),
            request.ambiguity_digest.clone(),
            correlation.created_at - time::Duration::nanoseconds(1),
        ),
        disposition(correlation, request.ambiguity_digest.clone(), non_utc),
    ] {
        assert!(matches!(
            invalid.validate_for(&request),
            Err(PortError::CorrelationMismatch)
        ));
    }
}

#[tokio::test]
async fn unresolved_reconciliation_remains_retryable_until_returned_or_fenced() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let recorded_at = request.correlation.valid_until + time::Duration::nanoseconds(1);
    let resolutions = [
        ReconciliationDisposition::Returned {
            disposition_id: "disposition-1".to_owned(),
            session_id: "session-1".to_owned(),
            correlation: correlation.clone(),
            ambiguity_digest: request.ambiguity_digest.clone(),
            recorded_at,
        },
        ReconciliationDisposition::Fenced {
            disposition_id: "disposition-2".to_owned(),
            fence_token: "fence-1".to_owned(),
            correlation,
            ambiguity_digest: request.ambiguity_digest.clone(),
            recorded_at,
        },
    ];

    for resolution in resolutions {
        let fake = FakeCoven::builder()
            .reconciliation(ReconciliationDisposition::Unresolved)
            .reconciliation(resolution.clone())
            .build()
            .unwrap();
        let fixture: &dyn CovenConformanceFixture = &fake;

        assert_eq!(
            fixture.port().reconcile(request.clone()).await.unwrap(),
            ReconciliationDisposition::Unresolved
        );
        assert_eq!(
            fixture.port().reconcile(request.clone()).await.unwrap(),
            resolution
        );
        assert_eq!(fixture.observations().await.reconciliation_calls, 2);
    }
}

#[tokio::test]
async fn reconciliation_disconnect_before_and_stall_leave_ambiguity_retryable() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let returned = ReconciliationDisposition::Returned {
        disposition_id: "disposition-1".to_owned(),
        session_id: "session-1".to_owned(),
        correlation,
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: request.correlation.valid_until + time::Duration::nanoseconds(1),
    };
    let mut fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectBeforeCommit(
            FakeOperation::Reconcile,
        ))
        .step(CovenScriptStep::Stall(FakeOperation::Reconcile))
        .reconciliation(returned.clone())
        .build()
        .unwrap();
    let fixture: &mut dyn CovenConformanceFixture = &mut fake;

    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Err(PortError::Unavailable)
    ));
    fixture.restart().await;
    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Err(PortError::Stalled)
    ));
    assert_eq!(
        fixture.port().reconcile(request.clone()).await.unwrap(),
        returned
    );
    fixture.restart().await;
    assert_eq!(fixture.port().reconcile(request).await.unwrap(), returned);
    assert_eq!(fixture.observations().await.reconciliation_calls, 4);
}

#[tokio::test]
async fn fenced_reconciliation_survives_after_commit_disconnect_and_restart() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let fenced = ReconciliationDisposition::Fenced {
        disposition_id: "disposition-1".to_owned(),
        fence_token: "fence-1".to_owned(),
        correlation,
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: request.correlation.valid_until + time::Duration::nanoseconds(1),
    };
    let mut fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Reconcile(fenced.clone()),
        ))
        .build()
        .unwrap();
    let fixture: &mut dyn CovenConformanceFixture = &mut fake;

    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Err(PortError::Unavailable)
    ));
    fixture.restart().await;
    assert_eq!(
        fixture.port().reconcile(request.clone()).await.unwrap(),
        fenced
    );

    let mut changed = request;
    changed.ambiguity_digest = digest_of('f');
    assert!(matches!(
        fixture.port().reconcile(changed).await,
        Err(PortError::IntentConflict)
    ));
}

#[tokio::test]
async fn conformance_observations_match_through_concrete_and_trait_object_without_mutation() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let adoption = AdoptionRequest::new(input).unwrap();
    let correlation = adoption.correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let disposition = ReconciliationDisposition::Returned {
        disposition_id: "disposition-1".to_owned(),
        session_id: "session-1".to_owned(),
        correlation: correlation.clone(),
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: correlation.valid_until + time::Duration::nanoseconds(1),
    };
    let fake = FakeCoven::builder()
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .reconciliation(disposition)
        .build()
        .unwrap();

    fake.adopt(adoption).await.unwrap();
    fake.reconcile(request.clone()).await.unwrap();

    let concrete = fake.observations().await;
    let fixture: &dyn CovenConformanceFixture = &fake;
    let through_trait = fixture.observations().await;
    assert_eq!(concrete, through_trait);
    assert_eq!(fixture.observations().await, through_trait);
    assert_eq!(
        through_trait,
        CovenConformanceObservations {
            adoption_calls: 1,
            reconciliation_calls: 1,
            durable_reconciliation: Some(DurableDispositionObservation {
                disposition_id: "disposition-1".to_owned(),
                correlation,
                ambiguity_digest: request.ambiguity_digest,
                kind: DurableDispositionKind::Returned {
                    session_id: "session-1".to_owned(),
                },
                recorded_at: request.correlation.valid_until + time::Duration::nanoseconds(1),
            }),
        }
    );
}

#[tokio::test]
async fn conformance_observations_are_redacted_and_follow_restart_reset_semantics() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation: correlation.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let disposition = ReconciliationDisposition::Fenced {
        disposition_id: "disposition-1".to_owned(),
        fence_token: "fence-1".to_owned(),
        correlation,
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: request.correlation.valid_until + time::Duration::nanoseconds(1),
    };
    let mut fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Reconcile(disposition),
        ))
        .build()
        .unwrap();

    assert!(matches!(
        fake.reconcile(request).await,
        Err(PortError::Unavailable)
    ));
    let before_restart = fake.observations().await;
    CovenConformanceFixture::restart(&mut fake).await;
    assert_eq!(fake.observations().await, before_restart);

    let redacted = format!("{before_restart:?}");
    for raw_field in ["principal_id", "project_root", "cwd", "payload_digest"] {
        assert!(!redacted.contains(raw_field), "{raw_field}");
    }

    CovenConformanceFixture::reset(&mut fake).await;
    assert_eq!(
        fake.observations().await,
        CovenConformanceObservations::default()
    );
}

#[tokio::test]
async fn observations_follow_reconciliation_commit_order_across_restart() {
    let mut high_input: serde_json::Value = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    high_input["request_id"] = serde_json::json!("req_01J00000000000000000000001");
    let high = AdoptionRequest::new(serde_json::from_value(high_input).unwrap())
        .unwrap()
        .correlation();
    let low = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap())
        .unwrap()
        .correlation();
    let high_request = ReconciliationRequest {
        correlation: high.clone(),
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let low_request = ReconciliationRequest {
        correlation: low.clone(),
        ambiguity_digest: digest_of('f'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let high_disposition = ReconciliationDisposition::Returned {
        disposition_id: "committed-first".to_owned(),
        session_id: "session-high".to_owned(),
        correlation: high,
        ambiguity_digest: high_request.ambiguity_digest.clone(),
        recorded_at: high_request.correlation.valid_until,
    };
    let low_disposition = ReconciliationDisposition::Returned {
        disposition_id: "committed-last".to_owned(),
        session_id: "session-low".to_owned(),
        correlation: low,
        ambiguity_digest: low_request.ambiguity_digest.clone(),
        recorded_at: low_request.correlation.valid_until,
    };
    let mut fake = FakeCoven::builder()
        .reconciliation(high_disposition)
        .reconciliation(low_disposition)
        .build()
        .unwrap();

    fake.reconcile(high_request).await.unwrap();
    fake.reconcile(low_request).await.unwrap();
    assert_eq!(
        fake.observations()
            .await
            .durable_reconciliation
            .unwrap()
            .disposition_id,
        "committed-last"
    );
    CovenConformanceFixture::restart(&mut fake).await;
    assert_eq!(
        fake.observations()
            .await
            .durable_reconciliation
            .unwrap()
            .disposition_id,
        "committed-last"
    );
    CovenConformanceFixture::reset(&mut fake).await;
    assert!(fake.observations().await.durable_reconciliation.is_none());
}

#[tokio::test]
async fn conformance_fault_controls_are_object_safe_and_resettable() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation,
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let mut fake = FakeCoven::builder()
        .reconciliation(ReconciliationDisposition::Unresolved)
        .build()
        .unwrap();
    let fixture: &mut dyn CovenConformanceFixture = &mut fake;

    fixture
        .select_fault(CovenFaultPoint::ReconcileStall)
        .await
        .unwrap();
    assert!(matches!(
        fixture.port().reconcile(request.clone()).await,
        Err(PortError::Stalled)
    ));
    assert_eq!(fixture.observations().await.reconciliation_calls, 1);

    fixture.clear_fault().await;
    assert_eq!(
        fixture.port().reconcile(request).await.unwrap(),
        ReconciliationDisposition::Unresolved
    );
    assert_eq!(fixture.observations().await.reconciliation_calls, 2);

    fixture.reset().await;
    fixture.restart().await;
    assert_eq!(
        fixture.observations().await,
        CovenConformanceObservations::default()
    );
}

#[tokio::test]
async fn conformance_fixture_truthfully_reports_cases_and_fault_support() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let request = ReconciliationRequest {
        correlation,
        ambiguity_digest: digest_of('e'),
        reason_code: "adoption_unknown".to_owned(),
    };
    let mut fake = FakeCoven::builder()
        .reconciliation(ReconciliationDisposition::Unresolved)
        .build()
        .unwrap();
    let fixture: &mut dyn CovenConformanceFixture = &mut fake;

    for case in [
        CovenConformanceCase::C_S1,
        CovenConformanceCase::C_S2,
        CovenConformanceCase::C_S3,
        CovenConformanceCase::C_S4,
        CovenConformanceCase::C_S5,
        CovenConformanceCase::C_S6,
        CovenConformanceCase::C_S7,
        CovenConformanceCase::C_S8,
        CovenConformanceCase::C_S9,
        CovenConformanceCase::C_S10,
        CovenConformanceCase::C_S11,
        CovenConformanceCase::C_S12,
    ] {
        assert!(matches!(
            fixture.availability(case),
            FixtureAvailability::ExpectedUnsupported { .. }
        ));
    }

    let supported = [
        CovenFaultPoint::ReconcileBeforeDisposition,
        CovenFaultPoint::ReconcileAfterDisposition,
        CovenFaultPoint::ReconcileStall,
    ];
    let unsupported = [
        CovenFaultPoint::AdoptionBeforeCommit,
        CovenFaultPoint::AdoptionAfterCommit,
        CovenFaultPoint::InputBeforeCommit,
        CovenFaultPoint::InputAfterCommit,
        CovenFaultPoint::LookupBeforeRead,
        CovenFaultPoint::LookupAfterRead,
        CovenFaultPoint::CursorBeforePage,
        CovenFaultPoint::CursorAfterPage,
        CovenFaultPoint::CancellationBeforeAcknowledgement,
        CovenFaultPoint::CancellationAfterAcknowledgement,
        CovenFaultPoint::TerminalBeforePersistence,
        CovenFaultPoint::ResultBeforePersistence,
        CovenFaultPoint::ArtifactBeforePersistence,
    ];
    for point in supported {
        assert!(fixture.supports(point), "{point:?}");
    }
    for point in unsupported {
        assert!(!fixture.supports(point), "{point:?}");
    }

    assert_eq!(
        fixture
            .select_fault(CovenFaultPoint::AdoptionBeforeCommit)
            .await,
        Err(FixtureControlError::UnsupportedFault)
    );
    assert_eq!(
        fixture.port().reconcile(request).await.unwrap(),
        ReconciliationDisposition::Unresolved
    );
}

#[tokio::test]
async fn before_commit_error_and_stall_never_advertise_success() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectBeforeCommit(
            FakeOperation::Adopt,
        ))
        .step(CovenScriptStep::Stall(FakeOperation::Adopt))
        .adoption(disposition.clone())
        .build()
        .unwrap();
    assert!(matches!(
        fake.adopt(request.clone()).await,
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        fake.adopt(request.clone()).await,
        Err(PortError::Stalled)
    ));
    assert_eq!(fake.adopt(request).await.unwrap(), disposition);
}

#[tokio::test]
async fn result_references_survive_after_commit_disconnect_and_restart() {
    let bundle: ResultBundle = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    let fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Result(bundle.clone()),
        ))
        .build()
        .unwrap();
    assert!(matches!(
        fake.result("session-1").await,
        Err(PortError::Unavailable)
    ));
    let restarted = fake.restart();
    assert_eq!(restarted.result("session-1").await.unwrap(), bundle);
}

#[tokio::test]
async fn invalid_scripted_results_are_response_errors() {
    let mut bundle: ResultBundle = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    bundle.result.media_type = "Application/JSON".to_owned();
    let fake = FakeCoven::builder().result(bundle).build().unwrap();

    assert!(matches!(
        fake.result("session-1").await,
        Err(PortError::InvalidResponse)
    ));

    let fake = FakeCoven::builder()
        .step(CovenScriptStep::Return(CovenScriptReturn::Negotiate(
            CapabilityProfile {
                api_version: String::new(),
                capabilities: std::collections::BTreeSet::new(),
            },
        )))
        .build()
        .unwrap();
    assert!(matches!(
        fake.negotiate(NegotiateRequest::new("coven.daemon.v1"))
            .await,
        Err(PortError::InvalidResponse)
    ));
}

#[tokio::test]
async fn result_references_must_echo_the_adopted_correlation() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    for (field, replacement) in [
        (
            "/request_id",
            serde_json::json!("req_01J00000000000000000000001"),
        ),
        (
            "/request_digest",
            serde_json::json!(format!("sha256:{}", "d".repeat(64))),
        ),
        (
            "/familiar_snapshot_id",
            serde_json::json!("ids_01J00000000000000000000001"),
        ),
        ("/project_id", serde_json::json!("project:sha256:def")),
        (
            "/graph_id",
            serde_json::json!("grf_01J00000000000000000000001"),
        ),
        (
            "/node_id",
            serde_json::json!("nod_01J00000000000000000000001"),
        ),
        (
            "/attempt_id",
            serde_json::json!("att_01J00000000000000000000001"),
        ),
        ("/created_at", serde_json::json!("2026-08-05T13:59:59Z")),
        ("/valid_until", serde_json::json!("2026-08-05T14:04:30Z")),
    ] {
        let mut mismatched: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
        *mismatched
            .pointer_mut(&format!("/correlation{field}"))
            .unwrap() = replacement.clone();
        *mismatched
            .pointer_mut(&format!("/artifacts/0/correlation{field}"))
            .unwrap() = replacement;
        let fake = FakeCoven::builder()
            .adoption(AdoptionDisposition::Adopted {
                session_id: "session-1".to_owned(),
            })
            .result(serde_json::from_value(mismatched).unwrap())
            .build()
            .unwrap();

        fake.adopt(request.clone()).await.unwrap();
        assert!(
            matches!(
                fake.result("session-1").await,
                Err(PortError::CorrelationMismatch)
            ),
            "{field}"
        );
    }
}

#[tokio::test]
async fn result_references_reject_an_unadopted_session() {
    let request = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap()).unwrap();
    let mut mismatched: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    mismatched["session_id"] = serde_json::json!("session-2");
    mismatched["artifacts"][0]["session_id"] = serde_json::json!("session-2");
    let fake = FakeCoven::builder()
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .result(serde_json::from_value(mismatched).unwrap())
        .build()
        .unwrap();

    fake.adopt(request).await.unwrap();
    assert!(matches!(
        fake.result("session-2").await,
        Err(PortError::CorrelationMismatch)
    ));
}

#[tokio::test]
async fn session_snapshot_must_echo_the_adopted_correlation() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let mut correlations = Vec::new();
    let mut correlation = request.correlation();
    correlation.request_digest = digest_of('d');
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.familiar_snapshot_id =
        record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000001");
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.project_id = "project:sha256:def".to_owned();
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.graph_id = record_id(RecordKind::Graph, "01J00000000000000000000001");
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.node_id = record_id(RecordKind::GraphNode, "01J00000000000000000000001");
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.attempt_id = record_id(RecordKind::Attempt, "01J00000000000000000000001");
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.created_at += time::Duration::seconds(1);
    correlations.push(correlation);
    let mut correlation = request.correlation();
    correlation.valid_until -= time::Duration::seconds(1);
    correlations.push(correlation);

    for mismatched in correlations {
        let fake = FakeCoven::builder()
            .adoption(AdoptionDisposition::Adopted {
                session_id: "session-1".to_owned(),
            })
            .snapshot(SessionSnapshot {
                session_id: "session-1".to_owned(),
                correlation: mismatched,
                terminal_state: None,
            })
            .build()
            .unwrap();
        fake.adopt(request.clone()).await.unwrap();
        assert!(matches!(
            fake.inspect("session-1").await,
            Err(PortError::CorrelationMismatch)
        ));
    }

    let fake = FakeCoven::builder()
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .snapshot(SessionSnapshot {
            session_id: "session-2".to_owned(),
            correlation: request.correlation(),
            terminal_state: None,
        })
        .build()
        .unwrap();
    fake.adopt(request).await.unwrap();
    assert!(matches!(
        fake.inspect("session-2").await,
        Err(PortError::CorrelationMismatch)
    ));
}

#[tokio::test]
async fn surface_after_commit_replays_the_durable_disposition() {
    let committed = DeliveryDisposition::Applied {
        external_id: "delivery-1".to_owned(),
    };
    let fake = FakeSurface::builder()
        .step(SurfaceScriptStep::DisconnectAfterCommit(
            SurfaceScriptReturn::Apply(committed.clone()),
        ))
        .delivery(DeliveryDisposition::Applied {
            external_id: "delivery-2".to_owned(),
        })
        .build()
        .unwrap();
    let effect = surface_effect();

    assert!(matches!(
        fake.apply(effect.clone()).await,
        Err(psyche_surfaces::PortError::Unavailable)
    ));
    assert_eq!(fake.restart().apply(effect).await.unwrap(), committed);
}

#[tokio::test]
async fn successful_surface_acceptance_is_durable_and_conflict_safe() {
    let event = surface_event();
    let committed = SurfaceAcceptance {
        surface_event_id: event.surface_event_id.clone(),
        accepted: true,
    };
    let fake = FakeSurface::builder()
        .acceptance(committed.clone())
        .acceptance(SurfaceAcceptance {
            surface_event_id: event.surface_event_id.clone(),
            accepted: false,
        })
        .build()
        .unwrap();

    assert_eq!(fake.accept(event.clone()).await.unwrap(), committed);
    assert_eq!(
        fake.restart().accept(event.clone()).await.unwrap(),
        committed
    );

    let mut changed = event;
    changed.content["text"] = serde_json::json!("different");
    assert!(matches!(
        fake.accept(changed).await,
        Err(psyche_surfaces::PortError::IntentConflict)
    ));
}

#[tokio::test]
async fn successful_surface_delivery_is_durable_and_conflict_safe() {
    let committed = DeliveryDisposition::Applied {
        external_id: "delivery-1".to_owned(),
    };
    let fake = FakeSurface::builder()
        .delivery(committed.clone())
        .delivery(DeliveryDisposition::Rejected {
            code: "policy_denied".to_owned(),
        })
        .build()
        .unwrap();
    let effect = surface_effect();

    assert_eq!(fake.apply(effect.clone()).await.unwrap(), committed);
    assert_eq!(
        fake.restart().apply(effect.clone()).await.unwrap(),
        committed
    );

    let mut changed = effect;
    changed.project_id = "project:sha256:def".to_owned();
    assert!(matches!(
        fake.apply(changed).await,
        Err(psyche_surfaces::PortError::IntentConflict)
    ));
}

#[tokio::test]
async fn raw_session_statuses_never_become_termination_acknowledgement() {
    for status in [
        "created",
        "running",
        "idle",
        "completed",
        "failed",
        "killed",
        "orphaned",
    ] {
        let (_dir, path) = create_store();
        let requested = seed(&path);
        let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
        let correlation = AdoptionRequest::new(input).unwrap().correlation();
        let fake = FakeCoven::builder()
            .snapshot(SessionSnapshot {
                session_id: "session-1".to_owned(),
                correlation,
                terminal_state: Some(status.to_owned()),
            })
            .event_page(EventPage {
                events: vec![CovenEvent {
                    sequence: 1,
                    event_digest: digest_of('e'),
                    terminal_state: Some(status.to_owned()),
                }],
                next_cursor: EventCursor {
                    session_id: "session-1".to_owned(),
                    after_sequence: 1,
                },
            })
            .build()
            .unwrap();
        fake.inspect("session-1").await.unwrap();
        fake.events(EventCursor {
            session_id: "session-1".to_owned(),
            after_sequence: 0,
        })
        .await
        .unwrap();
        assert!(matches!(
            persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await,
            Err(TerminationDispatchError::Port(PortError::UnexpectedCall))
        ));
        assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
    }
}

#[tokio::test]
async fn killed_then_orphaned_statuses_do_not_create_termination_evidence() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let correlation = AdoptionRequest::new(input).unwrap().correlation();
    let snapshot = |status: &str| SessionSnapshot {
        session_id: "session-1".to_owned(),
        correlation: correlation.clone(),
        terminal_state: Some(status.to_owned()),
    };
    let page = |status: &str, sequence| EventPage {
        events: vec![CovenEvent {
            sequence,
            event_digest: digest_of('e'),
            terminal_state: Some(status.to_owned()),
        }],
        next_cursor: EventCursor {
            session_id: "session-1".to_owned(),
            after_sequence: sequence,
        },
    };
    let fake = FakeCoven::builder()
        .snapshot(snapshot("killed"))
        .event_page(page("killed", 1))
        .snapshot(snapshot("orphaned"))
        .event_page(page("orphaned", 2))
        .build()
        .unwrap();

    assert_eq!(
        fake.inspect("session-1").await.unwrap().terminal_state,
        Some("killed".to_owned())
    );
    fake.events(EventCursor {
        session_id: "session-1".to_owned(),
        after_sequence: 0,
    })
    .await
    .unwrap();

    let restarted = fake.restart();
    assert_eq!(
        restarted.inspect("session-1").await.unwrap().terminal_state,
        Some("orphaned".to_owned())
    );
    restarted
        .events(EventCursor {
            session_id: "session-1".to_owned(),
            after_sequence: 1,
        })
        .await
        .unwrap();
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &restarted, requested.clone()).await,
        Err(TerminationDispatchError::Port(PortError::UnexpectedCall))
    ));

    let stored = revisions(&path, &requested.attempt_id);
    assert_eq!(stored.len(), 2);
    assert_eq!(
        stored.last().unwrap().cancellation_state,
        CancellationState::TerminationRequested
    );
    assert!(
        stored
            .last()
            .unwrap()
            .cancellation_acknowledgement
            .is_none()
    );
    assert!(stored.last().unwrap().cancellation_unresolved.is_none());
}

#[tokio::test]
async fn changed_request_with_retained_digest_fails_before_adoption() {
    for golden in [LAUNCH_GOLDEN, INPUT_GOLDEN] {
        let input: ExecutionRequestInput = serde_json::from_slice(golden).unwrap();
        let request = AdoptionRequest::new(input).unwrap();
        let envelope = serde_json::to_value(request).unwrap();
        let base = envelope["input"].clone();
        let retained_digest = envelope["request_digest"].clone();
        let mut mutations = Vec::new();
        let changes = [
            (
                "/schema_version",
                serde_json::json!("psyche.execution_request.v2"),
            ),
            (
                "/request_id",
                serde_json::json!("req_01J00000000000000000000001"),
            ),
            (
                "/graph_id",
                serde_json::json!("grf_01J00000000000000000000001"),
            ),
            (
                "/node_id",
                serde_json::json!("nod_01J00000000000000000000001"),
            ),
            (
                "/attempt_id",
                serde_json::json!("att_01J00000000000000000000001"),
            ),
            ("/principal_id", serde_json::json!("principal:other")),
            (
                "/familiar_snapshot_id",
                serde_json::json!("ids_01J00000000000000000000001"),
            ),
            ("/project_id", serde_json::json!("project:sha256:def")),
            (
                "/context_manifest_digest",
                serde_json::json!(format!("sha256:{}", "7".repeat(64))),
            ),
            (
                "/payload_digest",
                serde_json::json!(format!("sha256:{}", "8".repeat(64))),
            ),
            ("/created_at", serde_json::json!("2026-08-05T14:00:01Z")),
            ("/valid_until", serde_json::json!("2026-08-05T14:07:00Z")),
        ];
        for (pointer, replacement) in changes {
            let mut changed = base.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            mutations.push(changed);
        }
        if base["operation"] == "launch" {
            for (pointer, replacement) in [
                ("/project_root", serde_json::json!("/workspace")),
                ("/cwd", serde_json::json!("/workspace/project/subdir")),
                ("/harness", serde_json::json!("future-harness")),
                (
                    "/delegation_digest",
                    serde_json::json!(format!("sha256:{}", "9".repeat(64))),
                ),
                (
                    "/budget_digest",
                    serde_json::json!(format!("sha256:{}", "a".repeat(64))),
                ),
                (
                    "/required_artifact_bindings/0/artifact_id",
                    serde_json::json!("artifact-2"),
                ),
                (
                    "/required_artifact_bindings/0/digest",
                    serde_json::json!(format!("sha256:{}", "b".repeat(64))),
                ),
                (
                    "/required_artifact_bindings/0/media_type",
                    serde_json::json!("application/json"),
                ),
                ("/required_artifact_bindings/0/size", serde_json::json!(13)),
            ] {
                let mut changed = base.clone();
                *changed.pointer_mut(pointer).unwrap() = replacement;
                mutations.push(changed);
            }
            let mut changed = base.clone();
            changed["required_artifact_bindings"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "artifact_id":"artifact-2",
                    "digest":format!("sha256:{}", "c".repeat(64)),
                    "media_type":"text/plain",
                    "size":1
                }));
            mutations.push(changed.clone());
            changed["required_artifact_bindings"]
                .as_array_mut()
                .unwrap()
                .reverse();
            mutations.push(changed);
        } else {
            for (pointer, replacement) in [
                ("/session_id", serde_json::json!("session-2")),
                (
                    "/input_digest",
                    serde_json::json!(format!("sha256:{}", "9".repeat(64))),
                ),
            ] {
                let mut changed = base.clone();
                *changed.pointer_mut(pointer).unwrap() = replacement;
                mutations.push(changed);
            }
            let mut changed = base.clone();
            changed["required_artifact_bindings"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "artifact_id":"artifact-1",
                    "digest":format!("sha256:{}", "c".repeat(64)),
                    "media_type":"text/plain",
                    "size":1
                }));
            mutations.push(changed);
        }

        for mutated_input in mutations {
            if let Ok(input) =
                serde_json::from_value::<ExecutionRequestInput>(mutated_input.clone())
            {
                if let Ok(rebuilt) = AdoptionRequest::new(input) {
                    assert_ne!(
                        serde_json::to_value(rebuilt.request_digest()).unwrap(),
                        retained_digest
                    );
                }
            }
            let forged: AdoptionRequest = serde_json::from_value(serde_json::json!({
                "input": mutated_input,
                "request_digest": retained_digest
            }))
            .unwrap();
            let fake = FakeCoven::builder()
                .adoption(AdoptionDisposition::Adopted {
                    session_id: "session-1".to_owned(),
                })
                .build()
                .unwrap();
            assert!(matches!(
                fake.adopt(forged).await,
                Err(PortError::RequestDigestMismatch)
            ));
            let fixture: &dyn CovenConformanceFixture = &fake;
            assert_eq!(fixture.observations().await.adoption_calls, 0);
        }
    }
}

#[tokio::test]
async fn input_request_digest_binds_every_artifact_field_order_and_content() {
    let mut base: serde_json::Value = serde_json::from_slice(INPUT_GOLDEN).unwrap();
    base["required_artifact_bindings"] = serde_json::json!([
        {
            "artifact_id":"artifact-1",
            "digest":format!("sha256:{}", "c".repeat(64)),
            "media_type":"text/plain",
            "size":1
        },
        {
            "artifact_id":"artifact-2",
            "digest":format!("sha256:{}", "d".repeat(64)),
            "media_type":"application/json",
            "size":2
        }
    ]);
    let baseline = AdoptionRequest::new(serde_json::from_value(base.clone()).unwrap()).unwrap();
    let retained_digest = serde_json::to_value(baseline.request_digest()).unwrap();
    let mut mutations = Vec::new();

    for (name, pointer, replacement) in [
        (
            "artifact_id",
            "/required_artifact_bindings/0/artifact_id",
            serde_json::json!("artifact-3"),
        ),
        (
            "digest",
            "/required_artifact_bindings/0/digest",
            serde_json::json!(format!("sha256:{}", "e".repeat(64))),
        ),
        (
            "media_type",
            "/required_artifact_bindings/0/media_type",
            serde_json::json!("application/octet-stream"),
        ),
        (
            "size",
            "/required_artifact_bindings/0/size",
            serde_json::json!(3),
        ),
    ] {
        let mut changed = base.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        mutations.push((name, changed));
    }

    let mut reordered = base.clone();
    reordered["required_artifact_bindings"]
        .as_array_mut()
        .unwrap()
        .reverse();
    mutations.push(("order", reordered));

    let mut removed = base.clone();
    removed["required_artifact_bindings"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    mutations.push(("removed_content", removed));

    let mut added = base;
    added["required_artifact_bindings"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "artifact_id":"artifact-3",
            "digest":format!("sha256:{}", "f".repeat(64)),
            "media_type":"text/plain",
            "size":3
        }));
    mutations.push(("added_content", added));

    for (name, mutated_input) in mutations {
        let rebuilt =
            AdoptionRequest::new(serde_json::from_value(mutated_input.clone()).unwrap()).unwrap();
        assert_ne!(
            serde_json::to_value(rebuilt.request_digest()).unwrap(),
            retained_digest,
            "{name}"
        );

        let forged: AdoptionRequest = serde_json::from_value(serde_json::json!({
            "input": mutated_input,
            "request_digest": retained_digest
        }))
        .unwrap();
        let fake = FakeCoven::builder()
            .adoption(AdoptionDisposition::Adopted {
                session_id: "session-1".to_owned(),
            })
            .build()
            .unwrap();
        assert!(
            matches!(
                fake.adopt(forged).await,
                Err(PortError::RequestDigestMismatch)
            ),
            "{name}"
        );
        let fixture: &dyn CovenConformanceFixture = &fake;
        assert_eq!(fixture.observations().await.adoption_calls, 0, "{name}");
    }
}

#[tokio::test]
async fn stable_adoption_replay_survives_fake_restart_and_rejects_changed_intent() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    changed["cwd"] = serde_json::json!("/workspace/project/subdir");
    let changed = AdoptionRequest::new(serde_json::from_value(changed).unwrap()).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .adoption(disposition.clone())
        .build()
        .unwrap();
    assert_eq!(fake.adopt(request.clone()).await.unwrap(), disposition);
    let restarted = fake.restart();
    assert_eq!(restarted.adopt(request).await.unwrap(), disposition);
    assert!(matches!(
        restarted.adopt(changed).await,
        Err(PortError::IntentConflict)
    ));
}

#[tokio::test]
async fn expired_new_adoption_fails_before_calls_but_durable_replay_survives() {
    let request = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap()).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .current_time(at("2026-08-05T14:04:00Z"))
        .adoption(disposition.clone())
        .build()
        .unwrap();
    assert_eq!(fake.adopt(request.clone()).await.unwrap(), disposition);
    assert_eq!(
        fake.at_time(at("2026-08-05T14:05:00.000000001Z"))
            .adopt(request.clone())
            .await
            .unwrap(),
        disposition
    );

    let expired = FakeCoven::builder()
        .current_time(at("2026-08-05T14:05:00.000000001Z"))
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .build()
        .unwrap();
    assert!(matches!(
        expired.adopt(request.clone()).await,
        Err(PortError::InvalidRequest)
    ));
    let fixture: &dyn CovenConformanceFixture = &expired;
    assert_eq!(fixture.observations().await.adoption_calls, 0);
    assert!(
        expired
            .at_time(at("2026-08-05T14:04:00Z"))
            .adopt(request)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn distinct_requests_may_share_an_adopted_session() {
    let launch = AdoptionRequest::new(serde_json::from_slice(LAUNCH_GOLDEN).unwrap()).unwrap();
    let mut input: serde_json::Value = serde_json::from_slice(INPUT_GOLDEN).unwrap();
    input["request_id"] = serde_json::json!("req_01J00000000000000000000001");
    let input = AdoptionRequest::new(serde_json::from_value(input).unwrap()).unwrap();
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .adoption(disposition.clone())
        .adoption(disposition.clone())
        .build()
        .unwrap();

    assert_eq!(fake.adopt(launch).await.unwrap(), disposition);
    assert_eq!(fake.adopt(input).await.unwrap(), disposition);
}

#[tokio::test]
async fn termination_disconnect_before_and_stall_leave_request_retryable() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let disposition = TerminationDisposition::Acknowledged {
        evidence: acknowledgement_for(&requested),
    };
    let fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectBeforeCommit(
            FakeOperation::Terminate,
        ))
        .step(CovenScriptStep::Stall(FakeOperation::Terminate))
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();

    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await,
        Err(TerminationDispatchError::Port(PortError::Stalled))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
    assert_eq!(
        persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
            .await
            .unwrap(),
        disposition
    );
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_after_commit_disconnect_replays_durable_acknowledgement() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let disposition = TerminationDisposition::Acknowledged {
        evidence: acknowledgement_for(&requested),
    };
    let fake = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Terminate(disposition.clone()),
        ))
        .build()
        .unwrap();

    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await,
        Err(TerminationDispatchError::Port(PortError::Unavailable))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);

    assert_eq!(
        persist_then_terminate(&mut persistence(&path), &fake.restart(), requested.clone(),)
            .await
            .unwrap(),
        disposition
    );
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_identical_termination_calls_share_one_scripted_commit() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let disposition = TerminationDisposition::Acknowledged {
        evidence: acknowledgement_for(&requested),
    };
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let first = Arc::new(AtomicBool::new(true));
    let fake = FakeCoven::builder()
        .before_terminate({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let first = Arc::clone(&first);
            Arc::new(move |_| {
                if first.swap(false, Ordering::SeqCst) {
                    entered.wait();
                    release.wait();
                }
                Ok(())
            })
        })
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();

    let first_call = {
        let fake = fake.clone();
        let path = path.clone();
        let requested = requested.clone();
        tokio::spawn(async move {
            persist_then_terminate(&mut persistence(&path), &fake, requested).await
        })
    };
    entered.wait();
    let mut second_call = {
        let fake = fake.clone();
        let path = path.clone();
        let requested = requested.clone();
        tokio::spawn(async move {
            persist_then_terminate(&mut persistence(&path), &fake, requested).await
        })
    };

    let early_second = tokio::time::timeout(Duration::from_millis(100), &mut second_call).await;
    release.wait();
    assert!(
        early_second.is_err(),
        "same-key caller bypassed the in-flight durable commit"
    );
    assert_eq!(first_call.await.unwrap().unwrap(), disposition);
    assert_eq!(second_call.await.unwrap().unwrap(), disposition);
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_dispatch_requires_durable_session_bound_revision() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let check_path = path.clone();
    let attempt = requested.attempt_id.clone();
    let expected_requested = canonical_bytes(&requested).unwrap();
    let fake = FakeCoven::builder()
        .before_terminate(Arc::new(move |_| {
            let persisted = revisions(&check_path, &attempt);
            if persisted.len() == 2
                && canonical_bytes(&persisted[1]).ok().as_ref() == Some(&expected_requested)
            {
                Ok(())
            } else {
                Err(PortError::Unavailable)
            }
        }))
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut store = persistence(&path);
    let dyn_port: &dyn CovenPort = &fake;
    persist_then_terminate(&mut store, dyn_port, requested.clone())
        .await
        .unwrap();
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);

    let (_dir, path) = create_store();
    let initial = session_bound();
    let mut unbound = initial.clone();
    unbound.coven_session_id = None;
    Store::open(&path)
        .unwrap()
        .insert(&CanonicalDocument::ExecutionBinding(unbound.clone()))
        .unwrap();
    let requested = requested_after(&initial);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let result = persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await;
    assert!(matches!(
        result,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 1);

    let (_dir, path) = create_store();
    let mut unbound = session_bound();
    unbound.coven_session_id = None;
    let mut requested = requested_after(&unbound);
    requested.coven_session_id = Some("session-1".to_owned());
    let mut raw_store = Store::open(&path).unwrap();
    raw_store
        .insert(&CanonicalDocument::ExecutionBinding(unbound))
        .unwrap();
    raw_store
        .insert(&CanonicalDocument::ExecutionBinding(requested.clone()))
        .unwrap();
    drop(raw_store);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);

    let (_dir, path) = create_store();
    let mut changed_session = seed(&path);
    changed_session.coven_session_id = Some("session-2".to_owned());
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&changed_session))
        .build()
        .unwrap();
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, changed_session.clone()).await,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert_eq!(revisions(&path, &changed_session.attempt_id).len(), 1);

    let (_dir, path) = create_store();
    let mut unknown = requested_after(&session_bound());
    unknown.attempt_id = record_id(RecordKind::Attempt, "01J00000000000000000000002");
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&unknown))
        .build()
        .unwrap();
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, unknown.clone()).await,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert!(revisions(&path, &unknown.attempt_id).is_empty());

    let (_dir, path) = create_store();
    let requested = seed(&path);
    for mode in [RequestFaultMode::Write, RequestFaultMode::Attestation] {
        let fake = FakeCoven::builder()
            .acknowledge_termination(acknowledgement_for(&requested))
            .build()
            .unwrap();
        let mut faulty = RequestFault {
            inner: persistence(&path),
            mode,
        };
        let result = persist_then_terminate(&mut faulty, &fake, requested.clone()).await;
        match mode {
            RequestFaultMode::Write => assert!(matches!(
                result,
                Err(TerminationDispatchError::RequestPersistence(_))
            )),
            RequestFaultMode::Attestation => assert!(matches!(
                result,
                Err(TerminationDispatchError::PersistedBindingMismatch)
            )),
        }
        assert_eq!(revisions(&path, &requested.attempt_id).len(), 1);
    }

    let (_dir, path) = create_store();
    let requested = seed(&path);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut fault = OutcomeFault {
        inner: persistence(&path),
        fail_once: true,
        attest_wrong_bytes: false,
    };
    assert!(
        persist_then_terminate(&mut fault, &fake, requested.clone())
            .await
            .is_err()
    );
    let mut changed_reason = requested;
    changed_reason.termination_reason_code = Some("shutdown".to_owned());
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&changed_reason))
        .build()
        .unwrap();
    assert!(matches!(
        persist_then_terminate(&mut persistence(&path), &fake, changed_reason).await,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
}

#[derive(Debug, Clone, Copy)]
enum RequestFaultMode {
    Write,
    Attestation,
}

struct RequestFault {
    inner: StoreTerminationPersistence,
    mode: RequestFaultMode,
}

impl TerminationPersistence for RequestFault {
    type Error = StoreError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        match self.mode {
            RequestFaultMode::Write => Err(TerminationPersistenceFailure::Write(
                StoreError::DatabaseOperation,
            )),
            RequestFaultMode::Attestation => {
                let bytes = canonical_bytes(&requested)
                    .map_err(StoreError::Contract)
                    .map_err(TerminationPersistenceFailure::Write)?;
                Ok(alternate_utc_spelling(bytes))
            }
        }
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.inner.persist_outcome(outcome)
    }
}

#[tokio::test]
async fn termination_dispatch_persists_acknowledged_outcome_before_success() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let mut evidence = acknowledgement_for(&requested);
    evidence.acknowledged_at = requested.termination_request.as_ref().unwrap().created_at;
    let disposition = TerminationDisposition::Acknowledged {
        evidence: evidence.clone(),
    };
    let fake = FakeCoven::builder()
        .acknowledge_termination(evidence)
        .build()
        .unwrap();
    let actual = persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
        .await
        .unwrap();
    assert_eq!(actual, disposition);
    let stored = revisions(&path, &requested.attempt_id);
    assert_eq!(stored.len(), 3);
    let expected = derive_termination_outcome_revision(&requested, &disposition).unwrap();
    assert_eq!(
        canonical_bytes(&stored[2]).unwrap(),
        canonical_bytes(&expected).unwrap()
    );
}

#[tokio::test]
async fn termination_dispatch_persists_unresolved_outcome_before_success() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let mut evidence = unresolved_for(&requested);
    evidence.recorded_at = requested.termination_request.as_ref().unwrap().valid_until;
    let disposition = TerminationDisposition::Unresolved {
        evidence: evidence.clone(),
    };
    let fake = FakeCoven::builder()
        .unresolved_termination(evidence)
        .build()
        .unwrap();
    let actual = persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
        .await
        .unwrap();
    assert_eq!(actual, disposition);
    let stored = revisions(&path, &requested.attempt_id);
    assert_eq!(stored.len(), 3);
    let expected = derive_termination_outcome_revision(&requested, &disposition).unwrap();
    assert_eq!(
        canonical_bytes(&stored[2]).unwrap(),
        canonical_bytes(&expected).unwrap()
    );
}

#[tokio::test]
async fn termination_dispatch_exact_replay_is_idempotent() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let evidence = acknowledgement_for(&requested);
    let fake = FakeCoven::builder()
        .acknowledge_termination(evidence.clone())
        .acknowledge_termination(evidence)
        .build()
        .unwrap();
    persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
        .await
        .unwrap();
    persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
        .await
        .unwrap();
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

struct OutcomeFault {
    inner: StoreTerminationPersistence,
    fail_once: bool,
    attest_wrong_bytes: bool,
}

impl TerminationPersistence for OutcomeFault {
    type Error = StoreError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.inner.persist_requested(requested)
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        if self.fail_once {
            self.fail_once = false;
            return Err(TerminationPersistenceFailure::Write(
                StoreError::DatabaseOperation,
            ));
        }
        let bytes = self.inner.persist_outcome(outcome)?;
        if self.attest_wrong_bytes {
            Ok(alternate_utc_spelling(bytes))
        } else {
            Ok(bytes)
        }
    }
}

#[tokio::test]
async fn termination_dispatch_crash_after_response_leaves_recoverable_request() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut fault = OutcomeFault {
        inner: persistence(&path),
        fail_once: true,
        attest_wrong_bytes: false,
    };
    let result = persist_then_terminate(&mut fault, &fake, requested.clone()).await;
    assert!(matches!(
        result,
        Err(TerminationDispatchError::OutcomePersistenceIndeterminate(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
}

#[tokio::test]
async fn termination_dispatch_restart_recovers_missing_outcome() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let evidence = acknowledgement_for(&requested);
    let fake = FakeCoven::builder()
        .acknowledge_termination(evidence.clone())
        .build()
        .unwrap();
    let mut fault = OutcomeFault {
        inner: persistence(&path),
        fail_once: true,
        attest_wrong_bytes: false,
    };
    assert!(
        persist_then_terminate(&mut fault, &fake, requested.clone())
            .await
            .is_err()
    );
    drop(fault);
    let restarted = FakeCoven::builder()
        .acknowledge_termination(evidence)
        .build()
        .unwrap();
    persist_then_terminate(&mut persistence(&path), &restarted, requested.clone())
        .await
        .unwrap();
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_dispatch_rejects_conflicting_replay_response() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .conflicting_termination(TerminationDisposition::Unresolved {
            evidence: unresolved_for(&requested),
        })
        .build()
        .unwrap();
    persist_then_terminate(&mut persistence(&path), &fake, requested.clone())
        .await
        .unwrap();
    let result = persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await;
    assert!(matches!(
        result,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_dispatch_rejects_invalid_outcome_evidence() {
    type EvidenceMutation = Box<dyn Fn(&mut CancellationAcknowledgementEvidence)>;
    let mut mutations: Vec<EvidenceMutation> = vec![
        Box::new(|evidence| evidence.acknowledgement_id.clear()),
        Box::new(|evidence| evidence.acknowledgement_id = "a".repeat(256)),
        Box::new(|evidence| {
            evidence.termination_request_id =
                RequestId::parse("req_01J00000000000000000000002").unwrap();
        }),
        Box::new(|evidence| evidence.session_id = "session-2".to_owned()),
        Box::new(|evidence| {
            evidence.execution_request_id =
                RequestId::parse("req_01J00000000000000000000002").unwrap();
        }),
        Box::new(|evidence| evidence.execution_request_digest = digest_of('e')),
        Box::new(|evidence| evidence.authority_evidence_digest = digest_of('0')),
        Box::new(|evidence| evidence.acknowledged_at = at("2026-08-05T14:03:00.000000001Z")),
    ];
    for mutation in mutations.drain(..) {
        let (_dir, path) = create_store();
        let requested = seed(&path);
        let mut evidence = acknowledgement_for(&requested);
        mutation(&mut evidence);
        let fake = FakeCoven::builder()
            .acknowledge_termination(evidence)
            .build()
            .unwrap();
        let result =
            persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await;
        assert!(matches!(
            result,
            Err(TerminationDispatchError::OutcomeEvidenceMismatch)
        ));
        assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
    }
}

#[tokio::test]
async fn termination_dispatch_rejects_unresolved_outside_termination_window() {
    for recorded_at in [
        at("2026-08-05T14:00:59.999999999Z"),
        at("2026-08-05T14:03:00.000000001Z"),
    ] {
        let (_dir, path) = create_store();
        let requested = seed(&path);
        let mut evidence = unresolved_for(&requested);
        evidence.recorded_at = recorded_at;
        let fake = FakeCoven::builder()
            .unresolved_termination(evidence)
            .build()
            .unwrap();
        let result =
            persist_then_terminate(&mut persistence(&path), &fake, requested.clone()).await;
        assert!(matches!(
            result,
            Err(TerminationDispatchError::OutcomeEvidenceMismatch)
        ));
        assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
    }
}

#[tokio::test]
async fn termination_dispatch_reports_indeterminate_outcome_persistence() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut fault = OutcomeFault {
        inner: persistence(&path),
        fail_once: true,
        attest_wrong_bytes: false,
    };
    assert!(matches!(
        persist_then_terminate(&mut fault, &fake, requested.clone()).await,
        Err(TerminationDispatchError::OutcomePersistenceIndeterminate(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 2);
}

struct ConcurrentPersistence {
    inner: StoreTerminationPersistence,
    path: PathBuf,
    concurrent: Option<ExecutionBinding>,
}

impl TerminationPersistence for ConcurrentPersistence {
    type Error = StoreError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.inner.persist_requested(requested)
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        if let Some(concurrent) = self.concurrent.take() {
            Store::open(&self.path)
                .unwrap()
                .insert(&CanonicalDocument::ExecutionBinding(concurrent))
                .unwrap();
        }
        self.inner.persist_outcome(outcome)
    }
}

#[tokio::test]
async fn termination_dispatch_accepts_concurrent_exact_outcome_replay() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let disposition = TerminationDisposition::Acknowledged {
        evidence: acknowledgement_for(&requested),
    };
    let concurrent = derive_termination_outcome_revision(&requested, &disposition).unwrap();
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut persistence = ConcurrentPersistence {
        inner: persistence(&path),
        path: path.clone(),
        concurrent: Some(concurrent),
    };
    assert_eq!(
        persist_then_terminate(&mut persistence, &fake, requested.clone())
            .await
            .unwrap(),
        disposition
    );
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_dispatch_rejects_concurrent_divergent_outcome() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let divergent = derive_termination_outcome_revision(
        &requested,
        &TerminationDisposition::Unresolved {
            evidence: unresolved_for(&requested),
        },
    )
    .unwrap();
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut persistence = ConcurrentPersistence {
        inner: persistence(&path),
        path: path.clone(),
        concurrent: Some(divergent),
    };
    assert!(matches!(
        persist_then_terminate(&mut persistence, &fake, requested.clone()).await,
        Err(TerminationDispatchError::RevisionConflict(_))
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn termination_dispatch_rejects_outcome_byte_attestation_mismatch() {
    let (_dir, path) = create_store();
    let requested = seed(&path);
    let fake = FakeCoven::builder()
        .acknowledge_termination(acknowledgement_for(&requested))
        .build()
        .unwrap();
    let mut fault = OutcomeFault {
        inner: persistence(&path),
        fail_once: false,
        attest_wrong_bytes: true,
    };
    assert!(matches!(
        persist_then_terminate(&mut fault, &fake, requested.clone()).await,
        Err(TerminationDispatchError::PersistedOutcomeMismatch)
    ));
    assert_eq!(revisions(&path, &requested.attempt_id).len(), 3);
}

#[tokio::test]
async fn surface_fake_consumes_only_scripted_behavior() {
    let fake = FakeSurface::builder()
        .delivery(DeliveryDisposition::Unknown)
        .build()
        .unwrap();
    let effect = surface_effect();
    let port: &dyn SurfacePort = &fake;
    assert_eq!(
        port.apply(effect.clone()).await.unwrap(),
        DeliveryDisposition::Unknown
    );
    assert_eq!(
        port.apply(effect).await.unwrap(),
        DeliveryDisposition::Unknown
    );
}
