//! `psyche.surface_event.v1`, a `stable-v1` target in #60.

use psyche_core::contracts::{ContractError, SchemaKind};
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind, json_error, nested, replace_once};

const KIND: Kind = Kind {
    name: "surface_event",
    schema: SchemaKind::SurfaceEvent,
    pinned_sha256: "c458776df557249dbfba1e3d9e876e5f22f1353a0a1d2488bd822777d77761d2",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &[
        "parser-only-cases:common",
        "parser-only-cases:surface_event",
    ],
};

/// As for intent `constraints`: a free-form member sits at depth 1, its key
/// at 2, and the innermost `true` of `nested(n)` at `n + 2`. Measured.
const DEEPEST_ACCEPTED_NEST: usize = 62;

fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let vector = KIND.vector();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["surface_event_id"], "sev_", "sfx_", &document, &document));
    cases.extend(KIND.bounded_string_cases(&["adapter_id"], 256, &document));
    cases.extend(KIND.bounded_string_cases(&["account_id"], 256, &document));
    cases.extend(KIND.digest_cases(&["adapter_event_digest"], &document));
    cases.extend(KIND.timestamp_cases(&["received_at"], &document));
    for member in ["actor", "locator", "content"] {
        cases.extend([
            Case::new(
                format!("{member}_empty"),
                KIND.with(member, json!({})),
                Accept,
            ),
            Case::new(
                format!("{member}_array"),
                KIND.with(member, json!([])),
                Reject(document.clone()),
            ),
            Case::new(
                format!("{member}_null"),
                KIND.with(member, Value::Null),
                Reject(document.clone()),
            ),
            Case::new(
                format!("{member}_string"),
                KIND.with(member, json!("text")),
                Reject(document.clone()),
            ),
        ]);
    }
    cases.extend([
        Case::new(
            "content_long_and_empty_keys",
            KIND.with("content", json!({"": 1, "k".repeat(300): 2})),
            Accept,
        ),
        Case::new(
            "content_largest_safe_integer",
            KIND.with(
                "content",
                json!({"n": 9_007_199_254_740_991_u64, "m": -9_007_199_254_740_991_i64}),
            ),
            Accept,
        ),
        Case::new(
            "content_unsafe_integer",
            replace_once(&vector, "\"length\":9", "\"length\":9007199254740992"),
            Reject(ContractError::NonInteroperableNumber),
        ),
        Case::new(
            "actor_unsafe_integer_nested",
            KIND.with("actor", json!({"ids": [{"n": 9_007_199_254_740_992_u64}]})),
            Reject(ContractError::NonInteroperableNumber),
        ),
        Case::new(
            "content_nesting_at_limit",
            KIND.with("content", json!({"deep": nested(DEEPEST_ACCEPTED_NEST)})),
            Accept,
        ),
        // --- parser-only ---------------------------------------------------
        Case::new(
            "content_number_overflow",
            replace_once(&vector, "\"length\":9", "\"length\":1e400"),
            ParserOnly(ContractError::NonInteroperableNumber),
        ),
        Case::new(
            "content_nesting_beyond_limit",
            KIND.with("content", json!({"deep": nested(DEEPEST_ACCEPTED_NEST + 1)})),
            ParserOnly(json_error()),
        ),
        Case::new(
            "duplicate_locator_key",
            replace_once(&vector, "{\"chat_id\"", "{\"type\":\"message\",\"chat_id\""),
            ParserOnly(json_error()),
        ),
        Case::new(
            "content_canonical_over_1_mib",
            KIND.with(
                "content",
                json!({"n": Value::Array(vec![serde_json::from_str::<Value>("1e-6").unwrap(); 120_000])}),
            ),
            ParserOnly(document.clone()),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["surface_event_id"], true),
        (["adapter_event_digest"], true),
        (["received_at"], false),
    ],
    min_sweep: 10_000
);
