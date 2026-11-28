//! The non-canonical-form search of the record kinds, run over the intent
//! vector. Intent's own parity tests live in the `intent_schema` target,
//! which this module leaves unchanged.

use psyche_core::contracts::SchemaKind;

use crate::schema_parity::Kind;

const INTENT: Kind = Kind {
    name: "intent",
    schema: SchemaKind::Intent,
    pinned_sha256: "8ef6700f6d72a7666b57cc051342bc04491c47782f73950cc60fb2666071f5ed",
    optional_members: &["surface_event_id"],
    doc: "docs/protocol/INTENT-SCHEMA.md",
    doc_sections: &["parser-only-cases"],
};

/// Intent has no enum member and no closed nested object, so neither
/// decoder-only form applies and INTENT-SCHEMA.md needs no such table.
#[test]
fn intent_has_no_decoder_only_form() {
    INTENT.check_pinned_vector();
    INTENT.check_non_canonical_forms(&[]);
}
