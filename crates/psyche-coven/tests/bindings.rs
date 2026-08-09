#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::sync::atomic::{AtomicUsize, Ordering};

use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, CancellationUnresolvedEvidence, ExecutionBinding,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::{RecordKind, SchemaVersion};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, CapabilityProfile, ContentAddressedReference, CovenPort,
    EventCursor, EventPage, ExecutionRequestInput, NegotiateRequest, PortError,
    ReconciliationDisposition, ReconciliationRequest, ResultBundle, SessionSnapshot,
    TerminationDispatchError, TerminationDisposition, TerminationPersistence,
    TerminationPersistenceFailure, TerminationRequest, persist_then_terminate,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const RESULT_GOLDEN: &[u8] = include_bytes!("fixtures/result-bundle.json");
const LAUNCH_GOLDEN: &[u8] = include_bytes!("fixtures/execution-request-launch.json");

fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn digest_of(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn record_id(kind: RecordKind, suffix: &str) -> RecordId {
    RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())).unwrap()
}

#[test]
fn result_bundle_fixture_round_trips_complete_content_references() {
    let bundle: ResultBundle = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    bundle.validate().unwrap();
    assert_eq!(canonical_bytes(&bundle).unwrap(), RESULT_GOLDEN);
    assert_eq!(bundle.session_id, "session-1");
    assert_eq!(
        bundle.result.digest.as_str(),
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    assert_eq!(bundle.result.media_type, "application/json");
    assert_eq!(bundle.result.size_bytes, 2);
    assert_eq!(bundle.result.expires_at, at("2026-08-05T14:04:00Z"));
    assert_eq!(bundle.artifacts.len(), 1);
    assert_eq!(bundle.artifacts[0].artifact_id, "artifact-1");
    assert_eq!(
        bundle.artifacts[0].content.digest.as_str(),
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    assert_eq!(bundle.artifacts[0].content.media_type, "text/plain");
    assert_eq!(bundle.artifacts[0].content.size_bytes, 5);
    assert_eq!(
        bundle.artifacts[0].content.expires_at,
        at("2026-08-05T14:03:00Z")
    );

    let mut missing: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    missing.as_object_mut().unwrap().remove("result");
    assert!(serde_json::from_value::<ResultBundle>(missing).is_err());
    let mut unknown: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    unknown["future"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ResultBundle>(unknown).is_err());

    let mut missing_nested: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    missing_nested["result"]
        .as_object_mut()
        .unwrap()
        .remove("digest");
    assert!(serde_json::from_value::<ResultBundle>(missing_nested).is_err());
    let mut unknown_nested: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    unknown_nested["artifacts"][0]["content"]["future"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ResultBundle>(unknown_nested).is_err());
}

#[test]
fn result_bundle_fixture_uses_launch_request_correlation() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let expected = AdoptionRequest::new(input).unwrap().correlation();
    let bundle: ResultBundle = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    assert_eq!(bundle.correlation, expected);
    assert_eq!(bundle.artifacts[0].correlation, expected);
}

#[test]
fn result_bundle_accepts_unique_artifacts_in_wire_order() {
    let mut ordered: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    let mut second = ordered["artifacts"][0].clone();
    second["artifact_id"] = serde_json::json!("artifact-2");
    second["content"]["digest"] = serde_json::json!(format!("sha256:{}", "d".repeat(64)));
    ordered["artifacts"].as_array_mut().unwrap().push(second);
    let ordered: ResultBundle = serde_json::from_value(ordered.clone()).unwrap();

    let mut reversed = serde_json::to_value(ordered).unwrap();
    reversed["artifacts"].as_array_mut().unwrap().reverse();
    let reversed: ResultBundle = serde_json::from_value(reversed).unwrap();

    assert_eq!(reversed.artifacts[0].artifact_id, "artifact-2");
    assert_eq!(reversed.artifacts[1].artifact_id, "artifact-1");
}

#[test]
fn content_reference_rejects_digest_size_media_type_and_lifetime_mismatch() {
    let expires = at("2030-08-05T14:04:00Z");
    let reference =
        ContentAddressedReference::for_bytes("application/json", b"{}", expires).unwrap();
    reference.validate().unwrap();
    reference.validate_payload(b"{}").unwrap();
    assert!(reference.validate_payload(b"{]").is_err());

    let mut wrong_size = reference.clone();
    wrong_size.size_bytes += 1;
    assert!(wrong_size.validate_payload(b"{}").is_err());
    let mut wrong_digest = reference.clone();
    wrong_digest.digest = digest_of('a');
    assert!(wrong_digest.validate_payload(b"{}").is_err());
    for media_type in [
        "",
        "TEXT/PLAIN",
        "text",
        "text/plain; charset=utf-8",
        "text/ plain",
        "text//plain",
    ] {
        assert!(
            ContentAddressedReference::for_bytes(media_type, b"x", expires).is_err(),
            "{media_type:?}"
        );
    }
    let mut zero = reference.clone();
    zero.size_bytes = 0;
    assert!(zero.validate().is_err());
    let mut oversized = reference.clone();
    oversized.size_bytes = 9_007_199_254_740_992;
    assert!(oversized.validate().is_err());
    let mut maximum = reference.clone();
    maximum.size_bytes = 9_007_199_254_740_991;
    maximum.validate().unwrap();
    let maximum: ContentAddressedReference =
        serde_json::from_value(serde_json::to_value(maximum).unwrap()).unwrap();
    maximum.validate().unwrap();
    assert!(
        reference
            .validate_payload_at(b"{}", expires + time::Duration::nanoseconds(1))
            .is_err()
    );

    let bundle: ResultBundle = serde_json::from_slice(RESULT_GOLDEN).unwrap();
    let mut value = serde_json::to_value(&bundle).unwrap();
    value["result"]["expires_at"] = serde_json::json!("2026-08-05T14:05:00.000000001Z");
    assert!(serde_json::from_value::<ResultBundle>(value).is_err());
    let mut value = serde_json::to_value(&bundle).unwrap();
    value["artifacts"][0]["content"]["expires_at"] =
        serde_json::json!("2026-08-05T14:04:00.000000001Z");
    assert!(serde_json::from_value::<ResultBundle>(value).is_err());
    let mut value = serde_json::to_value(&bundle).unwrap();
    let duplicate = value["artifacts"][0].clone();
    value["artifacts"].as_array_mut().unwrap().push(duplicate);
    assert!(serde_json::from_value::<ResultBundle>(value).is_err());

    for (pointer, replacement) in [
        ("/artifacts/0/session_id", serde_json::json!("session-2")),
        (
            "/artifacts/0/correlation/request_id",
            serde_json::json!("req_01J00000000000000000000001"),
        ),
        (
            "/artifacts/0/correlation/request_digest",
            serde_json::json!(format!("sha256:{}", "d".repeat(64))),
        ),
        (
            "/artifacts/0/correlation/familiar_snapshot_id",
            serde_json::json!("ids_01J00000000000000000000001"),
        ),
        (
            "/artifacts/0/correlation/project_id",
            serde_json::json!("project:sha256:def"),
        ),
        (
            "/artifacts/0/correlation/graph_id",
            serde_json::json!("grf_01J00000000000000000000001"),
        ),
        (
            "/artifacts/0/correlation/node_id",
            serde_json::json!("nod_01J00000000000000000000001"),
        ),
        (
            "/artifacts/0/correlation/attempt_id",
            serde_json::json!("att_01J00000000000000000000001"),
        ),
        (
            "/artifacts/0/correlation/created_at",
            serde_json::json!("2026-08-05T14:00:01Z"),
        ),
        (
            "/artifacts/0/correlation/valid_until",
            serde_json::json!("2026-08-05T14:04:59Z"),
        ),
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<ResultBundle>(value).is_err(),
            "{pointer}"
        );
    }

    for (pointer, replacement) in [
        ("/result/media_type", serde_json::json!("Application/JSON")),
        ("/result/size_bytes", serde_json::json!(0)),
        (
            "/result/size_bytes",
            serde_json::json!((i64::MAX as u64) + 1),
        ),
        (
            "/artifacts/0/content/media_type",
            serde_json::json!("text/plain; charset=utf-8"),
        ),
        ("/artifacts/0/content/size_bytes", serde_json::json!(0)),
        (
            "/artifacts/0/content/size_bytes",
            serde_json::json!((i64::MAX as u64) + 1),
        ),
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<ResultBundle>(value).is_err(),
            "{pointer}"
        );
    }

    for pointer in ["/result/size_bytes", "/artifacts/0/content/size_bytes"] {
        let mut value: serde_json::Value = serde_json::from_slice(RESULT_GOLDEN).unwrap();
        *value.pointer_mut(pointer).unwrap() = serde_json::json!(9_007_199_254_740_991_u64);
        let bundle: ResultBundle = serde_json::from_value(value).unwrap();
        bundle.validate().unwrap();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistenceError {
    Unexpected,
}

#[derive(Default)]
struct RecordingPersistence {
    requested_calls: usize,
    outcome_calls: usize,
}

impl TerminationPersistence for RecordingPersistence {
    type Error = PersistenceError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.requested_calls += 1;
        canonical_bytes(&requested)
            .map_err(|_| TerminationPersistenceFailure::Write(PersistenceError::Unexpected))
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.outcome_calls += 1;
        canonical_bytes(&outcome)
            .map_err(|_| TerminationPersistenceFailure::Write(PersistenceError::Unexpected))
    }
}

