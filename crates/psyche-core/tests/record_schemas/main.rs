//! Parity between the experimental record JSON Schemas in `schemas/` and the
//! typed decoder, one module per #50 record kind (issue #11).
//!
//! Each module derives its corpus in code from the kind's pinned #50 vector
//! and runs it through both `decode_document` and the kind's Draft 2020-12
//! schema, using the shared harness in `schema_parity`. The intent kind has
//! its own target, `intent_schema`, which came first. The parser-only rules
//! each schema cannot state are listed in `docs/protocol/RECORD-SCHEMAS.md`
//! and checked against the corpus.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../schema_parity/mod.rs"]
mod schema_parity;

/// The tests every kind runs. The invoking module defines `KIND` and
/// `corpus()`. `sweep` lists the pattern-checked string members and whether
/// the schema states the decoder's whole rule for each one.
macro_rules! parity_tests {
    (sweep: [$(($path:expr, $exact:expr)),* $(,)?], min_sweep: $min:expr) => {
        #[test]
        fn the_schema_compiles_offline_against_draft_2020_12() {
            KIND.check_compiles();
        }

        #[test]
        fn the_schema_names_exactly_the_typed_members() {
            KIND.check_members();
        }

        #[test]
        fn the_pinned_vector_keeps_its_bytes_and_digest_through_both() {
            KIND.check_pinned_vector();
        }

        #[test]
        fn schema_and_decoder_agree_on_the_corpus() {
            KIND.check_corpus(&corpus());
        }

        #[test]
        fn the_canonical_form_of_every_accepted_case_passes_both() {
            KIND.check_canonical_forms(&corpus());
        }

        #[test]
        fn every_parser_only_case_is_documented() {
            KIND.check_documented(&corpus());
        }

        #[test]
        fn every_rejection_case_mutates_the_vector() {
            KIND.check_rejections_mutate(&corpus());
        }

        #[test]
        fn single_character_sweeps_never_reject_what_the_decoder_accepts() {
            let checked = KIND.sweep(&[$((&$path[..], $exact)),*]);
            assert!(checked >= $min, "sweep too small: {checked}");
        }
    };
}

mod execution_binding;
mod graph;
mod graph_node;
mod identity_snapshot;
mod recovery;
mod surface_event;
