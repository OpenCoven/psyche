//! Parity between the experimental `psyche.intent.v1` JSON Schema and the
//! typed decoder.
//!
//! `schemas/psyche.intent.v1.schema.json` is hand-written. This file is its
//! drift test (issue #11): one corpus, derived in code from the pinned #50
//! vector, runs through both `decode_document` and a Draft 2020-12
//! validator. Every case pins the decoder's exact result and the schema's
//! verdict. The schema must accept everything the decoder accepts. A case
//! the decoder rejects and the schema accepts is allowed only when it is
//! declared [`Expect::ParserOnly`] and listed in
//! `docs/protocol/INTENT-SCHEMA.md`, because the typed decoder, not the
//! schema, is the authority on validity. The shared harness in
//! `schema_parity` runs the checks and keeps the validator offline.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod schema_parity;

use psyche_core::contracts::{ContractError, SchemaKind};
use schema_parity::{
    Case, Expect, Kind, MAX_DOCUMENT_BYTES, crate_file, json_error, nested, replace_once,
};
use serde_json::{Map, Value, json};

const INTENT: Kind = Kind {
    name: "intent",
    schema: SchemaKind::Intent,
    pinned_sha256: "8ef6700f6d72a7666b57cc051342bc04491c47782f73950cc60fb2666071f5ed",
    // The one optional member: serde defaults an absent `Option` to `None`.
    optional_members: &["surface_event_id"],
    doc: "docs/protocol/INTENT-SCHEMA.md",
    doc_sections: &["parser-only-cases"],
};
const LOCAL_FIXTURE_FILE: &str = "tests/fixtures/intent-local.json";
const OPTIONAL_MEMBERS: &[&str] = INTENT.optional_members;

fn vector() -> Vec<u8> {
    INTENT.vector()
}

fn vector_object() -> Map<String, Value> {
    INTENT.vector_object()
}

fn with(field: &str, value: Value) -> Vec<u8> {
    INTENT.with(field, value)
}

fn without(field: &str) -> Vec<u8> {
    INTENT.without(field)
}

fn with_extra(field: &str, value: Value) -> Vec<u8> {
    INTENT.with_extra(field, value)
}

fn ulid_like(prefix: &str, suffix: &str) -> Value {
    schema_parity::id(prefix, suffix)
}

/// The deepest `nested(n)` the decoder accepts under `constraints.deep`.
/// The strict parser counts the document root as depth 0 and refuses any
/// value deeper than 64. `constraints` is at 1, `deep` at 2, and the
/// innermost `true` of `nested(n)` at `n + 2`. `nesting_at_limit` and
/// `nesting_beyond_limit` pin both sides of the boundary (measured, not
/// derived: 62 decodes and 63 does not).
const DEEPEST_ACCEPTED_NEST: usize = 62;

const ULID: &str = "01J00000000000000000000001";

