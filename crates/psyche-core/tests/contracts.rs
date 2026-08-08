//! Psyche v1 contract fixtures and strict decoding integration tests.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use psyche_core::contracts::error::ErrorCode;
use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, CancellationUnresolvedEvidence, ExecutionBinding,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::foundation::{Approval, Budget, Evidence, Recovery, Verdict};
use psyche_core::contracts::graph::{Graph, GraphNode};
use psyche_core::contracts::identity::IdentitySnapshot;
use psyche_core::contracts::intent::Intent;
use psyche_core::contracts::surface::{
    Delivery, DeliverySurfaceDecision, SurfaceEffect, SurfaceEvent,
};
use psyche_core::contracts::{CanonicalDocument, ContractError, SchemaKind, decode_document};
use psyche_core::digest::{canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use serde_json::{Value, json};
use time::OffsetDateTime;

const ULID_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ULID_B: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";
const ULID_C: &str = "01C3F7YQ4R2M8N6P5K1J9H0GTS";

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

fn replace_fixture_once(name: &str, from: &str, to: &str) -> Vec<u8> {
    let source = String::from_utf8(fixture(name)).unwrap();
    assert_eq!(source.matches(from).count(), 1, "{name}: {from}");
    source.replacen(from, to, 1).into_bytes()
}

fn duplicate_fixture_fragment(name: &str, fragment: &str) -> Vec<u8> {
    replace_fixture_once(name, fragment, &format!("{fragment}, {fragment}"))
}

fn assert_duplicate_json_rejected(bytes: &[u8], location: &str) {
    assert!(
        matches!(
            decode_document(bytes),
            Err(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "json",
            })
        ),
        "{location}"
    );
}

fn timestamp(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
}

#[test]
fn canonical_timestamp_fields_are_typed_and_serialize_as_rfc3339_strings() {
    let assert_types = |identity: IdentitySnapshot,
                        intent: Intent,
                        binding: ExecutionBinding,
                        acknowledgement: CancellationAcknowledgementEvidence,
                        unresolved: CancellationUnresolvedEvidence,
                        correlation: TerminationRequestCorrelation,
                        approval: Approval,
                        evidence: Evidence,
                        verdict: Verdict,
                        event: SurfaceEvent,
                        effect: SurfaceEffect,
                        delivery: Delivery,
                        decision: DeliverySurfaceDecision| {
        let _: OffsetDateTime = identity.resolved_at;
        let _: OffsetDateTime = intent.created_at;
        let _: serde_json::Map<String, Value> = intent.constraints;
        let _: OffsetDateTime = binding.revision_created_at;
        let _: OffsetDateTime = binding.request_created_at;
        let _: OffsetDateTime = binding.request_valid_until;
        let _: OffsetDateTime = acknowledgement.acknowledged_at;
        let _: OffsetDateTime = unresolved.recorded_at;
        let _: OffsetDateTime = correlation.created_at;
        let _: OffsetDateTime = correlation.valid_until;
        let _: OffsetDateTime = approval.expires_at;
        let _: OffsetDateTime = evidence.created_at;
        let _: OffsetDateTime = verdict.created_at;
        let _: OffsetDateTime = event.received_at;
        let _: OffsetDateTime = effect.created_at;
        let _: OffsetDateTime = delivery.surface_decision.expires_at;
        let _: OffsetDateTime = decision.expires_at;
    };
    let _ = assert_types;

    let binding = valid_binding(CancellationState::AcknowledgedTerminated);
    let value = serde_json::to_value(binding).unwrap();
    assert_eq!(value["revision_created_at"], "2026-08-01T00:00:00Z");
    assert_eq!(
        value["termination_request"]["created_at"],
        "2026-08-01T00:01:00Z"
    );
    assert_eq!(
        value["cancellation_acknowledgement"]["acknowledged_at"],
        "2026-08-01T00:02:00Z"
    );
}

#[test]
fn directly_constructed_typed_timestamps_still_enforce_cross_field_windows() {
    let mut binding = valid_binding(CancellationState::NotRequested);
    binding.request_valid_until = binding.request_created_at;
    assert!(matches!(
        CanonicalDocument::ExecutionBinding(binding).validate(),
        Err(ContractError::InvalidShape {
            schema: SchemaKind::ExecutionBinding,
            field: "request_valid_until"
        })
    ));
}

#[test]
fn execution_binding_requires_a_utc_revision_timestamp_when_directly_constructed() {
    let mut binding = valid_binding(CancellationState::NotRequested);
    binding.revision_created_at = timestamp("2026-08-05T01:00:00+01:00");

    assert_eq!(
        binding.validate(),
        Err(ContractError::InvalidShape {
            schema: SchemaKind::ExecutionBinding,
            field: "revision_created_at",
        })
    );
}

#[test]
fn execution_binding_requires_a_utc_revision_timestamp_when_decoded() {
    let mut value = binding_value(CancellationState::NotRequested);
    value["revision_created_at"] = json!("2026-08-05T01:00:00+01:00");

    assert_eq!(
        decode_document(&serde_json::to_vec(&value).unwrap()),
        Err(ContractError::InvalidShape {
            schema: SchemaKind::ExecutionBinding,
            field: "revision_created_at",
        })
    );
}

