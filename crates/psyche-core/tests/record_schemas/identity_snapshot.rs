//! `psyche.identity_snapshot.v1`, a `stable-v1` target in #60.

use psyche_core::contracts::SchemaKind;
use serde_json::{Value, json};

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "identity_snapshot",
    schema: SchemaKind::IdentitySnapshot,
    pinned_sha256: "1da8dfa1d333edef27cbb3e1e2c384951d32299bda8d75f5712f690f07dde619",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &[
        "parser-only-cases:common",
        "parser-only-cases:identity_snapshot",
    ],
};

const DIGESTS: [&str; 5] = [
    "declaration_digest",
    "identity_file_digest",
    "identity_digest",
    "soul_digest",
    "role_skill_digest",
];

fn corpus() -> Vec<Case> {
    use Expect::Reject;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[(&["provenance"], &[], document.clone())]);
    cases.extend(KIND.id_cases(&["snapshot_id"], "ids_", "int_", &document, &document));
    for path in [
        &["familiar_id"][..],
        &["principal_id"],
        &["provenance", "familiar_home_id"],
        &["provenance", "resolver_version"],
    ] {
        cases.extend(KIND.bounded_string_cases(path, 255, &document));
    }
    cases.extend(KIND.integer_cases(&["revision"], 1, 9_007_199_254_740_991, &document));
    for field in DIGESTS {
        cases.extend(KIND.digest_cases(&[field], &document));
    }
    cases.extend(KIND.timestamp_cases(&["resolved_at"], &document));
    cases.extend([
        Case::new(
            "provenance_null",
            KIND.with("provenance", Value::Null),
            Reject(document.clone()),
        ),
        Case::new(
            "provenance_array",
            KIND.with("provenance", json!([])),
            Reject(document.clone()),
        ),
    ]);
    cases.push(Case::new(
        "provenance_as_positional_array",
        KIND.as_positional_array(&["provenance"], &["familiar_home_id", "resolver_version"]),
        Expect::DecoderOnly,
    ));
    cases
}

parity_tests!(
    sweep: [
        (["snapshot_id"], true),
        (["declaration_digest"], true),
        (["identity_file_digest"], true),
        (["identity_digest"], true),
        (["soul_digest"], true),
        (["role_skill_digest"], true),
        (["resolved_at"], false),
    ],
    min_sweep: 40_000
);

#[test]
fn the_schema_names_exactly_the_provenance_members() {
    KIND.check_nested_members(&["provenance"], &KIND.vector(), &[]);
}
