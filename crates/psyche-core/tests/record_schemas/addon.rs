//! `psyche.addon.v1`, `experimental` in #60. #60 notes that no test on main
//! built, decoded or stored an addon document before #50's vector.

use psyche_core::contracts::SchemaKind;
use serde_json::json;

use crate::schema_parity::{Case, Expect, Kind};

const KIND: Kind = Kind {
    name: "addon",
    schema: SchemaKind::Addon,
    pinned_sha256: "73a3d18b2dceb2e307ef5038e1466493b46fba4962962ceac3c8e7b1ed57df2f",
    optional_members: &[],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &["parser-only-cases:common", "parser-only-cases:addon"],
};

const DIGESTS: [&str; 4] = [
    "package_digest",
    "provenance_digest",
    "contributions_digest",
    "allowlist_digest",
];

fn corpus() -> Vec<Case> {
    use Expect::Accept;
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[]);
    cases.extend(KIND.id_cases(&["addon_id"], "adn_", "apr_", &document, &document));
    for field in ["package", "version", "revocation_state"] {
        cases.extend(KIND.bounded_string_cases(&[field], 256, &document));
    }
    for field in DIGESTS {
        cases.extend(KIND.digest_cases(&[field], &document));
    }
    cases.extend([
        Case::new(
            "version_not_semver",
            KIND.with("version", json!("latest")),
            Accept,
        ),
        Case::new(
            "revocation_state_free_text",
            KIND.with("revocation_state", json!("Revoked?")),
            Accept,
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["addon_id"], true),
        (["package_digest"], true),
        (["provenance_digest"], true),
        (["contributions_digest"], true),
        (["allowlist_digest"], true),
    ],
    min_sweep: 35_000
);