#[test]
fn foundation_timestamp_fields_parse_and_serialize_as_rfc3339_strings() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let documents = [
        (
            json!({
                "schema_version": "psyche.identity_snapshot.v1",
                "snapshot_id": format!("ids_{ULID_A}"),
                "familiar_id": "familiar:one",
                "principal_id": "principal:one",
                "revision": 1,
                "declaration_digest": digest,
                "identity_file_digest": digest,
                "identity_digest": digest,
                "soul_digest": digest,
                "role_skill_digest": digest,
                "provenance": {
                    "familiar_home_id": "home:one",
                    "resolver_version": "1"
                },
                "resolved_at": "2026-08-01T00:00:00Z"
            }),
            "resolved_at",
        ),
        (
            json!({
                "schema_version": "psyche.approval.v1",
                "approval_id": format!("apr_{ULID_A}"),
                "node_id": format!("nod_{ULID_A}"),
                "requester_principal_id": "principal:one",
                "decision": null,
                "expires_at": "2026-08-01T00:00:00Z"
            }),
            "expires_at",
        ),
        (
            json!({
                "schema_version": "psyche.evidence.v1",
                "evidence_id": format!("evd_{ULID_A}"),
                "node_id": format!("nod_{ULID_A}"),
                "attempt_id": format!("att_{ULID_A}"),
                "content_digest": digest,
                "producer": "test",
                "collection_method": "test",
                "media_type": "text/plain",
                "size": 1,
                "created_at": "2026-08-01T00:00:00Z",
                "retention_policy": "default"
            }),
            "created_at",
        ),
        (
            json!({
                "schema_version": "psyche.verdict.v1",
                "verdict_id": format!("vrd_{ULID_A}"),
                "node_id": format!("nod_{ULID_A}"),
                "sealed_evidence_digest": digest,
                "policy_revision": "policy:one",
                "verdict_type": "review",
                "reviewer_id": "reviewer:one",
                "outcome": "allow",
                "reason_codes": ["verified"],
                "created_at": "2026-08-01T00:00:00Z"
            }),
            "created_at",
        ),
    ];
    for (value, field) in documents {
        let document = decode_document(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(document).unwrap()[field],
            "2026-08-01T00:00:00Z"
        );
    }
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
    for (fixture_name, nullable) in [
        ("intent-local.json", "surface_event_id"),
        ("node-root.json", "delegation_id"),
    ] {
        let value: Value = serde_json::from_slice(&fixture(fixture_name)).unwrap();
        let object = value.as_object().unwrap();
        let null_fields: Vec<_> = object
            .iter()
            .filter_map(|(key, value)| value.is_null().then_some(key.as_str()))
            .collect();
        assert_eq!(null_fields, [nullable], "{fixture_name}");
        assert!(decode_document(&fixture(fixture_name)).is_ok());
    }

    for (fixture_name, required_ids) in [
        (
            "intent-local.json",
            &["intent_id", "familiar_snapshot_id"][..],
        ),
        (
            "node-root.json",
            &["node_id", "graph_id", "familiar_snapshot_id", "budget_id"][..],
        ),
        ("surface-event.json", &["surface_event_id"][..]),
        (
            "surface-effect.json",
            &[
                "surface_effect_id",
                "intent_id",
                "graph_id",
                "node_id",
                "attempt_id",
                "familiar_snapshot_id",
            ][..],
        ),
        ("delivery-ready.json", &["delivery_id", "intent_id"][..]),
    ] {
        for field in required_ids {
            let bytes = mutate(fixture_name, |object| {
                object.insert((*field).into(), Value::Null);
            });
            assert!(decode_document(&bytes).is_err(), "{fixture_name}.{field}");
        }
    }
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
    for field in ["locator", "effect"] {
        let scalar = mutate("surface-effect.json", |o| {
            o.insert(field.into(), json!(false));
        });
        assert!(decode_document(&scalar).is_err(), "{field}");
    }
    let bad_digest = mutate("surface-effect.json", |o| {
        o.insert(
            "effect_digest".into(),
            json!(format!("sha256:{}", "0".repeat(64))),
        );
    });
    assert!(decode_document(&bad_digest).is_err());
    for fixture_name in ["surface-event.json", "surface-effect.json"] {
        let unknown = mutate(fixture_name, |o| {
            o.insert("extra".into(), json!(1));
        });
        assert!(decode_document(&unknown).is_err(), "{fixture_name}");
    }
    for (fixture_name, fields) in [
        ("surface-event.json", &["actor", "locator", "content"][..]),
        ("surface-effect.json", &["locator", "effect"][..]),
    ] {
        for field in fields {
            let oversized = mutate(fixture_name, |o| {
                o.insert(
                    (*field).into(),
                    json!({"nested": {"text": "x".repeat(1_048_577)}}),
                );
            });
            assert!(
                decode_document(&oversized).is_err(),
                "{fixture_name}.{field}"
            );
        }
    }
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
    let bad_decision_state = mutate("delivery-ready.json", |o| {
        o.get_mut("surface_decision")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("state".into(), json!("future"));
    });
    assert!(decode_document(&bad_decision_state).is_err());
    for effect in [
        json!("scalar"),
        json!({}),
        json!({"type": "different"}),
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
    for chat_id in ["", "-", "12x", "1".repeat(33).as_str()] {
        let bytes = mutate("delivery-ready.json", |o| {
            o.insert("chat_id".into(), json!(chat_id));
        });
        assert!(decode_document(&bytes).is_err(), "chat_id={chat_id:?}");
    }
    for message_id in ["", "-1", "12x", "1".repeat(33).as_str()] {
        let bytes = mutate("delivery-ready.json", |o| {
            o.insert("telegram_message_id".into(), json!(message_id));
        });
        assert!(
            decode_document(&bytes).is_err(),
            "telegram_message_id={message_id:?}"
        );
    }
    let sent = mutate("delivery-ready.json", |o| {
        o.insert("state".into(), json!("sent"));
    });
    assert!(decode_document(&sent).is_err());
    let non_sent_with_id = mutate("delivery-ready.json", |o| {
        o.insert("telegram_message_id".into(), json!("314"));
    });
    assert!(decode_document(&non_sent_with_id).is_err());
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
fn error_code_typed_deserialization_is_strict() {
    assert_eq!(
        serde_json::from_str::<ErrorCode>("\"coven_capability_missing\"").unwrap(),
        ErrorCode::CovenCapabilityMissing
    );
    for rejected in [
        "\"\"",
        "\"future_error\"",
        "\"CONFIG_INVALID\"",
        "\"config-invalid\"",
        "null",
        "1",
    ] {
        assert!(
            serde_json::from_str::<ErrorCode>(rejected).is_err(),
            "{rejected}"
        );
    }

    let unknown = json!({
        "schema_version": "psyche.error.v1",
        "error": {
            "code": "future_error",
            "message": "bad",
            "retryable": false,
            "correlation_id": "corr-1",
            "details": {}
        }
    });
    assert!(matches!(
        decode_document(&serde_json::to_vec(&unknown).unwrap()),
        Err(ContractError::UnknownEnumValue {
            schema: SchemaKind::Error,
            field: "code"
        })
    ));
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
        json!({"schema_version":"psyche.error.v1","error":{"code":"","message":"bad","retryable":false,"correlation_id":"c","details":{}}}),
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

#[test]
fn required_error_codes_have_dedicated_success_coverage() {
    for (spelling, expected) in [
        (
            "coven_capability_missing",
            ErrorCode::CovenCapabilityMissing,
        ),
        ("coven_adoption_unknown", ErrorCode::CovenAdoptionUnknown),
        (
            "preview_finalize_blocked",
            ErrorCode::PreviewFinalizeBlocked,
        ),
    ] {
        let envelope = json!({
            "schema_version": "psyche.error.v1",
            "error": {
                "code": spelling,
                "message": "public",
                "retryable": false,
                "correlation_id": "corr-1",
                "details": {"scope": "public"}
            }
        });
        let CanonicalDocument::Error(decoded) =
            decode_document(&serde_json::to_vec(&envelope).unwrap()).unwrap()
        else {
            panic!("expected error envelope");
        };
        assert_eq!(decoded.error.code, expected);
    }
}

#[test]
fn error_public_envelope_bounds_reject_empty_and_oversized_content() {
    let valid = || {
        json!({
            "schema_version": "psyche.error.v1",
            "error": {
                "code": "config_invalid",
                "message": "public",
                "retryable": false,
                "correlation_id": "corr-1",
                "details": {"scope": "public"}
            }
        })
    };
    for (label, mutate_error) in [
        (
            "empty detail key",
            (|error: &mut serde_json::Map<String, Value>| {
                error.insert("details".into(), json!({"": "public"}));
            }) as fn(&mut serde_json::Map<String, Value>),
        ),
        (
            "empty detail value",
            |error: &mut serde_json::Map<String, Value>| {
                error.insert("details".into(), json!({"scope": ""}));
            },
        ),
        (
            "oversized detail key",
            |error: &mut serde_json::Map<String, Value>| {
                error.insert("details".into(), json!({"k".repeat(257): "public"}));
            },
        ),
        (
            "oversized detail value",
            |error: &mut serde_json::Map<String, Value>| {
                error.insert("details".into(), json!({"scope": "v".repeat(4097)}));
            },
        ),
        (
            "oversized message",
            |error: &mut serde_json::Map<String, Value>| {
                error.insert("message".into(), json!("m".repeat(4097)));
            },
        ),
        (
            "oversized correlation id",
            |error: &mut serde_json::Map<String, Value>| {
                error.insert("correlation_id".into(), json!("c".repeat(256)));
            },
        ),
    ] {
        let mut value = valid();
        mutate_error(value.get_mut("error").unwrap().as_object_mut().unwrap());
        assert!(
            decode_document(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{label}"
        );
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
        revision_created_at: timestamp("2026-08-01T00:00:00Z"),
        familiar_snapshot_id: RecordId::parse(
            psyche_core::contracts::RecordKind::IdentitySnapshot,
            &format!("ids_{ULID_B}"),
        )
        .unwrap(),
        project_id: "project:one".into(),
        request_id: request.clone(),
        request_digest: digest_value.clone().try_into().unwrap(),
        request_created_at: timestamp("2026-08-01T00:00:00Z"),
        request_valid_until: timestamp("2026-08-01T00:10:00Z"),
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
            created_at: timestamp("2026-08-01T00:01:00Z"),
            valid_until: timestamp("2026-08-01T00:05:00Z"),
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
            acknowledged_at: timestamp("2026-08-01T00:02:00Z"),
        });
    }
    if state == CancellationState::TerminationUnknown {
        binding.cancellation_unresolved = Some(CancellationUnresolvedEvidence {
            disposition_id: "disp-1".into(),
            termination_request_id: termination,
            session_id: "session-1".into(),
            execution_request_id: request,
            execution_request_digest: digest_value.try_into().unwrap(),
            reason_code: "authority_unreachable".into(),
            recorded_at: timestamp("2026-08-01T00:03:00Z"),
        });
    }
    binding
}

fn assert_cancellation_mismatch(binding: ExecutionBinding, label: &str) {
    assert!(
        matches!(
            CanonicalDocument::ExecutionBinding(binding).validate(),
            Err(ContractError::CancellationEvidenceMismatch)
        ),
        "{label}"
    );
}

fn binding_value(state: CancellationState) -> Value {
    serde_json::to_value(valid_binding(state)).unwrap()
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
fn cancellation_binding_normalizes_every_correlation_and_evidence_failure() {
    let other_request = RequestId::parse(&format!("req_{ULID_C}")).unwrap();
    let other_digest = format!("sha256:{}", "3".repeat(64)).try_into().unwrap();

    let mut wrong_termination = valid_binding(CancellationState::AcknowledgedTerminated);
    wrong_termination
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .termination_request_id = other_request.clone();
    assert_cancellation_mismatch(wrong_termination, "termination request id");

    let mut wrong_execution = valid_binding(CancellationState::AcknowledgedTerminated);
    wrong_execution
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .execution_request_id = other_request.clone();
    assert_cancellation_mismatch(wrong_execution, "execution request id");

    let mut wrong_digest = valid_binding(CancellationState::AcknowledgedTerminated);
    wrong_digest
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .execution_request_digest = other_digest;
    assert_cancellation_mismatch(wrong_digest, "execution digest");

    let mut wrong_session = valid_binding(CancellationState::AcknowledgedTerminated);
    wrong_session
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .session_id = "other".into();
    assert_cancellation_mismatch(wrong_session, "session");

    let mut wrong_kind = valid_binding(CancellationState::AcknowledgedTerminated);
    wrong_kind
        .cancellation_acknowledgement
        .as_mut()
        .unwrap()
        .kind = CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal;
    assert_cancellation_mismatch(wrong_kind, "acknowledgement kind");

    let mut missing_evidence = valid_binding(CancellationState::AcknowledgedTerminated);
    missing_evidence.cancellation_acknowledgement = None;
    assert_cancellation_mismatch(missing_evidence, "missing evidence");

    let mut dual_evidence = valid_binding(CancellationState::AcknowledgedTerminated);
    dual_evidence.cancellation_unresolved =
        valid_binding(CancellationState::TerminationUnknown).cancellation_unresolved;
    assert_cancellation_mismatch(dual_evidence, "dual evidence");

    let mut missing_correlation = valid_binding(CancellationState::AcknowledgedTerminated);
    missing_correlation.termination_request = None;
    assert_cancellation_mismatch(missing_correlation, "missing termination correlation");

    let mut mismatched_correlation = valid_binding(CancellationState::AcknowledgedTerminated);
    mismatched_correlation
        .termination_request
        .as_mut()
        .unwrap()
        .termination_request_id = other_request;
    assert_cancellation_mismatch(mismatched_correlation, "mismatched termination correlation");

    let mut missing_reason = valid_binding(CancellationState::AcknowledgedTerminated);
    missing_reason.termination_reason_code = None;
    assert_cancellation_mismatch(missing_reason, "missing reason");

    let mut unexpected_reason = valid_binding(CancellationState::NotRequested);
    unexpected_reason.termination_reason_code = Some("operator_requested".into());
    assert_cancellation_mismatch(unexpected_reason, "unexpected reason");

    for invalid_reason in ["UPPER", "bad-", "", &"a".repeat(129)] {
        let mut binding = valid_binding(CancellationState::AcknowledgedTerminated);
        binding.termination_reason_code = Some(invalid_reason.into());
        assert_cancellation_mismatch(binding, "invalid reason");
    }

    let mut reused_request = valid_binding(CancellationState::TerminationRequested);
    reused_request
        .termination_request
        .as_mut()
        .unwrap()
        .termination_request_id = reused_request.request_id.clone();
    assert_cancellation_mismatch(reused_request, "reused execution request id");

    let mut invalid_window = valid_binding(CancellationState::AcknowledgedTerminated);
    invalid_window
        .termination_request
        .as_mut()
        .unwrap()
        .valid_until = timestamp("2026-08-01T00:00:59Z");
    assert_cancellation_mismatch(invalid_window, "invalid termination window");

    let mut early_correlation = valid_binding(CancellationState::TerminationRequested);
    early_correlation
        .termination_request
        .as_mut()
        .unwrap()
        .created_at = timestamp("2026-07-31T23:59:59Z");
    assert_cancellation_mismatch(early_correlation, "early termination correlation");

    for (label, at) in [
        ("evidence before window", "2026-08-01T00:00:59Z"),
        ("evidence after window", "2026-08-01T00:05:01Z"),
    ] {
        let mut binding = valid_binding(CancellationState::AcknowledgedTerminated);
        binding
            .cancellation_acknowledgement
            .as_mut()
            .unwrap()
            .acknowledged_at = timestamp(at);
        assert_cancellation_mismatch(binding, label);
    }

    for at in ["2026-08-01T00:01:00Z", "2026-08-01T00:05:00Z"] {
        let mut binding = valid_binding(CancellationState::AcknowledgedTerminated);
        binding
            .cancellation_acknowledgement
            .as_mut()
            .unwrap()
            .acknowledged_at = timestamp(at);
        CanonicalDocument::ExecutionBinding(binding)
            .validate()
            .unwrap();
    }

    for invalid_reason in ["UPPER", "bad-", "", &"a".repeat(129)] {
        let mut binding = valid_binding(CancellationState::TerminationUnknown);
        binding
            .cancellation_unresolved
            .as_mut()
            .unwrap()
            .reason_code = invalid_reason.into();
        assert_cancellation_mismatch(binding, "invalid unresolved reason");
    }
}

#[test]
fn cancellation_binding_decode_maps_nested_wire_failures_to_evidence_mismatch() {
    for (label, mutate_value) in [
        (
            "authority digest",
            (|value: &mut Value| {
                value["cancellation_acknowledgement"]["authority_evidence_digest"] =
                    json!("sha256:not-a-digest");
            }) as fn(&mut Value),
        ),
        ("acknowledgement timestamp", |value: &mut Value| {
            value["cancellation_acknowledgement"]["acknowledged_at"] = json!("tomorrow");
        }),
        ("termination request id", |value: &mut Value| {
            value["termination_request"]["termination_request_id"] = json!("not-a-request");
        }),
        ("execution request id", |value: &mut Value| {
            value["cancellation_acknowledgement"]["execution_request_id"] = json!("not-a-request");
        }),
        ("execution request digest", |value: &mut Value| {
            value["cancellation_acknowledgement"]["execution_request_digest"] =
                json!("not-a-digest");
        }),
        ("evidence session", |value: &mut Value| {
            value["cancellation_acknowledgement"]["session_id"] = json!("");
        }),
        ("termination timestamp", |value: &mut Value| {
            value["termination_request"]["created_at"] = json!("tomorrow");
        }),
        ("termination reason shape", |value: &mut Value| {
            value["termination_reason_code"] = json!(42);
        }),
    ] {
        let mut value = binding_value(CancellationState::AcknowledgedTerminated);
        mutate_value(&mut value);
        assert!(
            matches!(
                decode_document(&serde_json::to_vec(&value).unwrap()),
                Err(ContractError::CancellationEvidenceMismatch)
            ),
            "{label}"
        );
    }

    let mut unknown_kind = binding_value(CancellationState::AcknowledgedTerminated);
    unknown_kind["cancellation_acknowledgement"]["kind"] = json!("future_kind");
    assert!(matches!(
        decode_document(&serde_json::to_vec(&unknown_kind).unwrap()),
        Err(ContractError::UnknownEnumValue {
            schema: SchemaKind::ExecutionBinding,
            field: "cancellation_acknowledgement.kind",
        })
    ));

    let mut unknown = binding_value(CancellationState::TerminationRequested);
    unknown["cancellation_state"] = json!("cancelled");
    assert!(decode_document(&serde_json::to_vec(&unknown).unwrap()).is_err());

    let mut raw_ledger = binding_value(CancellationState::AcknowledgedTerminated);
    raw_ledger["cancellation_acknowledgement"] = Value::Null;
    raw_ledger["terminal_state"] = json!("killed");
    assert!(matches!(
        decode_document(&serde_json::to_vec(&raw_ledger).unwrap()),
        Err(ContractError::CancellationEvidenceMismatch)
    ));
}

#[test]
fn strict_probe_and_document_limit_fail_closed() {
    assert!(matches!(
        decode_document(br#"{"schema_version":"psyche.unknown.v1"}"#),
        Err(ContractError::UnknownSchema)
    ));
    assert!(matches!(
        decode_document(br#"{"schema_version":"psyche.intent.v2"}"#),
        Err(ContractError::UnsupportedMajor { .. })
    ));
    assert!(decode_document(&vec![b' '; 1_048_577]).is_err());
}

#[test]
fn duplicate_schema_versions_are_rejected_before_version_dispatch() {
    for (order, bytes) in [
        (
            "unsupported then supported",
            replace_fixture_once(
                "intent-local.json",
                r#""schema_version": "psyche.intent.v1""#,
                r#""schema_version": "psyche.intent.v2", "schema_version": "psyche.intent.v1""#,
            ),
        ),
        (
            "supported then unsupported",
            replace_fixture_once(
                "intent-local.json",
                r#""schema_version": "psyche.intent.v1""#,
                r#""schema_version": "psyche.intent.v1", "schema_version": "psyche.intent.v2""#,
            ),
        ),
    ] {
        assert_duplicate_json_rejected(&bytes, order);
    }
}

#[test]
fn duplicate_top_level_contract_fields_are_rejected() {
    for (field, bytes) in [
        (
            "intent_id",
            duplicate_fixture_fragment(
                "intent-local.json",
                r#""intent_id": "int_01ARZ3NDEKTSV4RRFFQ69G5FAV""#,
            ),
        ),
        (
            "digest",
            duplicate_fixture_fragment(
                "intent-local.json",
                r#""digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef""#,
            ),
        ),
    ] {
        assert_duplicate_json_rejected(&bytes, field);
    }
}

#[test]
fn duplicate_nested_contract_fields_are_rejected_recursively() {
    for (location, bytes) in [
        (
            "intent constraints",
            replace_fixture_once(
                "intent-local.json",
                r#""constraints": {}"#,
                r#""constraints": {"outer": {"mode": "strict", "mode": "strict"}}"#,
            ),
        ),
        (
            "surface actor",
            duplicate_fixture_fragment("surface-event.json", r#""type": "user""#),
        ),
        (
            "surface locator",
            duplicate_fixture_fragment("surface-event.json", r#""message_id": "42""#),
        ),
        (
            "surface content",
            duplicate_fixture_fragment("surface-event.json", r#""text": "Please review this.""#),
        ),
        (
            "surface effect locator",
            duplicate_fixture_fragment("surface-effect.json", r#""chat_id": "-100123""#),
        ),
        (
            "surface effect",
            duplicate_fixture_fragment("surface-effect.json", r#""text": "Review complete.""#),
        ),
        (
            "delivery topic",
            duplicate_fixture_fragment("delivery-ready.json", r#""kind": "forum""#),
        ),
        (
            "delivery effect",
            duplicate_fixture_fragment("delivery-ready.json", r#""format": "html""#),
        ),
        (
            "delivery surface_decision",
            duplicate_fixture_fragment(
                "delivery-ready.json",
                r#""policy_revision": "policy:sha256:0123456789abcdef""#,
            ),
        ),
        (
            "error details",
            br#"{
                "schema_version": "psyche.error.v1",
                "error": {
                    "code": "storage_unavailable",
                    "message": "temporarily unavailable",
                    "retryable": true,
                    "correlation_id": "corr-1",
                    "details": {"scope": "public", "scope": "public"}
                }
            }"#
            .to_vec(),
        ),
    ] {
        assert_duplicate_json_rejected(&bytes, location);
    }
}

#[test]
fn duplicate_key_errors_do_not_expose_attacker_controlled_text() {
    let marker = "SENTINEL_DUPLICATE_KEY_XYZ";
    let bytes = replace_fixture_once(
        "intent-local.json",
        r#""constraints": {}"#,
        &format!(r#""constraints": {{"{marker}": 1, "{marker}": 2}}"#),
    );

    let error = decode_document(&bytes).unwrap_err();
    assert_eq!(
        error,
        ContractError::InvalidShape {
            schema: SchemaKind::Error,
            field: "json",
        }
    );
    assert!(!format!("{error:?}").contains(marker));
    assert!(!format!("{error}").contains(marker));
}

#[test]
fn typed_deserialization_rejects_duplicate_keys_in_embedded_json_values() {
    let intent = replace_fixture_once(
        "intent-local.json",
        r#""constraints": {}"#,
        r#""constraints": {"scope": "public", "scope": "public"}"#,
    );
    assert!(serde_json::from_slice::<Intent>(&intent).is_err());

    let binding =
        serde_json::to_string(&valid_binding(CancellationState::AcknowledgedTerminated)).unwrap();
    let session = r#""session_id":"session-1""#;
    assert_eq!(binding.matches(session).count(), 1);
    let binding = binding.replacen(session, &format!("{session},{session}"), 1);
    assert!(serde_json::from_str::<ExecutionBinding>(&binding).is_err());
}

#[test]
fn directly_constructed_document_rejects_oversized_canonical_bytes() {
    let CanonicalDocument::Intent(mut intent) = decode("intent-local.json") else {
        panic!("expected intent");
    };
    intent.constraints.insert(
        "payload".into(),
        Value::String("x".repeat(psyche_core::contracts::MAX_DOCUMENT_BYTES - 128)),
    );
    assert!(matches!(
        CanonicalDocument::Intent(intent).validate(),
        Err(ContractError::DocumentTooLarge)
    ));
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
fn typed_u64_fields_accept_the_safe_boundary_and_reject_one_over() {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    const ONE_OVER: u64 = MAX_SAFE_INTEGER + 1;
    let digest = format!("sha256:{}", "a".repeat(64));

    let mut identity: IdentitySnapshot = serde_json::from_value(json!({
        "schema_version": "psyche.identity_snapshot.v1",
        "snapshot_id": format!("ids_{ULID_A}"),
        "familiar_id": "familiar:one",
        "principal_id": "principal:one",
        "revision": 1,
        "declaration_digest": digest,
        "identity_file_digest": digest,
        "identity_digest": digest,
        "soul_digest": digest,
        "role_skill_digest": digest,
        "provenance": {"familiar_home_id": "home:one", "resolver_version": "1"},
        "resolved_at": "2026-08-01T00:00:00Z"
    }))
    .unwrap();
    identity.revision = MAX_SAFE_INTEGER;
    identity.validate().unwrap();
    identity.revision = ONE_OVER;
    assert_invalid_numeric_field(
        identity.validate(),
        SchemaKind::IdentitySnapshot,
        "revision",
    );

    let mut graph: Graph = serde_json::from_value(json!({
        "schema_version": "psyche.graph.v1",
        "graph_id": format!("grf_{ULID_A}"),
        "root_intent_id": format!("int_{ULID_A}"),
        "owner_principal_id": "principal:one",
        "policy_revision": "policy:one",
        "state": "draft",
        "version": 1
    }))
    .unwrap();
    graph.version = MAX_SAFE_INTEGER;
    graph.validate().unwrap();
    graph.version = ONE_OVER;
    assert_invalid_numeric_field(graph.validate(), SchemaKind::Graph, "version");

    let CanonicalDocument::GraphNode(mut node) = decode("node-root.json") else {
        panic!("expected graph node");
    };
    let _: &GraphNode = &node;
    node.version = MAX_SAFE_INTEGER;
    node.validate().unwrap();
    node.version = ONE_OVER;
    assert_invalid_numeric_field(node.validate(), SchemaKind::GraphNode, "version");

    let mut budget: Budget = serde_json::from_value(json!({
        "schema_version": "psyche.budget.v1",
        "budget_id": format!("bud_{ULID_A}"),
        "graph_id": format!("grf_{ULID_A}"),
        "resource_class": "tokens",
        "limit": 1,
        "reserved": 1,
        "consumed": 1,
        "released": 1
    }))
    .unwrap();
    budget.limit = MAX_SAFE_INTEGER;
    budget.reserved = MAX_SAFE_INTEGER;
    budget.consumed = MAX_SAFE_INTEGER;
    budget.released = MAX_SAFE_INTEGER;
    budget.validate().unwrap();
    for field in ["limit", "reserved", "consumed", "released"] {
        let mut unsafe_budget = budget.clone();
        match field {
            "limit" => unsafe_budget.limit = ONE_OVER,
            "reserved" => unsafe_budget.reserved = ONE_OVER,
            "consumed" => unsafe_budget.consumed = ONE_OVER,
            "released" => unsafe_budget.released = ONE_OVER,
            _ => unreachable!(),
        }
        assert_invalid_numeric_field(unsafe_budget.validate(), SchemaKind::Budget, field);
    }

    let mut evidence: Evidence = serde_json::from_value(json!({
        "schema_version": "psyche.evidence.v1",
        "evidence_id": format!("evd_{ULID_A}"),
        "node_id": format!("nod_{ULID_A}"),
        "attempt_id": format!("att_{ULID_A}"),
        "content_digest": digest,
        "producer": "test",
        "collection_method": "test",
        "media_type": "text/plain",
        "size": 1,
        "created_at": "2026-08-01T00:00:00Z",
        "retention_policy": "default"
    }))
    .unwrap();
    evidence.size = MAX_SAFE_INTEGER;
    evidence.validate().unwrap();
    evidence.size = ONE_OVER;
    assert_invalid_numeric_field(evidence.validate(), SchemaKind::Evidence, "size");

    let mut recovery: Recovery = serde_json::from_value(json!({
        "schema_version": "psyche.recovery.v1",
        "recovery_id": format!("rcv_{ULID_A}"),
        "attempt_id": format!("att_{ULID_A}"),
        "lease_id": "lease:one",
        "fence_token": null,
        "ambiguity": "none",
        "reconciliation_count": 1,
        "operator_disposition": null
    }))
    .unwrap();
    recovery.reconciliation_count = MAX_SAFE_INTEGER;
    recovery.validate().unwrap();
    recovery.reconciliation_count = ONE_OVER;
    assert_invalid_numeric_field(
        recovery.validate(),
        SchemaKind::Recovery,
        "reconciliation_count",
    );

    let mut binding = valid_binding(CancellationState::NotRequested);
    binding.previous_revision_digest = Some(digest.try_into().unwrap());
    binding.revision = MAX_SAFE_INTEGER;
    binding.validate().unwrap();
    binding.revision = ONE_OVER;
    assert_invalid_numeric_field(binding.validate(), SchemaKind::ExecutionBinding, "revision");
}

#[test]
fn typed_u32_delivery_fields_remain_within_the_safe_integer_domain() {
    let CanonicalDocument::Delivery(mut delivery) = decode("delivery-ready.json") else {
        panic!("expected delivery");
    };
    delivery.logical_part = u32::MAX;
    delivery.attempt_count = u32::MAX;

    delivery.validate().unwrap();
}

#[test]
fn canonical_document_validation_rejects_nested_unsafe_integers() {
    let CanonicalDocument::Intent(mut intent) = decode("intent-local.json") else {
        panic!("expected intent");
    };
    intent.constraints.insert(
        "nested".into(),
        json!({"array": [9_007_199_254_740_992_u64]}),
    );

    assert_eq!(
        CanonicalDocument::Intent(intent).validate(),
        Err(ContractError::NonInteroperableNumber)
    );
}

#[test]
fn decoded_document_rejects_nested_unsafe_integers() {
    let bytes = mutate("surface-event.json", |object| {
        object.insert(
            "content".into(),
            json!({"nested": [9_007_199_254_740_992_u64]}),
        );
    });

    assert_eq!(
        decode_document(&bytes),
        Err(ContractError::NonInteroperableNumber)
    );
}

#[test]
fn decoded_and_direct_values_preserve_interoperable_arbitrary_precision_numbers() {
    let bytes = replace_fixture_once(
        "intent-local.json",
        r#""constraints": {}"#,
        r#""constraints": {"safe": 9007199254740991, "fraction": 1.2300, "exponent": 1e3}"#,
    );
    let direct: Value =
        serde_json::from_str(r#"{"safe":9007199254740991,"fraction":1.2300,"exponent":1e3}"#)
            .unwrap();

    let CanonicalDocument::Intent(intent) = decode_document(&bytes).unwrap() else {
        panic!("expected intent");
    };
    assert_eq!(
        canonical_bytes(&intent.constraints).unwrap(),
        br#"{"exponent":1000,"fraction":1.23,"safe":9007199254740991}"#
    );
    assert_eq!(
        canonical_bytes(&intent.constraints).unwrap(),
        canonical_bytes(&direct).unwrap()
    );
}

#[test]
fn decoded_private_number_marker_lookalikes_remain_objects() {
    for (constraints, expected) in [
        (
            r#"{"$serde_json::private::Number": "1.5"}"#,
            r#"{"$serde_json::private::Number":"1.5"}"#,
        ),
        (
            r#"{"$serde_json::private::Number": 1.5}"#,
            r#"{"$serde_json::private::Number":1.5}"#,
        ),
        (
            r#"{"$serde_json::private::Number": "1.5", "extra": true}"#,
            r#"{"$serde_json::private::Number":"1.5","extra":true}"#,
        ),
    ] {
        let bytes = replace_fixture_once(
            "intent-local.json",
            r#""constraints": {}"#,
            &format!(r#""constraints": {constraints}"#),
        );
        let CanonicalDocument::Intent(intent) = decode_document(&bytes).unwrap() else {
            panic!("expected intent");
        };
        assert_eq!(
            canonical_bytes(&intent.constraints).unwrap(),
            expected.as_bytes()
        );
    }
}

#[test]
fn arbitrary_precision_effect_digests_are_stable_across_decode() {
    let effect: Value =
        serde_json::from_str(r#"{"safe":9007199254740991,"fraction":1.2300}"#).unwrap();
    let expected_digest = digest(&effect).unwrap();
    let bytes = mutate("surface-effect.json", |object| {
        object.insert("effect".into(), effect.clone());
        object.insert("effect_digest".into(), json!(expected_digest.as_str()));
    });

    let CanonicalDocument::SurfaceEffect(decoded) = decode_document(&bytes).unwrap() else {
        panic!("expected surface effect");
    };
    assert_eq!(digest(&decoded.effect).unwrap(), expected_digest);
    assert_eq!(
        canonical_bytes(&decoded.effect).unwrap(),
        canonical_bytes(&effect).unwrap()
    );
}

fn assert_invalid_numeric_field(
    result: Result<(), ContractError>,
    schema: SchemaKind,
    field: &'static str,
) {
    assert_eq!(result, Err(ContractError::InvalidShape { schema, field }));
}

#[test]
fn typed_deserialization_cannot_bypass_validation() {
    let wrong_id = mutate("intent-local.json", |object| {
        object.insert("intent_id".into(), json!(format!("grf_{ULID_A}")));
    });
    assert!(serde_json::from_slice::<Intent>(&wrong_id).is_err());

    let mismatched_digest = mutate("surface-effect.json", |object| {
        object.insert(
            "effect_digest".into(),
            json!(format!("sha256:{}", "0".repeat(64))),
        );
    });
    assert!(serde_json::from_slice::<SurfaceEffect>(&mismatched_digest).is_err());
}

// Attacker-input-redaction tests via decode_document: nearly-1-MiB schema
// strings must not appear in ContractError Debug or Display output.
#[test]
fn decode_document_unknown_schema_does_not_expose_attacker_input() {
    let marker = "SENTINEL_DECODE_UNKNOWN";
    let version = format!("psyche.{}{}.v1", marker, "a".repeat(900_000));
    let json = format!(r#"{{"schema_version":{version:?}}}"#);
    let err = decode_document(json.as_bytes()).unwrap_err();
    let debug = format!("{err:?}");
    let display = format!("{err}");
    assert!(
        !debug.contains(marker),
        "Debug must not contain attacker marker (output len = {})",
        debug.len()
    );
    assert!(
        !display.contains(marker),
        "Display must not contain attacker marker (output len = {})",
        display.len()
    );
    assert!(debug.len() < 256);
    assert!(display.len() < 256);
}

#[test]
fn decode_document_unsupported_major_does_not_expose_attacker_input() {
    let marker = "SENTINEL_DECODE_MAJOR";
    // Known kind, non-digit marker in major segment → UnsupportedMajor
    let version = format!("psyche.intent.v{}{}", marker, "9".repeat(900_000));
    let json = format!(r#"{{"schema_version":{version:?}}}"#);
    let err = decode_document(json.as_bytes()).unwrap_err();
    let debug = format!("{err:?}");
    let display = format!("{err}");
    assert!(
        !debug.contains(marker),
        "Debug must not contain attacker marker (output len = {})",
        debug.len()
    );
    assert!(
        !display.contains(marker),
        "Display must not contain attacker marker (output len = {})",
        display.len()
    );
    assert!(debug.len() < 256);
    assert!(display.len() < 256);
}
