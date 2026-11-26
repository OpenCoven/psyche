//! `psyche.budget.v1`, `experimental` in #60. Budget policy has no owner.

use psyche_core::contracts::SchemaKind;
use serde_json::json;

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "budget",
    schema: SchemaKind::Budget,
    pinned_sha256: "f69fac29e8176690c0c4998649d92248a9a5f3e84ba510f6d02d2986acbf48be",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:budget"],
};

fn corpus() -> Vec<Case> {
    use Expect::Accept;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["budget_id"], "bud_", "grf_", &document, &document));
    cases.extend(KIND.id_cases(&["graph_id"], "grf_", "bud_", &document, &document));
    cases.extend(KIND.bounded_string_cases(&["resource_class"], 256, &document));
    for field in ["limit", "reserved", "consumed", "released"] {
        cases.extend(KIND.integer_cases(&[field], 0, 9_007_199_254_740_991, &document));
    }
    cases.extend([
        Case::new(
            "consumed_above_limit",
            KIND.with_all(&[(&["limit"], json!(10)), (&["consumed"], json!(11))]),
            Accept,
        ),
        Case::new(
            "reserved_and_released_above_limit",
            KIND.with_all(&[
                (&["limit"], json!(0)),
                (&["reserved"], json!(5)),
                (&["released"], json!(7)),
            ]),
            Accept,
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [(["budget_id"], true), (["graph_id"], true)],
    min_sweep: 6_000
);
