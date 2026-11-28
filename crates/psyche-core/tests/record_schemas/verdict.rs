//! `psyche.verdict.v1`, `experimental` in #60. Verdict policy has no owner.

use psyche_core::contracts::SchemaKind;
use serde_json::json;

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "verdict",
    schema: SchemaKind::Verdict,
    pinned_sha256: "6f468f27a6705b1f5dc69193d276d6c796ed7c72e879ed6f41aa4750e7a784cb",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:verdict"],
};

fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["verdict_id"], "vrd_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(&["node_id"], "nod_", "vrd_", &document, &document));
    cases.extend(KIND.digest_cases(&["sealed_evidence_digest"], &document));
    for field in ["policy_revision", "verdict_type", "reviewer_id", "outcome"] {
        cases.extend(KIND.bounded_string_cases(&[field], 256, &document));
    }
    cases.extend(KIND.timestamp_cases(&["created_at"], &document));
    cases.extend([
        Case::new(
            "reason_codes_empty",
            KIND.with("reason_codes", json!([])),
            Accept,
        ),
        Case::new(
            "reason_codes_free_text",
            KIND.with("reason_codes", json!(["Tests Passed", "tests_passed"])),
            Accept,
        ),
        Case::new(
            "reason_codes_1024_items",
            KIND.with("reason_codes", json!(vec!["r"; 1024])),
            Accept,
        ),
        Case::new(
            "reason_codes_1025_items",
            KIND.with("reason_codes", json!(vec!["r"; 1025])),
            Reject(document.clone()),
        ),
        Case::new(
            "reason_codes_item_empty",
            KIND.with("reason_codes", json!([""])),
            Reject(document.clone()),
        ),
        Case::new(
            "reason_codes_item_257_bytes",
            KIND.with("reason_codes", json!(["r".repeat(257)])),
            Reject(document.clone()),
        ),
        Case::new(
            "reason_codes_item_multibyte_over_256_bytes",
            KIND.with("reason_codes", json!(["\u{e9}".repeat(129)])),
            ParserOnly(document.clone()),
        ),
        Case::new(
            "outcome_free_text",
            KIND.with("outcome", json!("Allow With Notes")),
            Accept,
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["verdict_id"], true),
        (["node_id"], true),
        (["sealed_evidence_digest"], true),
        (["created_at"], false),
    ],
    min_sweep: 15_000
);