struct AcknowledgingPort {
    calls: AtomicUsize,
}

impl AcknowledgingPort {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl CovenPort for AcknowledgingPort {
    async fn negotiate(&self, _request: NegotiateRequest) -> Result<CapabilityProfile, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn adopt(&self, _request: AdoptionRequest) -> Result<AdoptionDisposition, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn lookup(&self, _request_id: &RequestId) -> Result<AdoptionDisposition, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn reconcile(
        &self,
        _request: ReconciliationRequest,
    ) -> Result<ReconciliationDisposition, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn inspect(&self, _session_id: &str) -> Result<SessionSnapshot, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn events(&self, _cursor: EventCursor) -> Result<EventPage, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn result(&self, _session_id: &str) -> Result<ResultBundle, PortError> {
        Err(PortError::UnexpectedCall)
    }

    async fn terminate(
        &self,
        request: TerminationRequest,
    ) -> Result<TerminationDisposition, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let binding = request.binding();
        let correlation = binding.termination_request.as_ref().unwrap();
        Ok(TerminationDisposition::Acknowledged {
            evidence: CancellationAcknowledgementEvidence {
                acknowledgement_id: "ack-1".to_owned(),
                termination_request_id: correlation.termination_request_id.clone(),
                session_id: binding.coven_session_id.clone().unwrap(),
                execution_request_id: binding.request_id.clone(),
                execution_request_digest: binding.request_digest.clone(),
                kind: CancellationAcknowledgementKind::Terminated,
                authority_evidence_digest: digest_of('d'),
                acknowledged_at: correlation.created_at,
            },
        })
    }
}

