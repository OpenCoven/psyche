//! `psyche.delivery.v1`, excluded from the first profile by #60 (row 13 is
//! held). The schema describes the current decoder only.

use psyche_core::contracts::{ContractError, SchemaKind};
use psyche_core::digest::digest;
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind, positional};

const KIND: Kind = Kind {
    name: "delivery",
    schema: SchemaKind::Delivery,
    pinned_sha256: "6ed2946ff8b51396c6c4bead2ccf7768256e201cca0436be6697d026a5a1ca54",
    optional_members: &["telegram_message_id"],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:delivery"],
};

const STATES: [&str; 10] = [
    "ready",
    "sending",
    "sent",
    "retryable",
    "delivery_unknown",
    "failed",
    "abandoned",
    "dead_letter",
    "resolving_unknown",
    "compensated",
];

const RELATIONSHIPS: [&str; 5] = [
    "reply_same_dm",
    "reply_same_group",
    "reply_same_topic",
    "cross_chat",
    "broadcast",
];

fn unknown(field: &'static str) -> ContractError {
    ContractError::UnknownEnumValue {
        schema: SchemaKind::Delivery,
        field,
    }
}

fn with_effect(effect: Value) -> Vec<u8> {
    let digest = json!(digest(&effect).unwrap().as_str());
    KIND.with_all(&[(&["effect"], effect), (&["effect_digest"], digest)])
}

/// Not yet sent: no Telegram message ID.
fn unsent(state: &str) -> Vec<u8> {
    KIND.with_all(&[
        (&["state"], json!(state)),
        (&["telegram_message_id"], Value::Null),
    ])
}

