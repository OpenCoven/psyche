//! `psyche.error.v1`, `experimental` in #60. Never stored, and no producer
//! row covers it. It may become the serialized rejection result (#60 slice
//! 3), which is undecided.

use psyche_core::contracts::{ContractError, SchemaKind};
use serde_json::{Map, Value, json};

use crate::schema_parity::{Case, Expect, Kind, replace_once};

const KIND: Kind = Kind {
    name: "error",
    schema: SchemaKind::Error,
    pinned_sha256: "9b33c0f2d972ce9496ff2c4d02938712572ac68b87eb2a54c6d375b5a3035f97",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:error"],
};

const CODES: [&str; 36] = [
    "config_invalid",
    "secret_unavailable",
    "telegram_unauthorized",
    "telegram_bot_identity_mismatch",
    "telegram_conflict",
    "telegram_rate_limited",
    "telegram_unavailable",
    "webhook_auth_failed",
    "storage_unavailable",
    "event_schema_unsupported",
    "principal_mapping_invalid",
    "graph_invalid",
    "delegation_widened",
    "budget_unenforceable",
    "evidence_incomplete",
    "verdict_invalid",
    "route_not_found",
    "route_ambiguous",
    "sender_unauthorized",
    "identity_invalid",
    "identity_changed",
    "coven_unavailable",
    "coven_version_unsupported",
    "coven_capability_missing",
    "coven_policy_denied",
    "coven_execution_binding_invalid",
    "coven_binding_mismatch",
    "coven_artifact_rejected",
    "coven_intent_conflict",
    "coven_adoption_unknown",
    "coven_cancellation_unknown",
    "coven_session_failed",
    "delivery_unknown",
    "preview_finalize_blocked",
    "media_rejected",
    "callback_invalid",
];

fn details(entries: impl IntoIterator<Item = (String, Value)>) -> Vec<u8> {
    KIND.with_at(
        &["error", "details"],
        Value::Object(entries.into_iter().collect::<Map<_, _>>()),
    )
}

#[allow(clippy::too_many_lines)]
fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let unknown_code = ContractError::UnknownEnumValue {
        schema: SchemaKind::Error,
        field: "code",
    };
    let mut cases = KIND.standard_cases(&[(&["error"], &[], document.clone())]);
    cases.extend(KIND.bounded_string_cases_with(
        &["error", "message"],
        4096,
        &KIND.invalid("message"),
        &document,
    ));
    cases.extend(KIND.bounded_string_cases_with(
        &["error", "correlation_id"],
        255,
        &KIND.invalid("correlation_id"),
        &document,
    ));
    cases.extend([
        Case::new(
            "error_null",
            KIND.with("error", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "code_unknown",
            KIND.with_at(&["error", "code"], json!("storage_down")),
            Reject(unknown_code.clone()),
        ),
        Case::new(
            "code_uppercase",
            KIND.with_at(&["error", "code"], json!("STORAGE_UNAVAILABLE")),
            Reject(unknown_code),
        ),
        Case::new(
            "code_number",
            KIND.with_at(&["error", "code"], json!(9)),
            Reject(document.clone()),
        ),
        Case::new(
            "retryable_false",
            KIND.with_at(&["error", "retryable"], json!(false)),
            Accept,
        ),
        Case::new(
            "retryable_string",
            KIND.with_at(&["error", "retryable"], json!("true")),
            Reject(document.clone()),
        ),
        Case::new(
            "retryable_null",
            KIND.with_at(&["error", "retryable"], Value::Null),
            Reject(document.clone()),
        ),
        Case::new("details_empty", details([]), Accept),
        Case::new(
            "details_128_entries",
            details((0..128).map(|i| (format!("k{i}"), json!("v")))),
            Accept,
        ),
        Case::new(
            "details_129_entries",
            details((0..129).map(|i| (format!("k{i}"), json!("v")))),
            Reject(KIND.invalid("details")),
        ),
        Case::new(
            "details_key_256_bytes",
            details([("k".repeat(256), json!("v"))]),
            Accept,
        ),
        Case::new(
            "details_key_257_bytes",
            details([("k".repeat(257), json!("v"))]),
            Reject(KIND.invalid("details.key")),
        ),
        Case::new(
            "details_key_empty",
            details([(String::new(), json!("v"))]),
            Reject(KIND.invalid("details.key")),
        ),
        Case::new(
            "details_value_4096_bytes",
            details([("k".into(), json!("v".repeat(4096)))]),
            Accept,
        ),
        Case::new(
            "details_value_4097_bytes",
            details([("k".into(), json!("v".repeat(4097)))]),
            Reject(KIND.invalid("details.value")),
        ),
        Case::new(
            "details_value_empty",
            details([("k".into(), json!(""))]),
            Reject(KIND.invalid("details.value")),
        ),
        Case::new(
            "details_value_number",
            details([("k".into(), json!(1))]),
            Reject(document.clone()),
        ),
        Case::new(
            "details_value_null",
            details([("k".into(), Value::Null)]),
            Reject(document.clone()),
        ),
        Case::new(
            "details_array",
            KIND.with_at(&["error", "details"], json!([])),
            Reject(document.clone()),
        ),
        // --- parser-only -----------------------------------------------------
        Case::new(
            "details_key_multibyte_over_256_bytes",
            details([("\u{e9}".repeat(129), json!("v"))]),
            ParserOnly(KIND.invalid("details.key")),
        ),
        Case::new(
            "details_value_multibyte_over_4096_bytes",
            details([("k".into(), json!("\u{e9}".repeat(2049)))]),
            ParserOnly(KIND.invalid("details.value")),
        ),
        Case::new(
            "duplicate_details_key",
            replace_once(
                &KIND.vector(),
                "{\"component\"",
                "{\"operation\":\"write\",\"component\"",
            ),
            ParserOnly(crate::schema_parity::json_error()),
        ),
    ]);
    cases.push(Case::new(
        "error_as_positional_array",
        KIND.as_positional_array(
            &["error"],
            &["code", "correlation_id", "retryable", "message", "details"],
        ),
        Expect::DecoderOnly,
    ));
    cases
}

parity_tests!(sweep: [(["error", "code"], true)], min_sweep: 2_000);

#[test]
fn the_schema_names_exactly_the_error_body_members() {
    KIND.check_nested_members(&["error"], &KIND.vector(), &[]);
}

#[test]
fn the_code_enum_matches_the_decoder() {
    KIND.check_enum(&["error", "code"], &CODES);
}
