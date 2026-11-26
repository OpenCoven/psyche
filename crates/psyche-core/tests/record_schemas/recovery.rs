//! `psyche.recovery.v1`, `experimental` in #60. Its `fence_token` carries
//! the process-attempt fence (held row 11), part of #11's minimum consumer
//! profile, with no published meaning.

use psyche_core::contracts::SchemaKind;
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "recovery",
    schema: SchemaKind::Recovery,
    pinned_sha256: "cc34e94d665cb945d2af6404675c06266e6892d5ff60e0cb76c106ea068c810d",
    optional_members: &["fence_token", "operator_disposition"],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:recovery"],
};

fn corpus() -> Vec<Case> {
    use Expect::Accept;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["recovery_id"], "rcv_", "att_", &document, &document));
    cases.extend(KIND.id_cases(&["attempt_id"], "att_", "rcv_", &document, &document));
    cases.extend(KIND.bounded_string_cases(&["lease_id"], 255, &document));
    cases.extend(KIND.bounded_string_cases(&["fence_token"], 255, &document));
    cases.extend(KIND.bounded_string_cases(&["ambiguity"], 256, &document));
    cases.extend(KIND.bounded_string_cases(&["operator_disposition"], 256, &document));
    cases.extend(KIND.integer_cases(
        &["reconciliation_count"],
        0,
        9_007_199_254_740_991,
        &document,
    ));
    cases.extend([
        Case::new(
            "fence_token_null",
            KIND.with("fence_token", Value::Null),
            Accept,
        ),
        Case::new("fence_token_absent", KIND.without("fence_token"), Accept),
        Case::new(
            "operator_disposition_null",
            KIND.with("operator_disposition", Value::Null),
            Accept,
        ),
        Case::new(
            "operator_disposition_absent",
            KIND.without("operator_disposition"),
            Accept,
        ),
        Case::new(
            "ambiguity_free_text",
            KIND.with("ambiguity", json!("Not An Enum")),
            Accept,
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [(["recovery_id"], true), (["attempt_id"], true)],
    min_sweep: 6_000
);