#[allow(clippy::too_many_lines)]
fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[
        (&["topic"], &[], document.clone()),
        (&["surface_decision"], &[], document.clone()),
    ]);
    cases.extend(KIND.id_cases(&["delivery_id"], "del_", "sfx_", &document, &document));
    cases.extend(KIND.id_cases(&["intent_id"], "int_", "del_", &document, &document));
    for path in [
        &["action_class"][..],
        &["account_id"],
        &["logical_response_id"],
        &["topic", "kind"],
        &["topic", "id"],
        &["surface_decision", "decision_id"],
        &["surface_decision", "policy_revision"],
    ] {
        cases.extend(KIND.bounded_string_cases(path, 256, &document));
    }
    cases.extend(KIND.digest_cases(&["surface_decision", "request_digest"], &document));
    cases.extend(KIND.timestamp_cases(&["surface_decision", "expires_at"], &document));
    for field in ["logical_part", "attempt_count"] {
        cases.extend(KIND.integer_cases(&[field], 0, u64::from(u32::MAX), &document));
    }
    cases.extend([
        // --- sent and the Telegram message ID --------------------------------
        Case::new("state_ready_without_message_id", unsent("ready"), Accept),
        Case::new(
            "state_ready_message_id_absent",
            {
                let mut map = KIND.vector_object();
                map.insert("state".into(), json!("ready"));
                map.remove("telegram_message_id");
                serde_json::to_vec(&map).unwrap()
            },
            Accept,
        ),
        Case::new(
            "state_sent_without_message_id",
            KIND.with("telegram_message_id", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "state_sent_message_id_absent",
            KIND.without("telegram_message_id"),
            Reject(document.clone()),
        ),
        Case::new(
            "state_ready_with_message_id",
            KIND.with("state", json!("ready")),
            Reject(document.clone()),
        ),
        // --- decimal identifiers ---------------------------------------------
        Case::new(
            "chat_id_positive",
            KIND.with("chat_id", json!("42")),
            Accept,
        ),
        Case::new(
            "chat_id_32_characters",
            KIND.with("chat_id", json!(format!("-{}", "1".repeat(31)))),
            Accept,
        ),
        Case::new(
            "chat_id_33_characters",
            KIND.with("chat_id", json!(format!("-{}", "1".repeat(32)))),
            Reject(document.clone()),
        ),
        Case::new(
            "chat_id_minus_only",
            KIND.with("chat_id", json!("-")),
            Reject(document.clone()),
        ),
        Case::new(
            "chat_id_plus",
            KIND.with("chat_id", json!("+1")),
            Reject(document.clone()),
        ),
        Case::new(
            "chat_id_empty",
            KIND.with("chat_id", json!("")),
            Reject(document.clone()),
        ),
        Case::new(
            "chat_id_number",
            KIND.with("chat_id", json!(-1)),
            Reject(document.clone()),
        ),
        Case::new(
            "chat_id_leading_zeros",
            KIND.with("chat_id", json!("-007")),
            Accept,
        ),
        Case::new(
            "telegram_message_id_negative",
            KIND.with("telegram_message_id", json!("-315")),
            Reject(document.clone()),
        ),
        Case::new(
            "telegram_message_id_32_digits",
            KIND.with("telegram_message_id", json!("9".repeat(32))),
            Accept,
        ),
        Case::new(
            "telegram_message_id_33_digits",
            KIND.with("telegram_message_id", json!("9".repeat(33))),
            Reject(document.clone()),
        ),
        Case::new(
            "telegram_message_id_non_ascii_digit",
            KIND.with("telegram_message_id", json!("\u{0663}15")),
            Reject(document.clone()),
        ),
        // --- effect ------------------------------------------------------------
        Case::new(
            "effect_changed_with_its_digest",
            with_effect(json!({"type": "send_message", "text": "Other."})),
            Accept,
        ),
        Case::new(
            "effect_empty_with_its_digest",
            with_effect(json!({})),
            Reject(document.clone()),
        ),
        Case::new(
            "effect_array",
            KIND.with("effect", json!([])),
            Reject(document.clone()),
        ),
        Case::new(
            "effect_digest_not_of_effect",
            KIND.with("effect_digest", json!(format!("sha256:{}", "1".repeat(64)))),
            ParserOnly(document.clone()),
        ),
        // --- enums -------------------------------------------------------------
        Case::new(
            "relationship_unknown",
            KIND.with("relationship", json!("reply")),
            Reject(unknown("relationship")),
        ),
        Case::new(
            "state_unknown",
            KIND.with("state", json!("delivered")),
            Reject(unknown("state")),
        ),
        Case::new(
            "surface_decision_state_unknown",
            KIND.with_at(&["surface_decision", "state"], json!("expired")),
            Reject(unknown("surface_decision.state")),
        ),
        Case::new(
            "relationship_number",
            KIND.with("relationship", json!(1)),
            Reject(document.clone()),
        ),
        Case::new(
            "topic_null",
            KIND.with("topic", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "surface_decision_null",
            KIND.with("surface_decision", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "schema_version_graph_with_delivery_state",
            KIND.with("schema_version", json!("psyche.graph.v1")),
            Reject(ContractError::UnknownEnumValue {
                schema: SchemaKind::Graph,
                field: "state",
            }),
        ),
        Case::new(
            "logical_part_unsafe",
            crate::schema_parity::replace_once(
                &KIND.vector(),
                "\"logical_part\":0",
                "\"logical_part\":9007199254740992",
            ),
            Reject(ContractError::NonInteroperableNumber),
        ),
    ]);
    cases.extend([
        Case::new(
            "relationship_as_single_key_object",
            KIND.as_single_key_object(&["relationship"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "state_as_single_key_object",
            KIND.as_single_key_object(&["state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "surface_decision.state_as_single_key_object",
            KIND.as_single_key_object(&["surface_decision", "state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "topic_as_positional_array",
            KIND.as_positional_array(&["topic"], &["kind", "id"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "surface_decision_as_positional_array",
            KIND.as_positional_array(
                &["surface_decision"],
                &[
                    "decision_id",
                    "request_digest",
                    "policy_revision",
                    "expires_at",
                    "state",
                ],
            ),
            Expect::DecoderOnly,
        ),
        Case::new(
            "state_unknown_as_single_key_object",
            KIND.with("state", json!({"lost": null})),
            Reject(KIND.document()),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["delivery_id"], true),
        (["intent_id"], true),
        (["chat_id"], true),
        (["telegram_message_id"], true),
        (["surface_decision", "request_digest"], true),
        (["surface_decision", "expires_at"], false),
    ],
    min_sweep: 20_000
);

#[test]
fn the_schema_names_exactly_the_nested_members() {
    KIND.check_nested_members(&["topic"], &KIND.vector(), &[]);
    KIND.check_nested_members(&["surface_decision"], &KIND.vector(), &[]);
}

#[test]
fn the_state_enum_matches_the_decoder() {
    KIND.check_enum(&["state"], &STATES);
}

#[test]
fn the_relationship_enum_matches_the_decoder() {
    KIND.check_enum(&["relationship"], &RELATIONSHIPS);
}

#[test]
fn the_surface_decision_state_enum_matches_the_decoder() {
    KIND.check_enum(&["surface_decision", "state"], &["reserved", "consumed"]);
}

/// `topic`'s two members are both strings, so its array form also decodes
/// in the other order, binding each value to the other member. Nothing in
/// the decoder notices.
#[test]
fn a_swapped_positional_array_binds_values_to_the_wrong_members() {
    let topic = KIND.vector_object()["topic"].as_object().unwrap().clone();
    let swapped = KIND.with("topic", positional(&topic, &["id", "kind"]));
    let decoded = psyche_core::contracts::decode_document(&swapped).unwrap();
    let canonical: Value =
        serde_json::from_slice(&psyche_core::digest::canonical_bytes(&decoded).unwrap()).unwrap();
    let original = KIND.vector_object();
    assert_ne!(original["topic"]["id"], original["topic"]["kind"]);
    assert_eq!(canonical["topic"]["kind"], original["topic"]["id"]);
    assert_eq!(canonical["topic"]["id"], original["topic"]["kind"]);
}
