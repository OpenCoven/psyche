//! `psyche.approval.v1`, `experimental` in #60. Approval policy has no
//! owner, and #60's row 7 lists what an approval does not yet carry.

use psyche_core::contracts::SchemaKind;
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "approval",
    schema: SchemaKind::Approval,
    pinned_sha256: "32913ed310cd15bf99796c2ef780334c555303001ccc328a28780e134006665f",
    optional_members: &["decision"],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:approval"],
};

fn corpus() -> Vec<Case> {
    use Expect::Accept;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["approval_id"], "apr_", "nod_", &document, &document));
    cases.extend(KIND.id_cases(&["node_id"], "nod_", "apr_", &document, &document));
    cases.extend(KIND.bounded_string_cases(&["requester_principal_id"], 255, &document));
    cases.extend(KIND.bounded_string_cases(&["decision"], 256, &document));
    cases.extend(KIND.timestamp_cases(&["expires_at"], &document));
    cases.extend([
        Case::new("decision_null", KIND.with("decision", Value::Null), Accept),
        Case::new("decision_absent", KIND.without("decision"), Accept),
        Case::new(
            "decision_free_text",
            KIND.with("decision", json!("Denied, see thread")),
            Accept,
        ),
        Case::new(
            "expires_at_in_the_past",
            KIND.with("expires_at", json!("1970-01-01T00:00:00Z")),
            Accept,
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["approval_id"], true),
        (["node_id"], true),
        (["expires_at"], false),
    ],
    min_sweep: 8_000
);
