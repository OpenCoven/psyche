//! `psyche.surface_effect.v1`, excluded from the first profile by #60
//! (row 13 is held). The schema describes the current decoder only.

use psyche_core::contracts::{ContractError, SchemaKind};
use psyche_core::digest::{canonical_bytes, digest};
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind, sha256_hex};

const KIND: Kind = Kind {
    name: "surface_effect",
    schema: SchemaKind::SurfaceEffect,
    pinned_sha256: "30b6d32aa65134fb5e52cdcbf0955bea49fd2e74cbbf58bf40661106d4b5e0c4",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &[
        "parser-only-cases:common",
        "parser-only-cases:surface_effect",
    ],
};

/// The vector with `effect` replaced and `effect_digest` recomputed.
fn with_effect(effect: Value) -> Vec<u8> {
    let digest = json!(digest(&effect).unwrap().as_str());
    KIND.with_all(&[(&["effect"], effect), (&["effect_digest"], digest)])
}

fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    for (field, prefix, wrong) in [
        ("surface_effect_id", "sfx_", "sev_"),
        ("intent_id", "int_", "sfx_"),
        ("graph_id", "grf_", "nod_"),
        ("node_id", "nod_", "grf_"),
        ("attempt_id", "att_", "nod_"),
        ("familiar_snapshot_id", "ids_", "int_"),
    ] {
        cases.extend(KIND.id_cases(&[field], prefix, wrong, &document, &document));
    }
    for field in ["project_id", "action_class", "account_id"] {
        cases.extend(KIND.bounded_string_cases(&[field], 256, &document));
    }
    cases.extend(KIND.timestamp_cases(&["created_at"], &document));
    cases.extend([
        Case::new(
            "effect_empty_with_its_digest",
            with_effect(json!({})),
            Accept,
        ),
        Case::new(
            "effect_changed_with_its_digest",
            with_effect(json!({"text": "Other.", "type": "message", "n": 1.5})),
            Accept,
        ),
        Case::new("locator_empty", KIND.with("locator", json!({})), Accept),
        Case::new(
            "effect_array",
            KIND.with("effect", json!([])),
            Reject(document.clone()),
        ),
        Case::new(
            "locator_null",
            KIND.with("locator", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "effect_unsafe_integer",
            KIND.with("effect", json!({"n": 9_007_199_254_740_992_u64})),
            Reject(ContractError::NonInteroperableNumber),
        ),
        Case::new(
            "effect_digest_uppercase_hex",
            KIND.with("effect_digest", json!(format!("sha256:{}", "A".repeat(64)))),
            Reject(document.clone()),
        ),
        // --- parser-only -----------------------------------------------------
        Case::new(
            "effect_digest_not_of_effect",
            KIND.with("effect_digest", json!(format!("sha256:{}", "1".repeat(64)))),
            ParserOnly(document.clone()),
        ),
        Case::new(
            "effect_changed_without_its_digest",
            KIND.with("effect", json!({"text": "Other.", "type": "message"})),
            ParserOnly(document.clone()),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["surface_effect_id"], true),
        (["intent_id"], true),
        (["graph_id"], true),
        (["node_id"], true),
        (["attempt_id"], true),
        (["familiar_snapshot_id"], true),
        (["created_at"], false),
    ],
    min_sweep: 25_000
);

#[test]
fn the_effect_digest_is_the_sha256_of_the_canonical_effect() {
    let effect = KIND.vector_object()["effect"].clone();
    let expected = format!("sha256:{}", sha256_hex(&canonical_bytes(&effect).unwrap()));
    assert_eq!(digest(&effect).unwrap().as_str(), expected);
    assert_eq!(KIND.vector_object()["effect_digest"], json!(expected));
}
