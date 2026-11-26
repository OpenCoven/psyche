//! `psyche.evidence.v1`, `experimental` in #60. Evidence policy has no owner.

use psyche_core::contracts::SchemaKind;

use crate::schema_parity::{Case, Kind};

const KIND: Kind = Kind {
    name: "evidence",
    schema: SchemaKind::Evidence,
    pinned_sha256: "4e16da1b930ac3632a48139d9ab93e803adcb603d742736ae45c5e86263955b0",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:evidence"],
};

fn corpus() -> Vec<Case> {
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["evidence_id"], "evd_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(&["node_id"], "nod_", "att_", &document, &document));
    cases.extend(KIND.id_cases(&["attempt_id"], "att_", "evd_", &document, &document));
    cases.extend(KIND.digest_cases(&["content_digest"], &document));
    for field in [
        "producer",
        "collection_method",
        "media_type",
        "retention_policy",
    ] {
        cases.extend(KIND.bounded_string_cases(&[field], 256, &document));
    }
    cases.extend(KIND.integer_cases(&["size"], 0, 9_007_199_254_740_991, &document));
    cases.extend(KIND.timestamp_cases(&["created_at"], &document));
    cases
}

parity_tests!(
    sweep: [
        (["evidence_id"], true),
        (["node_id"], true),
        (["attempt_id"], true),
        (["content_digest"], true),
        (["created_at"], false),
    ],
    min_sweep: 20_000
);
