//! Psyche v1 contract fixtures and strict decoding integration tests.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use psyche_core::contracts::error::ErrorCode;
use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, ExecutionBinding, TerminationRequestCorrelation,
};
use psyche_core::contracts::{CanonicalDocument, ContractError, SchemaKind, decode_document};
use psyche_core::digest::canonical_bytes;
use psyche_core::id::{RecordId, RequestId};
use serde_json::{Value, json};

const ULID_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ULID_B: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn decode(name: &str) -> CanonicalDocument {
    decode_document(&fixture(name)).unwrap()
}

fn mutate(name: &str, f: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&fixture(name)).unwrap();
    f(value.as_object_mut().unwrap());
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn intent_rejects_unknown_fields() {
    let bytes = mutate("intent-local.json", |object| {
        object.insert("unexpected".into(), json!(true));
    });
    assert!(matches!(
        decode_document(&bytes),
        Err(ContractError::InvalidShape { .. })
    ));
}

#[test]
fn graph_and_node_accept_only_the_two_frozen_nullable_bindings() {
    let intent = decode("intent-local.json");
    let node = decode("node-root.json");
    assert!(matches!(intent, CanonicalDocument::Intent(_)));
    assert!(matches!(node, CanonicalDocument::GraphNode(_)));

    let missing_required = mutate("intent-local.json", |object| {
        object.insert("principal_id".into(), Value::Null);
    });
    assert!(matches!(
        decode_document(&missing_required),
        Err(ContractError::InvalidShape { .. })
    ));

    let missing_node_binding = mutate("node-root.json", |object| {
        object.insert("budget_id".into(), Value::Null);
    });
    assert!(matches!(
        decode_document(&missing_node_binding),
        Err(ContractError::InvalidShape { .. })
    ));
}

#[test]
fn delivery_v1_fixture_round_trips_canonically() {
    let document = decode("delivery-ready.json");
    assert!(matches!(document, CanonicalDocument::Delivery(_)));
    let expected: Value = serde_json::from_slice(&fixture("delivery-ready.json")).unwrap();
    assert_eq!(
        canonical_bytes(&document).unwrap(),
        canonical_bytes(&expected).unwrap()
    );
    document.validate().unwrap();
}

#[test]
fn surface_event_and_effect_fixtures_round_trip() {
    for (name, expected_kind) in [
        ("surface-event.json", SchemaKind::SurfaceEvent),
        ("surface-effect.json", SchemaKind::SurfaceEffect),
    ] {
        let document = decode(name);
        assert_eq!(document.schema_version().kind, expected_kind);
        let expected: Value = serde_json::from_slice(&fixture(name)).unwrap();
        assert_eq!(
            canonical_bytes(&document).unwrap(),
            canonical_bytes(&expected).unwrap()
        );
    }
}

#[test]
fn delivery_keeps_the_canonical_del_prefix() {
    let document = decode("delivery-ready.json");
    assert_eq!(
        document.persistable_record_id().unwrap().as_str(),
        format!("del_{ULID_A}")
    );
    let wrong = mutate("delivery-ready.json", |o| {
        o.insert("delivery_id".into(), json!(format!("dlg_{ULID_A}")));
    });
    assert!(decode_document(&wrong).is_err());
}

#[test]
fn delegation_uses_the_distinct_dlg_prefix() {
    let value = json!({
        "schema_version": "psyche.delegation.v1",
        "delegation_id": format!("dlg_{ULID_A}"),
        "parent_node_id": format!("nod_{ULID_A}"),
        "child_node_id": format!("nod_{ULID_B}"),
        "scope_digest": format!("sha256:{}", "1".repeat(64)),
        "budget_id": format!("bud_{ULID_A}"),
        "evidence_scope_digest": format!("sha256:{}", "2".repeat(64)),
        "cancellation_policy": "cascade"
    });
    let document = decode_document(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        document.persistable_record_id().unwrap().as_str(),
        format!("dlg_{ULID_A}")
    );
}

#[test]
fn execution_binding_uses_attempt_as_its_only_record_kind() {
    let binding = valid_binding(CancellationState::NotRequested);
    let document = CanonicalDocument::ExecutionBinding(binding);
    document.validate().unwrap();
    assert_eq!(
        document.persistable_record_id().unwrap().as_str(),
        format!("att_{ULID_A}")
    );
}

