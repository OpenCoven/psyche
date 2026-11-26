//! `psyche.graph_node.v1`, `experimental` in #60. Its
//! `termination_unknown` state names a result that deferred row 12 produces.

use psyche_core::contracts::{ContractError, SchemaKind};
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind, ULID, id};

const KIND: Kind = Kind {
    name: "graph_node",
    schema: SchemaKind::GraphNode,
    pinned_sha256: "e54a359eaa131b62da6ef743ae45be5a0bc9e203ce13b607c1cb9ccc209bf1cf",
    optional_members: &["delegation_id"],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:graph_node"],
};

const STATES: [&str; 22] = [
    "proposed",
    "admitted",
    "rejected",
    "blocked",
    "ready",
    "skipped",
    "reserved",
    "dispatching",
    "adopted",
    "adoption_unknown",
    "proven_not_adopted",
    "failed",
    "running",
    "waiting_approval",
    "candidate",
    "awaiting_verification",
    "verified",
    "escalation_required",
    "cancelling",
    "cancelled",
    "termination_unknown",
    "recovery_required",
];

fn node(suffix: &str) -> Value {
    id("nod_", suffix)
}

fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let unknown_state = ContractError::UnknownEnumValue {
        schema: SchemaKind::GraphNode,
        field: "state",
    };
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["node_id"], "nod_", "grf_", &document, &document));
    cases.extend(KIND.id_cases(&["graph_id"], "grf_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(
        &["familiar_snapshot_id"],
        "ids_",
        "int_",
        &document,
        &document,
    ));
    cases.extend(KIND.id_cases(&["delegation_id"], "dlg_", "bud_", &document, &document));
    cases.extend(KIND.id_cases(&["budget_id"], "bud_", "dlg_", &document, &document));
    cases.extend(KIND.integer_cases(&["version"], 1, 9_007_199_254_740_991, &document));
    cases.extend([
        Case::new(
            "state_as_single_key_object",
            KIND.as_single_key_object(&["state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "state_unknown_as_single_key_object",
            KIND.with("state", json!({"done": null})),
            Reject(document.clone()),
        ),
        Case::new(
            "delegation_id_null",
            KIND.with("delegation_id", Value::Null),
            Accept,
        ),
        Case::new(
            "delegation_id_absent",
            KIND.without("delegation_id"),
            Accept,
        ),
        Case::new(
            "dependencies_empty",
            KIND.with("dependencies", json!([])),
            Accept,
        ),
        Case::new(
            "dependencies_duplicates",
            KIND.with("dependencies", json!([node(ULID), node(ULID)])),
            Accept,
        ),
        Case::new(
            "dependencies_self",
            KIND.with("dependencies", json!(["nod_01J00000000000000000000001"])),
            Accept,
        ),
        Case::new(
            "dependencies_2048_items",
            KIND.with("dependencies", Value::Array(vec![node(ULID); 2048])),
            Accept,
        ),
        Case::new(
            "dependencies_wrong_prefix",
            KIND.with("dependencies", json!([id("grf_", ULID)])),
            Reject(document.clone()),
        ),
        Case::new(
            "dependencies_lowercase_ulid",
            KIND.with("dependencies", json!([node(&ULID.to_lowercase())])),
            Reject(document.clone()),
        ),
        Case::new(
            "dependencies_not_a_list",
            KIND.with("dependencies", json!(node(ULID))),
            Reject(document.clone()),
        ),
        Case::new(
            "dependencies_null",
            KIND.with("dependencies", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "required_evidence_empty",
            KIND.with("required_evidence", json!([])),
            Accept,
        ),
        Case::new(
            "required_evidence_duplicates",
            KIND.with("required_evidence", json!(["tests", "tests"])),
            Accept,
        ),
        Case::new(
            "required_evidence_1024_items",
            KIND.with("required_evidence", json!(vec!["e"; 1024])),
            Accept,
        ),
        Case::new(
            "required_evidence_item_256_bytes",
            KIND.with("required_evidence", json!(["e".repeat(256)])),
            Accept,
        ),
        Case::new(
            "required_evidence_1025_items",
            KIND.with("required_evidence", json!(vec!["e"; 1025])),
            Reject(document.clone()),
        ),
        Case::new(
            "required_evidence_item_empty",
            KIND.with("required_evidence", json!([""])),
            Reject(document.clone()),
        ),
        Case::new(
            "required_evidence_item_257_bytes",
            KIND.with("required_evidence", json!(["e".repeat(257)])),
            Reject(document.clone()),
        ),
        Case::new(
            "required_evidence_item_number",
            KIND.with("required_evidence", json!([1])),
            Reject(document.clone()),
        ),
        Case::new(
            "required_evidence_item_multibyte_over_256_bytes",
            KIND.with("required_evidence", json!(["\u{e9}".repeat(129)])),
            ParserOnly(document.clone()),
        ),
        Case::new(
            "state_unknown",
            KIND.with("state", json!("done")),
            Reject(unknown_state.clone()),
        ),
        Case::new(
            "state_uppercase",
            KIND.with("state", json!("VERIFIED")),
            Reject(unknown_state),
        ),
        Case::new(
            "state_number",
            KIND.with("state", json!(1)),
            Reject(document),
        ),
        Case::new(
            "schema_version_graph_with_node_state",
            KIND.with("schema_version", json!("psyche.graph.v1")),
            Reject(ContractError::UnknownEnumValue {
                schema: SchemaKind::Graph,
                field: "state",
            }),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["node_id"], true),
        (["graph_id"], true),
        (["familiar_snapshot_id"], true),
        (["delegation_id"], true),
        (["budget_id"], true),
    ],
    min_sweep: 15_000
);

#[test]
fn the_state_enum_matches_the_decoder() {
    KIND.check_enum(&["state"], &STATES);
}

/// The decoder checks no transition rule: every state decodes alongside the
/// vector's other members, and the schema accepts each such document.
#[test]
fn every_state_decodes_with_the_vector_members() {
    for state in STATES {
        let bytes = KIND.with("state", json!(state));
        assert!(
            psyche_core::contracts::decode_document(&bytes).is_ok(),
            "{state} does not decode"
        );
        assert!(KIND.schema_accepts(&bytes), "{state} fails the schema");
    }
}
