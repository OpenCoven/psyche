//! Fail-closed canonical document decoding and quarantine-input tests.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use psyche_core::contracts::{
    CanonicalDocument, ContractError, MAX_DOCUMENT_BYTES, RejectedDocument, RejectionReason,
    SchemaKind, decode_document,
};
use serde_json::{Value, json};

const ULID_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ULID_B: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";
const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn assert_redacted(error: &ContractError, rejected_value: &str) {
    assert!(!format!("{error:?}").contains(rejected_value));
    assert!(!error.to_string().contains(rejected_value));
}

fn graph() -> Value {
    json!({
        "schema_version": "psyche.graph.v1",
        "graph_id": format!("grf_{ULID_A}"),
        "root_intent_id": format!("int_{ULID_A}"),
        "owner_principal_id": "principal:one",
        "policy_revision": "policy:one",
        "state": "draft",
        "version": 1
    })
}

fn execution_binding() -> Value {
    json!({
        "schema_version": "psyche.execution_binding.v1",
        "attempt_id": format!("att_{ULID_A}"),
        "revision": 1,
        "previous_revision_digest": null,
        "revision_created_at": "2026-08-01T00:00:00Z",
        "familiar_snapshot_id": format!("ids_{ULID_B}"),
        "project_id": "project:one",
        "request_id": format!("req_{ULID_A}"),
        "request_digest": DIGEST,
        "request_created_at": "2026-08-01T00:00:00Z",
        "request_valid_until": "2026-08-01T00:10:00Z",
        "coven_contract_version": "coven.execution.v1",
        "coven_session_id": null,
        "adoption_state": "not_submitted",
        "event_cursor": null,
        "cancellation_state": "not_requested",
        "termination_request": null,
        "termination_reason_code": null,
        "cancellation_acknowledgement": null,
        "cancellation_unresolved": null,
        "terminal_state": null
    })
}

#[test]
fn unknown_major_never_decodes_as_a_known_record() {
    let secret = "intent-secret-sentinel";
    let bytes = format!(
        r#"{{"schema_version":"psyche.intent.v2","intent_id":"{secret}","raw":"{secret}"}}"#
    );
    let error = decode_document(bytes.as_bytes()).unwrap_err();
    assert_eq!(
        error,
        ContractError::UnsupportedMajor {
            found: 2,
            supported: 1,
        }
    );
    assert_redacted(&error, secret);
}

#[test]
fn malformed_payload_is_bounded_before_quarantine() {
    let bytes = vec![b'x'; MAX_DOCUMENT_BYTES + 1];
    assert_eq!(
        decode_document(&bytes),
        Err(ContractError::DocumentTooLarge)
    );
    let rejected = RejectedDocument::from_bytes(&bytes, RejectionReason::TooLarge);
    assert_eq!(rejected.bounded_payload.len(), 64 * 1024);
}

#[test]
fn recognized_error_envelope_decodes_exhaustively() {
    let document =
        decode_document(&fixture("error-storage-unavailable.json")).expect("error fixture");
    assert!(matches!(document, CanonicalDocument::Error(_)));
    assert_eq!(document.schema_version().kind, SchemaKind::Error);
}

#[test]
fn error_non_string_details_is_invalid_shape() {
    let mut value: Value =
        serde_json::from_slice(&fixture("error-storage-unavailable.json")).unwrap();
    value["error"]["details"]["attempt"] = json!(3);
    assert!(matches!(
        decode_document(&serde_json::to_vec(&value).unwrap()),
        Err(ContractError::InvalidShape {
            schema: SchemaKind::Error,
            ..
        })
    ));
}

#[test]
fn error_unknown_field_is_invalid_shape() {
    let mut value: Value =
        serde_json::from_slice(&fixture("error-storage-unavailable.json")).unwrap();
    value["error"]["secret"] = json!("must-not-leak");
    assert!(matches!(
        decode_document(&serde_json::to_vec(&value).unwrap()),
        Err(ContractError::InvalidShape {
            schema: SchemaKind::Error,
            ..
        })
    ));
}