#[test]
fn surface_values_reject_scalars_oversize_unknown_fields_wrong_ids_and_bad_digest() {
    for field in ["actor", "locator", "content"] {
        let bytes = mutate("surface-event.json", |o| {
            o.insert(field.into(), json!("scalar"));
        });
        assert!(decode_document(&bytes).is_err(), "{field}");
    }
    for (field, value) in [
        ("surface_event_id", format!("sfx_{ULID_A}")),
        ("intent_id", format!("grf_{ULID_A}")),
        ("graph_id", format!("int_{ULID_A}")),
        ("node_id", format!("att_{ULID_A}")),
        ("attempt_id", format!("nod_{ULID_A}")),
        ("familiar_snapshot_id", format!("int_{ULID_A}")),
    ] {
        let name = if field == "surface_event_id" {
            "surface-event.json"
        } else {
            "surface-effect.json"
        };
        let bytes = mutate(name, |o| {
            o.insert(field.into(), json!(value));
        });
        assert!(decode_document(&bytes).is_err(), "{field}");
    }
    let scalar_effect = mutate("surface-effect.json", |o| {
        o.insert("effect".into(), json!(false));
    });
    assert!(decode_document(&scalar_effect).is_err());
    let bad_digest = mutate("surface-effect.json", |o| {
        o.insert(
            "effect_digest".into(),
            json!(format!("sha256:{}", "0".repeat(64))),
        );
    });
    assert!(decode_document(&bad_digest).is_err());
    let unknown = mutate("surface-event.json", |o| {
        o.insert("extra".into(), json!(1));
    });
    assert!(decode_document(&unknown).is_err());
    let oversized = mutate("surface-event.json", |o| {
        o.insert("content".into(), json!({"text": "x".repeat(1_048_577)}));
    });
    assert!(decode_document(&oversized).is_err());
}

#[test]
fn delivery_rejects_removed_fields_bad_enums_ids_effects_and_sent_without_message_id() {
    for removed in ["surface_effect_id", "surface_decision_digest", "attempts"] {
        let bytes = mutate("delivery-ready.json", |o| {
            o.insert(removed.into(), json!("removed"));
        });
        assert!(decode_document(&bytes).is_err(), "{removed}");
    }
    for (field, bad) in [
        ("relationship", json!("same_chat")),
        ("state", json!("done")),
        ("delivery_id", json!(format!("dly_{ULID_A}"))),
        ("intent_id", json!(format!("del_{ULID_B}"))),
        ("chat_id", json!("12x")),
    ] {
        let bytes = mutate("delivery-ready.json", |o| {
            o.insert(field.into(), bad);
        });
        assert!(decode_document(&bytes).is_err(), "{field}");
    }
    for effect in [
        json!("scalar"),
        json!({}),
        json!({"x": "y".repeat(1_048_577)}),
    ] {
        let bytes = mutate("delivery-ready.json", |o| {
            o.insert("effect".into(), effect);
        });
        assert!(decode_document(&bytes).is_err());
    }
    let bad_digest = mutate("delivery-ready.json", |o| {
        o.insert(
            "effect_digest".into(),
            json!(format!("sha256:{}", "f".repeat(64))),
        );
    });
    assert!(decode_document(&bad_digest).is_err());
    let bad_expiry = mutate("delivery-ready.json", |o| {
        o.get_mut("surface_decision")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("expires_at".into(), json!("tomorrow"));
    });
    assert!(decode_document(&bad_expiry).is_err());
    let sent = mutate("delivery-ready.json", |o| {
        o.insert("state".into(), json!("sent"));
    });
    assert!(decode_document(&sent).is_err());
}

#[test]
fn all_canonical_error_codes_decode() {
    let envelopes: Vec<Value> = serde_json::from_slice(&fixture("error-codes-v1.json")).unwrap();
    assert_eq!(envelopes.len(), ErrorCode::ALL.len());
    let mut codes = Vec::new();
    for (value, expected) in envelopes.iter().zip(ErrorCode::ALL) {
        let code = value["error"]["code"].as_str().unwrap();
        codes.push(code);
        assert_eq!(code, expected.as_str());
        let document = decode_document(&serde_json::to_vec(value).unwrap()).unwrap();
        assert!(matches!(document, CanonicalDocument::Error(_)));
        assert_eq!(serde_json::to_value(document).unwrap(), *value);
    }
    let unique: std::collections::HashSet<_> = codes.iter().collect();
    assert_eq!(unique.len(), ErrorCode::ALL.len());
}