fn valid_requested() -> ExecutionBinding {
    ExecutionBinding {
        schema_version: SchemaVersion::parse("psyche.execution_binding.v1").unwrap(),
        attempt_id: record_id(RecordKind::Attempt, "01J00000000000000000000000"),
        revision: 2,
        previous_revision_digest: Some(digest_of('a')),
        revision_created_at: at("2026-08-05T14:01:00Z"),
        familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000000"),
        project_id: "project:sha256:abc".to_owned(),
        request_id: RequestId::parse("req_01J00000000000000000000000").unwrap(),
        request_digest: digest_of('b'),
        request_created_at: at("2026-08-05T14:00:00Z"),
        request_valid_until: at("2026-08-05T14:05:00Z"),
        coven_contract_version: "coven.daemon.v1".to_owned(),
        coven_session_id: Some("session-1".to_owned()),
        adoption_state: AdoptionState::Adopted,
        event_cursor: None,
        cancellation_state: CancellationState::TerminationRequested,
        termination_request: Some(TerminationRequestCorrelation {
            termination_request_id: RequestId::parse("req_01J00000000000000000000001").unwrap(),
            created_at: at("2026-08-05T14:01:00Z"),
            valid_until: at("2026-08-05T14:03:00Z"),
        }),
        termination_reason_code: Some("operator_request".to_owned()),
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    }
}

fn acknowledged_binding(
    mut value: ExecutionBinding,
    kind: CancellationAcknowledgementKind,
) -> ExecutionBinding {
    let termination = value.termination_request.as_ref().unwrap();
    let termination_request_id = termination.termination_request_id.clone();
    let acknowledged_at = termination.created_at;
    value.cancellation_state = match kind {
        CancellationAcknowledgementKind::Terminated => CancellationState::AcknowledgedTerminated,
        CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal => {
            CancellationState::AcknowledgedAlreadyTerminal
        }
    };
    value.cancellation_acknowledgement = Some(CancellationAcknowledgementEvidence {
        acknowledgement_id: "ack-1".to_owned(),
        termination_request_id,
        session_id: value.coven_session_id.clone().unwrap(),
        execution_request_id: value.request_id.clone(),
        execution_request_digest: value.request_digest.clone(),
        kind,
        authority_evidence_digest: digest_of('d'),
        acknowledged_at,
    });
    value
}