#[allow(clippy::too_many_lines)]
fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserDependent, ParserOnly, Reject};
    let document = ContractError::InvalidShape {
        schema: SchemaKind::Intent,
        field: "document",
    };
    let mut cases = vec![
        // --- accepted by both ------------------------------------------------
        Case {
            name: "pinned_vector",
            bytes: vector(),
            expect: Accept,
        },
        Case {
            name: "local_fixture",
            bytes: crate_file(LOCAL_FIXTURE_FILE),
            expect: Accept,
        },
        Case {
            name: "pretty_printed_vector",
            bytes: serde_json::to_vec_pretty(&vector_object()).unwrap(),
            expect: Accept,
        },
        Case {
            name: "trailing_newline",
            bytes: [vector(), b"\n".to_vec()].concat(),
            expect: Accept,
        },
        Case {
            name: "surface_event_id_null",
            bytes: with("surface_event_id", Value::Null),
            expect: Accept,
        },
        Case {
            name: "surface_event_id_absent",
            bytes: without("surface_event_id"),
            expect: Accept,
        },
        Case {
            name: "constraints_empty",
            bytes: with("constraints", json!({})),
            expect: Accept,
        },
        Case {
            name: "constraints_float_and_negative_zero",
            bytes: with("constraints", json!({"ratio": 2.5, "z": -0.0})),
            expect: Accept,
        },
        Case {
            name: "required_evidence_empty",
            bytes: with("required_evidence", json!([])),
            expect: Accept,
        },
        Case {
            name: "required_evidence_duplicates",
            bytes: with("required_evidence", json!(["tests", "tests"])),
            expect: Accept,
        },
        Case {
            name: "required_evidence_1024_items",
            bytes: with("required_evidence", json!(vec!["e"; 1024])),
            expect: Accept,
        },
        Case {
            name: "principal_id_255_bytes",
            bytes: with("principal_id", json!("p".repeat(255))),
            expect: Accept,
        },
        Case {
            name: "project_id_255_bytes",
            bytes: with("project_id", json!("p".repeat(255))),
            expect: Accept,
        },
        Case {
            name: "requested_outcome_16384_bytes",
            bytes: with("requested_outcome", json!("r".repeat(16_384))),
            expect: Accept,
        },
        Case {
            name: "required_evidence_item_256_bytes",
            bytes: with("required_evidence", json!(["e".repeat(256)])),
            expect: Accept,
        },
        Case {
            name: "constraints_key_256_bytes",
            bytes: with("constraints", json!({"k".repeat(256): true})),
            expect: Accept,
        },
        Case {
            name: "principal_id_multibyte_within_bytes",
            bytes: with("principal_id", json!("é".repeat(127))),
            expect: Accept,
        },
        Case {
            name: "principal_id_nul_character",
            bytes: with("principal_id", json!("a\u{0}b")),
            expect: Accept,
        },
        Case {
            name: "intent_id_largest_ulid",
            bytes: with("intent_id", ulid_like("int_", "7ZZZZZZZZZZZZZZZZZZZZZZZZZ")),
            expect: Accept,
        },
        Case {
            name: "digest_not_recomputed",
            bytes: with("digest", json!(format!("sha256:{}", "a".repeat(64)))),
            expect: Accept,
        },
        Case {
            name: "created_at_lowercase_t_and_z",
            bytes: with("created_at", json!("2026-08-01t00:00:00z")),
            expect: Accept,
        },
        Case {
            name: "created_at_space_separator",
            bytes: with("created_at", json!("2026-08-01 00:00:00Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_positive_offset",
            bytes: with("created_at", json!("2026-08-01T00:00:00+01:00")),
            expect: Accept,
        },
        Case {
            name: "created_at_negative_zero_offset",
            bytes: with("created_at", json!("2026-08-01T00:00:00-00:00")),
            expect: Accept,
        },
        Case {
            name: "created_at_largest_offset",
            bytes: with("created_at", json!("2026-08-01T00:00:00+23:59")),
            expect: Accept,
        },
        Case {
            name: "created_at_nine_fraction_digits",
            bytes: with("created_at", json!("2026-08-01T00:00:00.123456789Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_ten_fraction_digits",
            bytes: with("created_at", json!("2026-08-01T00:00:00.1234567891Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_leap_second_at_year_end",
            bytes: with("created_at", json!("2026-12-31T23:59:60Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_year_zero",
            bytes: with("created_at", json!("0000-01-01T00:00:00Z")),
            expect: Accept,
        },
        // --- added after review: separators, integers, depth -----------------
        Case {
            name: "created_at_separator_x",
            bytes: with("created_at", json!("2026-08-01X00:00:00Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_separator_digit",
            bytes: with("created_at", json!("2026-08-01100:00:00Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_separator_tab",
            bytes: with("created_at", json!("2026-08-01\t00:00:00Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_separator_nul",
            bytes: with("created_at", json!("2026-08-01\u{0}00:00:00Z")),
            expect: Accept,
        },
        Case {
            name: "created_at_separator_non_ascii",
            bytes: with("created_at", json!("2026-08-01\u{e9}00:00:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "constraints_largest_safe_integer",
            bytes: with(
                "constraints",
                json!({"n": 9_007_199_254_740_991_u64, "m": -9_007_199_254_740_991_i64}),
            ),
            expect: Accept,
        },
        Case {
            name: "constraints_integral_exponent_within_range",
            bytes: replace_once(&vector(), "\"max_attempts\":2", "\"max_attempts\":1e15"),
            expect: Accept,
        },
        Case {
            name: "constraints_tiny_fraction",
            bytes: replace_once(&vector(), "\"max_attempts\":2", "\"max_attempts\":1e-400"),
            expect: Accept,
        },
        Case {
            name: "constraints_unsafe_integer",
            bytes: replace_once(
                &vector(),
                "\"max_attempts\":2",
                "\"max_attempts\":9007199254740993",
            ),
            expect: Reject(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "constraints_unsafe_negative_integer",
            bytes: replace_once(
                &vector(),
                "\"max_attempts\":2",
                "\"max_attempts\":-9007199254740992",
            ),
            expect: Reject(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "constraints_unsafe_integral_exponent",
            bytes: replace_once(&vector(), "\"max_attempts\":2", "\"max_attempts\":1e20"),
            expect: Reject(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "constraints_unsafe_integral_fraction",
            bytes: replace_once(
                &vector(),
                "\"max_attempts\":2",
                "\"max_attempts\":9007199254740992.0",
            ),
            expect: Reject(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "constraints_unsafe_integer_nested",
            bytes: replace_once(
                &vector(),
                "[\"read\",\"test\"]",
                "[\"read\",[{\"n\":9007199254740992}]]",
            ),
            expect: Reject(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "nesting_at_limit",
            bytes: with(
                "constraints",
                json!({"deep": nested(DEEPEST_ACCEPTED_NEST)}),
            ),
            expect: Accept,
        },
        // --- rejected by both: members ---------------------------------------
        Case {
            name: "unknown_top_level_field",
            bytes: with_extra("aa_unknown_field", json!(true)),
            expect: Reject(document.clone()),
        },
        // --- rejected by both: schema_version --------------------------------
        Case {
            name: "schema_version_major_2",
            bytes: with("schema_version", json!("psyche.intent.v2")),
            expect: Reject(ContractError::UnsupportedMajor {
                found: 2,
                supported: 1,
            }),
        },
        Case {
            name: "schema_version_major_0",
            bytes: with("schema_version", json!("psyche.intent.v0")),
            expect: Reject(ContractError::UnsupportedMajor {
                found: 0,
                supported: 1,
            }),
        },
        Case {
            name: "schema_version_unknown_kind",
            bytes: with("schema_version", json!("psyche.intent_unknown.v1")),
            expect: Reject(ContractError::UnknownSchema),
        },
        Case {
            name: "schema_version_other_kind",
            bytes: with("schema_version", json!("psyche.graph.v1")),
            expect: Reject(ContractError::InvalidShape {
                schema: SchemaKind::Graph,
                field: "document",
            }),
        },
        Case {
            name: "schema_version_not_a_string",
            bytes: with("schema_version", json!(1)),
            expect: Reject(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "schema_version",
            }),
        },
        // --- rejected by both: identifiers -----------------------------------
        Case {
            name: "intent_id_snapshot_prefix",
            bytes: with("intent_id", ulid_like("ids_", ULID)),
            expect: Reject(document.clone()),
        },
        Case {
            name: "familiar_snapshot_id_intent_prefix",
            bytes: with("familiar_snapshot_id", ulid_like("int_", ULID)),
            expect: Reject(document.clone()),
        },
        Case {
            name: "surface_event_id_effect_prefix",
            bytes: with("surface_event_id", ulid_like("sfx_", ULID)),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_unregistered_prefix",
            bytes: with("intent_id", ulid_like("dly_", ULID)),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_lowercase_ulid",
            bytes: with("intent_id", ulid_like("int_", &ULID.to_lowercase())),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_ulid_overflow",
            bytes: with("intent_id", ulid_like("int_", &format!("8{}", &ULID[1..]))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_ulid_excluded_letter",
            bytes: with("intent_id", ulid_like("int_", &format!("{}I", &ULID[..25]))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_ulid_25_characters",
            bytes: with("intent_id", ulid_like("int_", &ULID[1..])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_ulid_27_characters",
            bytes: with("intent_id", ulid_like("int_", &format!("{ULID}0"))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "intent_id_null",
            bytes: with("intent_id", Value::Null),
            expect: Reject(document.clone()),
        },
        // --- rejected by both: bounded strings and lists ---------------------
        Case {
            name: "principal_id_empty",
            bytes: with("principal_id", json!("")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "principal_id_256_bytes",
            bytes: with("principal_id", json!("p".repeat(256))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "principal_id_number",
            bytes: with("principal_id", json!(7)),
            expect: Reject(document.clone()),
        },
        Case {
            name: "project_id_empty",
            bytes: with("project_id", json!("")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "project_id_256_bytes",
            bytes: with("project_id", json!("p".repeat(256))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "requested_outcome_empty",
            bytes: with("requested_outcome", json!("")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "requested_outcome_16385_bytes",
            bytes: with("requested_outcome", json!("r".repeat(16_385))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "required_evidence_1025_items",
            bytes: with("required_evidence", json!(vec!["e"; 1025])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "required_evidence_item_empty",
            bytes: with("required_evidence", json!([""])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "required_evidence_item_257_bytes",
            bytes: with("required_evidence", json!(["e".repeat(257)])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "required_evidence_not_a_list",
            bytes: with("required_evidence", json!("tests")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "required_evidence_item_number",
            bytes: with("required_evidence", json!([1])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "constraints_key_empty",
            bytes: with("constraints", json!({"": true})),
            expect: Reject(document.clone()),
        },
        Case {
            name: "constraints_key_257_bytes",
            bytes: with("constraints", json!({"k".repeat(257): true})),
            expect: Reject(document.clone()),
        },
        Case {
            name: "constraints_array",
            bytes: with("constraints", json!([])),
            expect: Reject(document.clone()),
        },
        Case {
            name: "constraints_null",
            bytes: with("constraints", Value::Null),
            expect: Reject(document.clone()),
        },
        // --- rejected by both: digest ----------------------------------------
        Case {
            name: "digest_uppercase_hex",
            bytes: with("digest", json!(format!("sha256:{}", "A".repeat(64)))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "digest_other_algorithm",
            bytes: with("digest", json!(format!("sha512:{}", "1".repeat(64)))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "digest_63_hex",
            bytes: with("digest", json!(format!("sha256:{}", "1".repeat(63)))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "digest_null",
            bytes: with("digest", Value::Null),
            expect: Reject(document.clone()),
        },
        // --- rejected by both: created_at ------------------------------------
        Case {
            name: "created_at_no_offset",
            bytes: with("created_at", json!("2026-08-01T00:00:00")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_single_digit_month",
            bytes: with("created_at", json!("2026-8-01T00:00:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_signed_year",
            bytes: with("created_at", json!("+2026-08-01T00:00:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_hour_24",
            bytes: with("created_at", json!("2026-08-01T24:00:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_minute_60",
            bytes: with("created_at", json!("2026-08-01T00:60:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_offset_24_hours",
            bytes: with("created_at", json!("2026-08-01T00:00:00+24:00")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_offset_without_colon",
            bytes: with("created_at", json!("2026-08-01T00:00:00+0100")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_empty_fraction",
            bytes: with("created_at", json!("2026-08-01T00:00:00.Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_non_ascii_digit",
            bytes: with("created_at", json!("\u{0662}026-08-01T00:00:00Z")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_trailing_newline",
            bytes: with("created_at", json!("2026-08-01T00:00:00Z\n")),
            expect: Reject(document.clone()),
        },
        Case {
            name: "digest_trailing_newline",
            bytes: with("digest", json!(format!("sha256:{}\n", "1".repeat(64)))),
            expect: Reject(document.clone()),
        },
        Case {
            name: "created_at_number",
            bytes: with("created_at", json!(0)),
            expect: Reject(document.clone()),
        },
        // --- rejected by both: malformed input -------------------------------
        Case {
            name: "empty_input",
            bytes: Vec::new(),
            expect: Reject(json_error()),
        },
        Case {
            name: "truncated",
            bytes: vector()[..vector().len() - 1].to_vec(),
            expect: Reject(json_error()),
        },
        Case {
            name: "trailing_data",
            bytes: [vector(), b"{}".to_vec()].concat(),
            expect: Reject(json_error()),
        },
        Case {
            name: "trailing_comma",
            bytes: replace_once(&vector(), "}}", "},}"),
            expect: Reject(json_error()),
        },
        Case {
            name: "byte_order_mark",
            bytes: [b"\xef\xbb\xbf".to_vec(), vector()].concat(),
            expect: ParserDependent(json_error()),
        },
        Case {
            name: "invalid_utf8",
            bytes: replace_once(&vector(), "principal:synthetic-1", "principal:\u{0}")
                .into_iter()
                .map(|b| if b == 0 { 0xff } else { b })
                .collect(),
            expect: ParserDependent(json_error()),
        },
        Case {
            name: "lone_surrogate_escape",
            bytes: replace_once(&vector(), "principal:synthetic-1", "\\ud800"),
            expect: ParserDependent(json_error()),
        },
        Case {
            name: "array_root",
            bytes: b"[]".to_vec(),
            expect: Reject(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "schema_version",
            }),
        },
        Case {
            name: "string_root",
            bytes: b"\"psyche.intent.v1\"".to_vec(),
            expect: Reject(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "schema_version",
            }),
        },
        // --- parser-only: the decoder rejects, the schema cannot -------------
        Case {
            name: "duplicate_top_level_key",
            bytes: replace_once(
                &vector(),
                "{\"constraints\"",
                "{\"principal_id\":\"principal:synthetic-1\",\"constraints\"",
            ),
            expect: ParserOnly(json_error()),
        },
        Case {
            name: "duplicate_constraints_key",
            bytes: replace_once(
                &vector(),
                "{\"allowed_actions\"",
                "{\"dry\":1,\"dry\":1,\"allowed_actions\"",
            ),
            expect: ParserOnly(json_error()),
        },
        Case {
            name: "nesting_beyond_limit",
            bytes: with(
                "constraints",
                json!({"deep": nested(DEEPEST_ACCEPTED_NEST + 1)}),
            ),
            expect: ParserOnly(json_error()),
        },
        Case {
            name: "constraints_number_overflow",
            bytes: replace_once(&vector(), "\"max_attempts\":2", "\"max_attempts\":1e400"),
            expect: ParserOnly(ContractError::NonInteroperableNumber),
        },
        Case {
            name: "principal_id_multibyte_over_255_bytes",
            bytes: with("principal_id", json!("é".repeat(128))),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "project_id_multibyte_over_255_bytes",
            bytes: with("project_id", json!("é".repeat(128))),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "requested_outcome_multibyte_over_16384_bytes",
            bytes: with("requested_outcome", json!("é".repeat(8_193))),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "required_evidence_item_multibyte_over_256_bytes",
            bytes: with("required_evidence", json!(["é".repeat(129)])),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "constraints_key_multibyte_over_256_bytes",
            bytes: with("constraints", json!({"é".repeat(129): true})),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "created_at_impossible_date",
            bytes: with("created_at", json!("2026-02-30T00:00:00Z")),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "created_at_leap_second_off_boundary",
            bytes: with("created_at", json!("2026-08-01T23:59:60Z")),
            expect: ParserOnly(document.clone()),
        },
        Case {
            name: "document_over_1_mib",
            bytes: with(
                "constraints",
                json!({"pad": "x".repeat(MAX_DOCUMENT_BYTES)}),
            ),
            expect: ParserOnly(ContractError::DocumentTooLarge),
        },
        Case {
            name: "constraints_canonical_over_1_mib",
            bytes: with(
                "constraints",
                json!({"n": Value::Array(vec![serde_json::from_str::<Value>("1e-6").unwrap(); 120_000])}),
            ),
            expect: ParserOnly(document.clone()),
        },
    ];
    for field in vector_object().keys() {
        if OPTIONAL_MEMBERS.contains(&field.as_str()) {
            continue;
        }
        let name: &'static str = Box::leak(format!("missing_{field}").into_boxed_str());
        let expect = if field == "schema_version" {
            Reject(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "schema_version",
            })
        } else {
            Reject(document.clone())
        };
        cases.push(Case {
            name,
            bytes: without(field),
            expect,
        });
    }
    cases
}

#[test]
fn the_schema_compiles_offline_against_draft_2020_12() {
    INTENT.check_compiles();
}

#[test]
fn the_schema_names_exactly_the_typed_members() {
    INTENT.check_members();
}

#[test]
fn the_pinned_vector_keeps_its_bytes_and_digest_through_both() {
    INTENT.check_pinned_vector();
}

#[test]
fn schema_and_decoder_agree_on_the_corpus() {
    INTENT.check_corpus(&corpus());
}

#[test]
fn the_canonical_form_of_every_accepted_case_passes_both() {
    INTENT.check_canonical_forms(&corpus());
}

#[test]
fn every_parser_only_case_is_documented() {
    INTENT.check_documented(&corpus());
}

#[test]
fn every_rejection_case_mutates_the_vector() {
    INTENT.check_rejections_mutate(&corpus());
}

/// Exhaustive single-character substitutions in every pattern-checked member.
/// For identifiers and the digest the schema states the decoder's whole rule,
/// so the verdicts must be equal. For `created_at` the schema cannot state
/// calendar validity, so the sweep requires only that the schema accepts
/// whatever the decoder accepts.
#[test]
fn single_character_sweeps_never_reject_what_the_decoder_accepts() {
    let checked = INTENT.sweep(&[
        (&["intent_id"], true),
        (&["familiar_snapshot_id"], true),
        (&["surface_event_id"], true),
        (&["digest"], true),
        (&["created_at"], false),
    ]);
    assert!(checked > 10_000, "sweep too small: {checked}");
}
