//! `psyche.graph.v1`, `experimental` in #60. #60 notes that no test on
//! main decoded a graph document successfully before #50's vector.

use psyche_core::contracts::{ContractError, SchemaKind};
use serde_json::json;

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "graph",
    schema: SchemaKind::Graph,
    pinned_sha256: "fd10742cb0633589ed8e150a8b865bebcf7115ac96502ca097a5a39e28a54e93",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:graph"],
};

const STATES: [&str; 11] = [
    "draft",
    "admitted",
    "rejected",
    "running",
    "waiting_approval",
    "waiting_evidence",
    "cancelling",
    "completed",
    "failed",
    "cancelled",
    "recovery_required",
];

fn corpus() -> Vec<Case> {
    use Expect::Reject;
    let document = KIND.document();
    let unknown_state = ContractError::UnknownEnumValue {
        schema: SchemaKind::Graph,
        field: "state",
    };
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["graph_id"], "grf_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(&["root_intent_id"], "int_", "grf_", &document, &document));
    cases.extend(KIND.bounded_string_cases(&["owner_principal_id"], 255, &document));
    cases.extend(KIND.bounded_string_cases(&["policy_revision"], 255, &document));
    cases.extend(KIND.integer_cases(&["version"], 1, 9_007_199_254_740_991, &document));
    cases.extend([
        Case::new(
            "state_as_single_key_object",
            KIND.as_single_key_object(&["state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "state_unknown_as_single_key_object",
            KIND.with("state", json!({"paused": null})),
            Reject(document.clone()),
        ),
        Case::new(
            "state_unknown",
            KIND.with("state", json!("paused")),
            Reject(unknown_state.clone()),
        ),
        Case::new(
            "state_uppercase",
            KIND.with("state", json!("RUNNING")),
            Reject(unknown_state.clone()),
        ),
        Case::new(
            "state_trailing_space",
            KIND.with("state", json!("running ")),
            Reject(unknown_state),
        ),
        Case::new(
            "state_number",
            KIND.with("state", json!(3)),
            Reject(document.clone()),
        ),
        Case::new(
            "state_null",
            KIND.with("state", serde_json::Value::Null),
            Reject(document),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [(["graph_id"], true), (["root_intent_id"], true)],
    min_sweep: 6_000
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