#[test]
fn unknown_typed_enum_is_a_quarantinable_decode_failure() {
    let rejected_value = "future_state_secret";
    let mut value = graph();
    value["state"] = json!(rejected_value);
    let error = decode_document(&serde_json::to_vec(&value).unwrap()).unwrap_err();
    assert_eq!(
        error,
        ContractError::UnknownEnumValue {
            schema: SchemaKind::Graph,
            field: "state",
        }
    );
    assert_redacted(&error, rejected_value);
}

#[test]
fn every_typed_enum_reports_its_static_field_without_the_rejected_value() {
    let rejected = "future_enum_secret";
    let mut cases = Vec::new();

    let mut node: Value = serde_json::from_slice(&fixture("node-root.json")).unwrap();
    node["state"] = json!(rejected);
    cases.push((node, SchemaKind::GraphNode, "state"));

    let mut adoption = execution_binding();
    adoption["adoption_state"] = json!(rejected);
    cases.push((adoption, SchemaKind::ExecutionBinding, "adoption_state"));

    let mut cancellation = execution_binding();
    cancellation["cancellation_state"] = json!(rejected);
    cases.push((
        cancellation,
        SchemaKind::ExecutionBinding,
        "cancellation_state",
    ));

    let mut acknowledgement = execution_binding();
    acknowledgement["cancellation_acknowledgement"] = json!({"kind": rejected});
    cases.push((
        acknowledgement,
        SchemaKind::ExecutionBinding,
        "cancellation_acknowledgement.kind",
    ));

    let delivery: Value = serde_json::from_slice(&fixture("delivery-ready.json")).unwrap();
    for (field, expected) in [
        ("relationship", "relationship"),
        ("state", "state"),
        ("surface_decision.state", "surface_decision.state"),
    ] {
        let mut value = delivery.clone();
        if field == "surface_decision.state" {
            value["surface_decision"]["state"] = json!(rejected);
        } else {
            value[field] = json!(rejected);
        }
        cases.push((value, SchemaKind::Delivery, expected));
    }

    let mut error: Value =
        serde_json::from_slice(&fixture("error-storage-unavailable.json")).unwrap();
    error["error"]["code"] = json!(rejected);
    cases.push((error, SchemaKind::Error, "code"));

    for (value, schema, field) in cases {
        let error = decode_document(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert_eq!(
            error,
            ContractError::UnknownEnumValue { schema, field },
            "{schema:?}.{field}"
        );
        assert_redacted(&error, rejected);
    }
}

#[test]
fn duplicate_keys_at_every_recursive_location_fail_closed() {
    let sentinel = "duplicate-secret-sentinel";
    let cases = [
        format!(
            r#"{{"schema_version":"psyche.intent.v1","schema_version":"psyche.intent.v2","raw":"{sentinel}"}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.intent.v2","schema_version":"psyche.intent.v1","raw":"{sentinel}"}}"#
        ),
        format!(r#"{{"schema_version":"psyche.intent.v1","raw":"{sentinel}","raw":"second"}}"#),
        format!(
            r#"{{"schema_version":"psyche.intent.v1","constraints":{{"nested":{{"key":"{sentinel}","key":"second"}}}}}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.surface_event.v1","actor":{{"key":"{sentinel}","key":"second"}}}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.surface_event.v1","locator":{{"key":"{sentinel}","key":"second"}}}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.surface_event.v1","content":{{"key":"{sentinel}","key":"second"}}}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.surface_effect.v1","effect":{{"key":"{sentinel}","key":"second"}}}}"#
        ),
        format!(
            r#"{{"schema_version":"psyche.surface_event.v1","content":{{"items":[{{"key":"{sentinel}","key":"second"}}]}}}}"#
        ),
    ];

    for bytes in cases {
        let error = decode_document(bytes.as_bytes()).unwrap_err();
        assert!(matches!(
            error,
            ContractError::InvalidShape {
                schema: SchemaKind::Error,
                ..
            }
        ));
        assert_redacted(&error, sentinel);
    }
}

#[test]
fn excessive_json_nesting_is_rejected_without_echoing_payload() {
    let sentinel = "depth-secret-sentinel";
    let depth = 80;
    let bytes = format!(
        r#"{{"schema_version":"psyche.intent.v1","constraints":{}"{sentinel}"{}}}}}"#,
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let error = decode_document(bytes.as_bytes()).unwrap_err();
    assert!(matches!(error, ContractError::InvalidShape { .. }));
    assert_redacted(&error, sentinel);
}

#[test]
fn recognized_registry_entries_never_report_unknown_or_unsupported() {
    for schema in [
        "identity_snapshot",
        "intent",
        "surface_event",
        "graph",
        "graph_node",
        "delegation",
        "budget",
        "approval",
        "execution_binding",
        "evidence",
        "verdict",
        "recovery",
        "addon",
        "surface_effect",
        "delivery",
        "error",
    ] {
        let bytes = format!(r#"{{"schema_version":"psyche.{schema}.v1"}}"#);
        let error = decode_document(bytes.as_bytes()).unwrap_err();
        assert!(
            !matches!(
                error,
                ContractError::UnknownSchema | ContractError::UnsupportedMajor { .. }
            ),
            "{schema} fell through the registry: {error:?}"
        );
    }
}

#[test]
fn rejected_document_hashes_full_raw_bytes_and_bounds_retained_payload() {
    let small = RejectedDocument::from_bytes(
        b"abc",
        RejectionReason::InvalidShape {
            schema: SchemaKind::Error,
            field: "json",
        },
    );
    assert_eq!(small.bounded_payload, b"abc");
    assert_eq!(
        small.payload_digest.as_str(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let mut left = vec![b'a'; 64 * 1024 + 1];
    let mut right = left.clone();
    left[64 * 1024] = b'x';
    right[64 * 1024] = b'y';

    let left = RejectedDocument::from_bytes(
        &left,
        RejectionReason::InvalidShape {
            schema: SchemaKind::Intent,
            field: "document",
        },
    );
    let right = RejectedDocument::from_bytes(
        &right,
        RejectionReason::InvalidShape {
            schema: SchemaKind::Intent,
            field: "document",
        },
    );

    assert_eq!(left.bounded_payload.len(), 64 * 1024);
    assert_eq!(right.bounded_payload.len(), 64 * 1024);
    assert_eq!(left.bounded_payload, right.bounded_payload);
    assert_ne!(left.payload_digest, right.payload_digest);
}

#[test]
fn rejected_document_debug_never_prints_payload_or_schema_values() {
    let sentinel = "SECRET_DEBUG_SENTINEL";
    let bytes = format!(r#"{{"schema_version":"psyche.intent.v2","payload":"{sentinel}"}}"#);
    let rejected = RejectedDocument::from_bytes(
        bytes.as_bytes(),
        RejectionReason::UnsupportedMajor {
            found: 2,
            supported: 1,
        },
    );
    assert_eq!(rejected.schema_version.as_deref(), Some("psyche.intent.v2"));
    let debug = format!("{rejected:?}");
    assert!(!debug.contains(sentinel));
    assert!(!debug.contains("psyche.intent.v2"));
    assert!(!debug.contains("\"payload\""));
    assert!(debug.contains("bounded_payload_bytes"));
}

#[test]
fn rejected_document_handles_oversized_and_invalid_utf8_input() {
    let oversized = vec![0xff; MAX_DOCUMENT_BYTES + 1];
    let rejected = RejectedDocument::from_bytes(&oversized, RejectionReason::TooLarge);
    assert_eq!(rejected.bounded_payload.len(), 64 * 1024);
    assert_eq!(rejected.bounded_payload, vec![0xff; 64 * 1024]);
    assert!(rejected.schema_version.is_none());
    assert!(rejected.payload_digest.as_str().starts_with("sha256:"));
    assert_eq!(rejected.payload_digest.as_str().len(), 71);
    assert!(!format!("{rejected:?}").contains('\u{fffd}'));
}