fn unresolved_binding(mut value: ExecutionBinding) -> ExecutionBinding {
    let termination = value.termination_request.as_ref().unwrap();
    let termination_request_id = termination.termination_request_id.clone();
    let recorded_at = termination.created_at;
    value.cancellation_state = CancellationState::TerminationUnknown;
    value.cancellation_unresolved = Some(CancellationUnresolvedEvidence {
        disposition_id: "unresolved-1".to_owned(),
        termination_request_id,
        session_id: value.coven_session_id.clone().unwrap(),
        execution_request_id: value.request_id.clone(),
        execution_request_digest: value.request_digest.clone(),
        reason_code: "timeout".to_owned(),
        recorded_at,
    });
    value
}

#[tokio::test]
async fn termination_dispatch_rejects_invalid_request_before_persistence() {
    for accepted in [
        {
            let mut value = valid_requested();
            value.termination_request.as_mut().unwrap().created_at = value.request_created_at;
            value
        },
        {
            let mut value = valid_requested();
            value.termination_request.as_mut().unwrap().created_at =
                value.request_valid_until + time::Duration::seconds(1);
            value.termination_request.as_mut().unwrap().valid_until =
                value.request_valid_until + time::Duration::seconds(2);
            value
        },
        {
            let mut value = valid_requested();
            value.termination_reason_code = Some("operator2_request".to_owned());
            value
        },
    ] {
        let mut persistence = RecordingPersistence::default();
        let port = AcknowledgingPort::new();
        let dyn_port: &dyn CovenPort = &port;
        let result = persist_then_terminate(&mut persistence, dyn_port, accepted).await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(persistence.requested_calls, 1);
        assert_eq!(persistence.outcome_calls, 1);
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    let valid = valid_requested();
    let cases: Vec<ExecutionBinding> = vec![
        {
            let mut value = valid.clone();
            value.coven_session_id = Some(String::new());
            value
        },
        {
            let mut value = valid.clone();
            value.coven_session_id = Some("s".repeat(256));
            value
        },
        {
            let mut value = valid.clone();
            value.termination_reason_code = Some(String::new());
            value
        },
        {
            let mut value = valid.clone();
            value.termination_reason_code = Some("r".repeat(129));
            value
        },
        {
            let mut value = valid.clone();
            value.termination_reason_code = Some("OperatorRequest".to_owned());
            value
        },
        {
            let mut value = valid.clone();
            value
                .termination_request
                .as_mut()
                .unwrap()
                .termination_request_id = value.request_id.clone();
            value
        },
        {
            let mut value = valid.clone();
            let termination = value.termination_request.as_mut().unwrap();
            termination.valid_until = termination.created_at;
            value
        },
        {
            let mut value = valid.clone();
            let termination = value.termination_request.as_mut().unwrap();
            termination.valid_until = termination.created_at - time::Duration::nanoseconds(1);
            value
        },
        {
            let mut value = valid.clone();
            value.termination_request.as_mut().unwrap().created_at =
                value.request_created_at - time::Duration::nanoseconds(1);
            value
        },
        {
            let mut value = valid.clone();
            value.revision = 1;
            value.previous_revision_digest = None;
            value
        },
        {
            let mut value = valid.clone();
            value.cancellation_state = CancellationState::NotRequested;
            value.termination_request = None;
            value.termination_reason_code = None;
            value
        },
        acknowledged_binding(valid.clone(), CancellationAcknowledgementKind::Terminated),
        acknowledged_binding(
            valid.clone(),
            CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal,
        ),
        unresolved_binding(valid),
    ];

    for candidate in cases {
        let mut persistence = RecordingPersistence::default();
        let port = AcknowledgingPort::new();
        let result = persist_then_terminate(&mut persistence, &port, candidate).await;
        assert!(
            matches!(result, Err(TerminationDispatchError::Contract(_))),
            "{result:?}"
        );
        assert_eq!(persistence.requested_calls, 0);
        assert_eq!(persistence.outcome_calls, 0);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    assert_ne!(digest(&valid_requested()).unwrap(), digest_of('a'));
}