#[test]
fn error_envelope_is_strict_and_never_persistable() {
    let envelope = json!({
        "schema_version": "psyche.error.v1",
        "error": {
            "code": "config_invalid",
            "message": "bad config",
            "retryable": false,
            "correlation_id": "corr-1",
            "details": {}
        }
    });
    let document = decode_document(&serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(document.persistable_record_id().is_none());
    for bad in [
        json!({"schema_version":"psyche.error.v1","error":{"code":"CONFIG_INVALID","message":"bad","retryable":false,"correlation_id":"c","details":{}}}),
        json!({"schema_version":"psyche.error.v1","error":{"code":"config-invalid","message":"bad","retryable":false,"correlation_id":"c","details":{}}}),
        json!({"schema_version":"psyche.error.v1","error":{"code":"future_error","message":"bad","retryable":false,"correlation_id":"c","details":{}}}),
        json!({"schema_version":"psyche.error.v1","error":{"code":"config_invalid","message":"","retryable":false,"correlation_id":"c","details":{}}}),
        json!({"schema_version":"psyche.error.v1","error":{"code":"config_invalid","message":"bad","retryable":false,"correlation_id":"c","details":{"x":1}}}),
        json!({"schema_version":"psyche.error.v1","error":{"code":"config_invalid","message":"bad","retryable":false,"correlation_id":"c","details":{},"extra":true}}),
    ] {
        let result = decode_document(&serde_json::to_vec(&bad).unwrap());
        if bad["error"]["code"] == "future_error" {
            assert!(matches!(
                result,
                Err(ContractError::UnknownEnumValue {
                    schema: SchemaKind::Error,
                    field: "code"
                })
            ));
        } else {
            assert!(result.is_err());
        }
    }
}

fn valid_binding(state: CancellationState) -> ExecutionBinding {
    let request = RequestId::parse(&format!("req_{ULID_A}")).unwrap();
    let termination = RequestId::parse(&format!("req_{ULID_B}")).unwrap();
    let digest_value = format!("sha256:{}", "1".repeat(64));
    let mut binding = ExecutionBinding {
        schema_version: "psyche.execution_binding.v1".parse().unwrap(),
        attempt_id: RecordId::parse(
            psyche_core::contracts::RecordKind::Attempt,
            &format!("att_{ULID_A}"),
        )
        .unwrap(),
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: "2026-08-01T00:00:00Z".into(),
        familiar_snapshot_id: RecordId::parse(
            psyche_core::contracts::RecordKind::IdentitySnapshot,
            &format!("ids_{ULID_B}"),
        )
        .unwrap(),
        project_id: "project:one".into(),
        request_id: request.clone(),
        request_digest: digest_value.clone().try_into().unwrap(),
        request_created_at: "2026-08-01T00:00:00Z".into(),
        request_valid_until: "2026-08-01T00:10:00Z".into(),
        coven_contract_version: "coven.execution.v1".into(),
        coven_session_id: None,
        adoption_state: AdoptionState::NotSubmitted,
        event_cursor: None,
        cancellation_state: state,
        termination_request: None,
        termination_reason_code: None,
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    };
    if state != CancellationState::NotRequested {
        binding.coven_session_id = Some("session-1".into());
        binding.termination_request = Some(TerminationRequestCorrelation {
            termination_request_id: termination.clone(),
            created_at: "2026-08-01T00:01:00Z".into(),
            valid_until: "2026-08-01T00:05:00Z".into(),
        });
        binding.termination_reason_code = Some("operator_requested".into());
    }
    if matches!(
        state,
        CancellationState::AcknowledgedTerminated | CancellationState::AcknowledgedAlreadyTerminal
    ) {
        binding.cancellation_acknowledgement = Some(CancellationAcknowledgementEvidence {
            acknowledgement_id: "ack-1".into(),
            termination_request_id: termination.clone(),
            session_id: "session-1".into(),
            execution_request_id: request.clone(),
            execution_request_digest: digest_value.clone().try_into().unwrap(),
            kind: if state == CancellationState::AcknowledgedTerminated {
                CancellationAcknowledgementKind::Terminated
            } else {
                CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal
            },
            authority_evidence_digest: format!("sha256:{}", "2".repeat(64)).try_into().unwrap(),
            acknowledged_at: "2026-08-01T00:02:00Z".into(),
        });
    }
    if state == CancellationState::TerminationUnknown {
        binding.cancellation_unresolved = Some(
            psyche_core::contracts::execution::CancellationUnresolvedEvidence {
                disposition_id: "disp-1".into(),
                termination_request_id: termination,
                session_id: "session-1".into(),
                execution_request_id: request,
                execution_request_digest: digest_value.try_into().unwrap(),
                reason_code: "authority_unreachable".into(),
                recorded_at: "2026-08-01T00:03:00Z".into(),
            },
        );
    }
    binding
}

#[test]
fn cancellation_state_vocabulary_requires_matching_o5_evidence() {
    for state in [
        CancellationState::NotRequested,
        CancellationState::TerminationRequested,
        CancellationState::AcknowledgedTerminated,
        CancellationState::AcknowledgedAlreadyTerminal,
        CancellationState::TerminationUnknown,
    ] {
        let document = CanonicalDocument::ExecutionBinding(valid_binding(state));
        document.validate().unwrap();
        let bytes = serde_json::to_vec(&document).unwrap();
        let decoded = decode_document(&bytes).unwrap();
        decoded.validate().unwrap();
    }

    let unknown = serde_json::to_vec(&json!({
        "schema_version":"psyche.execution_binding.v1",
        "cancellation_state":"cancelled"
    }))
    .unwrap();
    assert!(decode_document(&unknown).is_err());

    let mut missing = valid_binding(CancellationState::AcknowledgedTerminated);
    missing.cancellation_acknowledgement = None;
    assert!(matches!(
        CanonicalDocument::ExecutionBinding(missing).validate(),
        Err(ContractError::CancellationEvidenceMismatch)
    ));

    let mut mismatch = valid_binding(CancellationState::AcknowledgedTerminated);
    mismatch
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .session_id = "other".into();
    assert!(matches!(
        CanonicalDocument::ExecutionBinding(mismatch).validate(),
        Err(ContractError::CancellationEvidenceMismatch)
    ));

    let mut raw_ledger = valid_binding(CancellationState::AcknowledgedTerminated);
    raw_ledger.cancellation_acknowledgement = None;
    raw_ledger.terminal_state = Some("terminated".into());
    assert!(matches!(
        CanonicalDocument::ExecutionBinding(raw_ledger).validate(),
        Err(ContractError::CancellationEvidenceMismatch)
    ));
}

#[test]
fn strict_probe_and_document_limit_fail_closed() {
    assert!(matches!(
        decode_document(br#"{"schema_version":"psyche.unknown.v1"}"#),
        Err(ContractError::UnknownSchema { .. })
    ));
    assert!(decode_document(&vec![b' '; 1_048_577]).is_err());
}

#[test]
fn directly_constructed_values_are_revalidated() {
    let mut binding = valid_binding(CancellationState::NotRequested);
    binding.revision = 0;
    assert!(
        CanonicalDocument::ExecutionBinding(binding)
            .validate()
            .is_err()
    );
}

#[test]
fn typed_deserialization_cannot_bypass_validation() {
    let wrong_id = mutate("intent-local.json", |object| {
        object.insert("intent_id".into(), json!(format!("grf_{ULID_A}")));
    });
    assert!(serde_json::from_slice::<psyche_core::contracts::intent::Intent>(&wrong_id).is_err());

    let mismatched_digest = mutate("surface-effect.json", |object| {
        object.insert(
            "effect_digest".into(),
            json!(format!("sha256:{}", "0".repeat(64))),
        );
    });
    assert!(
        serde_json::from_slice::<psyche_core::contracts::surface::SurfaceEffect>(
            &mismatched_digest
        )
        .is_err()
    );
}
