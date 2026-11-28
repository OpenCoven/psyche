//! `psyche.delegation.v1`, `experimental` in #60.

use psyche_core::contracts::SchemaKind;
use serde_json::json;

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "delegation",
    schema: SchemaKind::Delegation,
    pinned_sha256: "0893d64053a40fbd6d7ac09c36719a801a4967ba70d376c83efccf6b84c396f7",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:delegation"],
};

fn corpus() -> Vec<Case> {
    use Expect::Accept;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["delegation_id"], "dlg_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(&["parent_node_id"], "nod_", "dlg_", &document, &document));
    cases.extend(KIND.id_cases(&["child_node_id"], "nod_", "grf_", &document, &document));
    cases.extend(KIND.id_cases(&["budget_id"], "bud_", "dlg_", &document, &document));
    cases.extend(KIND.digest_cases(&["scope_digest"], &document));
    cases.extend(KIND.digest_cases(&["evidence_scope_digest"], &document));
    cases.extend(KIND.bounded_string_cases(&["cancellation_policy"], 256, &document));
    cases.push(Case::new(
        "parent_equals_child",
        KIND.with("child_node_id", json!("nod_01J00000000000000000000002")),
        Accept,
    ));
    cases
}

parity_tests!(
    sweep: [
        (["delegation_id"], true),
        (["parent_node_id"], true),
        (["child_node_id"], true),
        (["budget_id"], true),
        (["scope_digest"], true),
        (["evidence_scope_digest"], true),
    ],
    min_sweep: 30_000
);
