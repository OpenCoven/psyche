//! `psyche.execution_binding.v1`, `experimental` in #60. It carries three of
//! the facts in #11's minimum consumer profile: the `fenced` adoption state
//! (held row 11), cancellation acknowledgement and unresolved cancellation
//! (deferred row 12). The vector is in `acknowledged_terminated`.

use psyche_core::contracts::{ContractError, RecordKind, SchemaKind};
use serde_json::{Map, Value, json};

use crate::schema_parity::{Case, Expect, Kind, ULID, id, positional, replace_once};

const KIND: Kind = Kind {
    name: "execution_binding",
    schema: SchemaKind::ExecutionBinding,
    pinned_sha256: "1a3496c1483e958f399ba2fc23386396c1b8fa6e38d6a1538307843b2844b705",
    optional_members: &[
        "previous_revision_digest",
        "coven_session_id",
        "event_cursor",
        "terminal_state",
    ],
    doc: "docs/protocol/RECORD-SCHEMAS.md",
    doc_sections: &[
        "parser-only-cases:common",
        "parser-only-cases:execution_binding",
    ],
};

const MISMATCH: ContractError = ContractError::CancellationEvidenceMismatch;
const ACK: &str = "cancellation_acknowledgement";
const UNRESOLVED: &str = "cancellation_unresolved";

/// Unlike the serde-validated kinds, the execution binding keeps the
/// specific error for an identifier of another kind.
fn wrong_kind(field: &'static str, expected: RecordKind) -> ContractError {
    ContractError::WrongRecordKind {
        schema: SchemaKind::ExecutionBinding,
        field,
        expected,
        found: RecordKind::Intent,
    }
}

fn doc(map: &Map<String, Value>) -> Vec<u8> {
    serde_json::to_vec(map).unwrap()
}

fn unresolved_evidence() -> Value {
    json!({
        "disposition_id": "disposition-synthetic-1",
        "termination_request_id": "req_01J00000000000000000000002",
        "session_id": "session-synthetic-1",
        "execution_request_id": "req_01J00000000000000000000001",
        "execution_request_digest":
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "reason_code": "coven_unreachable",
        "recorded_at": "2026-08-01T00:02:00Z",
    })
}

/// The vector moved to another cancellation state with consistent evidence.
fn in_state(state: &str) -> Map<String, Value> {
    let mut map = KIND.vector_object();
    map.insert("cancellation_state".into(), json!(state));
    match state {
        "not_requested" => {
            for member in [
                "termination_request",
                "termination_reason_code",
                ACK,
                UNRESOLVED,
            ] {
                map.insert(member.into(), Value::Null);
            }
        }
        "termination_requested" => {
            map.insert(ACK.into(), Value::Null);
        }
        "acknowledged_already_terminal" => {
            map[ACK]["kind"] = json!("already_authoritatively_terminal");
        }
        "termination_unknown" => {
            map.insert(ACK.into(), Value::Null);
            map.insert(UNRESOLVED.into(), unresolved_evidence());
        }
        _ => {}
    }
    map
}

fn state_doc(state: &str, changes: &[(&str, Value)]) -> Vec<u8> {
    let mut map = in_state(state);
    for (member, value) in changes {
        map.insert((*member).into(), value.clone());
    }
    doc(&map)
}

fn unknown_with(member: &str, value: Option<Value>) -> Vec<u8> {
    let mut map = in_state("termination_unknown");
    let evidence = map[UNRESOLVED].as_object_mut().unwrap();
    match value {
        Some(value) => evidence.insert(member.into(), value),
        None => evidence.remove(member),
    };
    doc(&map)
}

#[allow(clippy::too_many_lines)]
fn corpus() -> Vec<Case> {
    use Expect::{Accept, ParserOnly, Reject};
    let document = KIND.document();
    let mut cases = KIND.standard_cases(&[
        (&["termination_request"], &[], MISMATCH),
        (&[ACK], &[], MISMATCH),
    ]);
    cases.extend(KIND.id_cases(
        &["attempt_id"],
        "att_",
        "int_",
        &wrong_kind("attempt_id", RecordKind::Attempt),
        &document,
    ));
    cases.extend(KIND.id_cases(
        &["familiar_snapshot_id"],
        "ids_",
        "int_",
        &wrong_kind("familiar_snapshot_id", RecordKind::IdentitySnapshot),
        &document,
    ));
    for field in [
        "project_id",
        "coven_contract_version",
        "event_cursor",
        "terminal_state",
    ] {
        cases.extend(KIND.bounded_string_cases_with(
            &[field],
            255,
            &KIND.invalid(field),
            &document,
        ));
    }
    cases.extend(KIND.digest_cases(&["request_digest"], &document));
    cases.extend(KIND.timestamp_cases(&["request_created_at"], &document));
    cases.extend([
        // --- the other cancellation states ---------------------------------
        Case::new(
            "state_not_requested",
            state_doc("not_requested", &[]),
            Accept,
        ),
        Case::new(
            "state_not_requested_members_absent",
            {
                let mut map = in_state("not_requested");
                for member in [
                    "termination_request",
                    "termination_reason_code",
                    ACK,
                    UNRESOLVED,
                ] {
                    map.remove(member);
                }
                doc(&map)
            },
            Reject(document.clone()),
        ),
        Case::new(
            "state_termination_requested",
            state_doc("termination_requested", &[]),
            Accept,
        ),
        Case::new(
            "state_acknowledged_already_terminal",
            state_doc("acknowledged_already_terminal", &[]),
            Accept,
        ),
        Case::new(
            "state_termination_unknown",
            state_doc("termination_unknown", &[]),
            Accept,
        ),
        Case::new(
            "optional_members_absent",
            {
                let mut map = in_state("not_requested");
                for member in KIND.optional_members {
                    map.remove(*member);
                }
                map.insert("revision".into(), json!(1));
                doc(&map)
            },
            Accept,
        ),
        Case::new(
            "adoption_state_fenced",
            KIND.with("adoption_state", json!("fenced")),
            Accept,
        ),
        // --- cancellation members the state forbids or requires --------------
        Case::new(
            "not_requested_with_termination_request",
            state_doc(
                "not_requested",
                &[(
                    "termination_request",
                    KIND.vector_object()["termination_request"].clone(),
                )],
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "not_requested_with_reason_code",
            state_doc(
                "not_requested",
                &[("termination_reason_code", json!("operator_requested"))],
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "termination_requested_with_acknowledgement",
            KIND.with("cancellation_state", json!("termination_requested")),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_without_acknowledgement",
            KIND.with(ACK, Value::Null),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_with_unresolved",
            KIND.with(UNRESOLVED, unresolved_evidence()),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_without_termination_request",
            KIND.with("termination_request", Value::Null),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_without_reason_code",
            KIND.with("termination_reason_code", Value::Null),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_without_session",
            KIND.with("coven_session_id", Value::Null),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_session_absent",
            KIND.without("coven_session_id"),
            Reject(MISMATCH),
        ),
        Case::new(
            "unknown_session_absent",
            {
                let mut map = in_state("termination_unknown");
                map.remove("coven_session_id");
                doc(&map)
            },
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledged_kind_mismatch",
            KIND.with_at(&[ACK, "kind"], json!("already_authoritatively_terminal")),
            Reject(MISMATCH),
        ),
        Case::new(
            "already_terminal_kind_mismatch",
            state_doc(
                "acknowledged_already_terminal",
                &[(ACK, KIND.vector_object()[ACK].clone())],
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "unknown_without_unresolved",
            state_doc("termination_unknown", &[(UNRESOLVED, Value::Null)]),
            Reject(MISMATCH),
        ),
        Case::new(
            "unknown_with_acknowledgement",
            state_doc(
                "termination_unknown",
                &[(ACK, KIND.vector_object()[ACK].clone())],
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "unknown_without_session",
            state_doc("termination_unknown", &[("coven_session_id", Value::Null)]),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledgement_not_an_object",
            KIND.with(ACK, json!("terminated")),
            Reject(MISMATCH),
        ),
        Case::new(
            "reason_code_not_a_string",
            KIND.with("termination_reason_code", json!(7)),
            Reject(MISMATCH),
        ),
        Case::new(
            "reason_code_uppercase",
            KIND.with("termination_reason_code", json!("Operator_requested")),
            Reject(MISMATCH),
        ),
        Case::new(
            "reason_code_double_underscore",
            KIND.with("termination_reason_code", json!("operator__requested")),
            Reject(MISMATCH),
        ),
        Case::new(
            "reason_code_leading_digit",
            KIND.with("termination_reason_code", json!("9operator")),
            Reject(MISMATCH),
        ),
        Case::new(
            "reason_code_128_bytes",
            KIND.with("termination_reason_code", json!("r".repeat(128))),
            Accept,
        ),
        Case::new(
            "reason_code_129_bytes",
            KIND.with("termination_reason_code", json!("r".repeat(129))),
            Reject(MISMATCH),
        ),
        Case::new(
            "authority_evidence_digest_zero",
            KIND.with_at(
                &[ACK, "authority_evidence_digest"],
                json!(format!("sha256:{}", "0".repeat(64))),
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledgement_id_empty",
            KIND.with_at(&[ACK, "acknowledgement_id"], json!("")),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledgement_session_256_bytes",
            KIND.with_at(&[ACK, "session_id"], json!("s".repeat(256))),
            Reject(MISMATCH),
        ),
        Case::new(
            "acknowledgement_kind_unknown",
            KIND.with_at(&[ACK, "kind"], json!("killed")),
            Reject(ContractError::UnknownEnumValue {
                schema: SchemaKind::ExecutionBinding,
                field: "cancellation_acknowledgement.kind",
            }),
        ),
        Case::new(
            "termination_request_wrong_id_prefix",
            KIND.with_at(
                &["termination_request", "termination_request_id"],
                id("att_", ULID),
            ),
            Reject(MISMATCH),
        ),
        Case::new(
            "unknown_unresolved_field",
            unknown_with("aa_unknown_field", Some(json!(true))),
            Reject(MISMATCH),
        ),
        Case::new(
            "unresolved_reason_code_uppercase",
            unknown_with("reason_code", Some(json!("Coven"))),
            Reject(MISMATCH),
        ),
        // --- enums and states ------------------------------------------------
        Case::new(
            "adoption_state_unknown",
            KIND.with("adoption_state", json!("ADOPTED")),
            Reject(ContractError::UnknownEnumValue {
                schema: SchemaKind::ExecutionBinding,
                field: "adoption_state",
            }),
        ),
        Case::new(
            "cancellation_state_unknown",
            KIND.with("cancellation_state", json!("cancelled")),
            Reject(ContractError::UnknownEnumValue {
                schema: SchemaKind::ExecutionBinding,
                field: "cancellation_state",
            }),
        ),
        Case::new(
            "adoption_state_number",
            KIND.with("adoption_state", json!(1)),
            Reject(document.clone()),
        ),
        // --- revision --------------------------------------------------------
        Case::new(
            "revision_1_without_previous",
            KIND.with_all(&[
                (&["revision"], json!(1)),
                (&["previous_revision_digest"], Value::Null),
            ]),
            Accept,
        ),
        Case::new(
            "revision_greatest",
            KIND.with("revision", json!(9_007_199_254_740_991_u64)),
            Accept,
        ),
        Case::new(
            "revision_0",
            KIND.with("revision", json!(0)),
            Reject(KIND.invalid("revision")),
        ),
        Case::new(
            "revision_1_with_previous",
            KIND.with("revision", json!(1)),
            Reject(KIND.invalid("revision")),
        ),
        Case::new(
            "revision_2_without_previous",
            KIND.with("previous_revision_digest", Value::Null),
            Reject(KIND.invalid("revision")),
        ),
        Case::new(
            "revision_2_previous_absent",
            KIND.without("previous_revision_digest"),
            Reject(KIND.invalid("revision")),
        ),
        Case::new(
            "revision_negative",
            KIND.with("revision", json!(-1)),
            Reject(document.clone()),
        ),
        Case::new(
            "revision_unsafe",
            replace_once(
                &KIND.vector(),
                "\"revision\":2",
                "\"revision\":9007199254740992",
            ),
            Reject(ContractError::NonInteroperableNumber),
        ),
        Case::new(
            "revision_integral_fraction",
            replace_once(&KIND.vector(), "\"revision\":2", "\"revision\":2.0"),
            ParserOnly(document.clone()),
        ),
        Case::new(
            "revision_exponent",
            replace_once(&KIND.vector(), "\"revision\":2", "\"revision\":2e0"),
            ParserOnly(document.clone()),
        ),
        // --- revision_created_at must be UTC ---------------------------------
        Case::new(
            "revision_created_at_lowercase_z",
            KIND.with("revision_created_at", json!("2026-08-01t00:02:30z")),
            Accept,
        ),
        Case::new(
            "revision_created_at_negative_zero_offset",
            KIND.with("revision_created_at", json!("2026-08-01T00:02:30-00:00")),
            Accept,
        ),
        Case::new(
            "revision_created_at_positive_zero_offset",
            KIND.with("revision_created_at", json!("2026-08-01T00:02:30+00:00")),
            Accept,
        ),
        Case::new(
            "revision_created_at_offset",
            KIND.with("revision_created_at", json!("2026-08-01T01:02:30+01:00")),
            Reject(KIND.invalid("revision_created_at")),
        ),
        Case::new(
            "revision_created_at_impossible_date",
            KIND.with("revision_created_at", json!("2026-02-30T00:00:00Z")),
            ParserOnly(document.clone()),
        ),
        // --- session bound depends on the cancellation state -------------------
        Case::new(
            "coven_session_id_empty_not_requested",
            state_doc("not_requested", &[("coven_session_id", json!(""))]),
            Reject(KIND.invalid("coven_session_id")),
        ),
        Case::new(
            "coven_session_id_empty_acknowledged",
            KIND.with("coven_session_id", json!("")),
            Reject(MISMATCH),
        ),
        Case::new(
            "coven_session_id_255_bytes_not_requested",
            state_doc(
                "not_requested",
                &[("coven_session_id", json!("s".repeat(255)))],
            ),
            Accept,
        ),
        Case::new(
            "coven_session_id_256_bytes_not_requested",
            state_doc(
                "not_requested",
                &[("coven_session_id", json!("s".repeat(256)))],
            ),
            Reject(KIND.invalid("coven_session_id")),
        ),
        Case::new(
            "coven_session_id_multibyte_over_255_bytes",
            state_doc(
                "not_requested",
                &[("coven_session_id", json!("\u{e9}".repeat(128)))],
            ),
            ParserOnly(KIND.invalid("coven_session_id")),
        ),
        Case::new(
            "request_id_wrong_prefix",
            KIND.with("request_id", id("att_", ULID)),
            Reject(document.clone()),
        ),
        // --- parser-only: bindings and orderings -------------------------------
        Case::new(
            "request_valid_until_not_after_created",
            KIND.with("request_valid_until", json!("2026-08-01T00:00:00Z")),
            ParserOnly(KIND.invalid("request_valid_until")),
        ),
        Case::new(
            "termination_valid_until_not_after_created",
            KIND.with_at(
                &["termination_request", "valid_until"],
                json!("2026-08-01T00:01:00Z"),
            ),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "termination_created_before_request",
            KIND.with_at(
                &["termination_request", "created_at"],
                json!("2026-07-31T23:59:59Z"),
            ),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "termination_request_id_equals_request_id",
            KIND.with_all(&[
                (
                    &["termination_request", "termination_request_id"],
                    json!("req_01J00000000000000000000001"),
                ),
                (
                    &[ACK, "termination_request_id"],
                    json!("req_01J00000000000000000000001"),
                ),
            ]),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledgement_other_termination_request",
            KIND.with_at(
                &[ACK, "termination_request_id"],
                json!("req_01J00000000000000000000003"),
            ),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledgement_other_session",
            KIND.with_at(&[ACK, "session_id"], json!("session-synthetic-2")),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledgement_other_execution_request",
            KIND.with_at(
                &[ACK, "execution_request_id"],
                json!("req_01J00000000000000000000003"),
            ),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledgement_other_request_digest",
            KIND.with_at(
                &[ACK, "execution_request_digest"],
                json!(format!("sha256:{}", "4".repeat(64))),
            ),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledged_before_termination_request",
            KIND.with_at(&[ACK, "acknowledged_at"], json!("2026-08-01T00:00:30Z")),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledged_after_termination_window",
            KIND.with_at(&[ACK, "acknowledged_at"], json!("2026-08-01T00:05:01Z")),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledged_at_window_end",
            KIND.with_at(&[ACK, "acknowledged_at"], json!("2026-08-01T00:05:00Z")),
            Accept,
        ),
        Case::new(
            "unresolved_other_session",
            unknown_with("session_id", Some(json!("session-synthetic-2"))),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "unresolved_recorded_after_window",
            unknown_with("recorded_at", Some(json!("2026-08-01T00:05:01Z"))),
            ParserOnly(MISMATCH),
        ),
        Case::new(
            "acknowledgement_id_multibyte_over_255_bytes",
            KIND.with_at(&[ACK, "acknowledgement_id"], json!("\u{e9}".repeat(128))),
            ParserOnly(MISMATCH),
        ),
    ]);
    let unresolved = unresolved_evidence();
    for member in unresolved.as_object().unwrap().keys() {
        cases.push(Case::new(
            format!("missing_{UNRESOLVED}_{member}"),
            unknown_with(member, None),
            Reject(MISMATCH),
        ));
    }
    let mut unknown = in_state("termination_unknown");
    let evidence = unknown[UNRESOLVED].as_object().unwrap().clone();
    unknown.insert(
        UNRESOLVED.into(),
        positional(
            &evidence,
            &[
                "disposition_id",
                "termination_request_id",
                "session_id",
                "execution_request_id",
                "execution_request_digest",
                "reason_code",
                "recorded_at",
            ],
        ),
    );
    cases.extend([
        Case::new(
            "adoption_state_as_single_key_object",
            KIND.as_single_key_object(&["adoption_state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "cancellation_state_as_single_key_object",
            KIND.as_single_key_object(&["cancellation_state"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "cancellation_acknowledgement.kind_as_single_key_object",
            KIND.as_single_key_object(&[ACK, "kind"]),
            Expect::DecoderOnly,
        ),
        Case::new(
            "termination_request_as_positional_array",
            KIND.as_positional_array(
                &["termination_request"],
                &["termination_request_id", "created_at", "valid_until"],
            ),
            Expect::DecoderOnly,
        ),
        Case::new(
            "cancellation_acknowledgement_as_positional_array",
            KIND.as_positional_array(
                &[ACK],
                &[
                    "acknowledgement_id",
                    "termination_request_id",
                    "session_id",
                    "execution_request_id",
                    "execution_request_digest",
                    "kind",
                    "authority_evidence_digest",
                    "acknowledged_at",
                ],
            ),
            Expect::DecoderOnly,
        ),
        Case::new(
            "cancellation_unresolved_as_positional_array",
            doc(&unknown),
            Expect::DecoderOnly,
        ),
        Case::new(
            "cancellation_state_unknown_as_single_key_object",
            KIND.with("cancellation_state", json!({"paused": null})),
            Reject(ContractError::InvalidShape {
                schema: SchemaKind::ExecutionBinding,
                field: "document",
            }),
        ),
        Case::new(
            "acknowledgement_kind_unknown_as_single_key_object",
            KIND.with_at(&[ACK, "kind"], json!({"revoked": null})),
            Reject(MISMATCH),
        ),
    ]);
    cases
}

parity_tests!(
    sweep: [
        (["attempt_id"], true),
        (["familiar_snapshot_id"], true),
        (["previous_revision_digest"], true),
        ([ACK, "authority_evidence_digest"], true),
        (["revision_created_at"], false),
        (["request_created_at"], false),
        (["request_valid_until"], false),
        (["termination_request", "created_at"], false),
        ([ACK, "acknowledged_at"], false),
    ],
    min_sweep: 35_000
);

#[test]
fn the_schema_names_exactly_the_nested_evidence_members() {
    KIND.check_nested_members(&["termination_request"], &KIND.vector(), &[]);
    KIND.check_nested_members(&[ACK], &KIND.vector(), &[]);
    KIND.check_nested_members(&[UNRESOLVED], &state_doc("termination_unknown", &[]), &[]);
}

#[test]
fn the_adoption_state_enum_matches_the_decoder() {
    KIND.check_enum(
        &["adoption_state"],
        &[
            "not_submitted",
            "submitting",
            "adopted",
            "proven_not_adopted",
            "adoption_unknown",
            "fenced",
        ],
    );
}

#[test]
fn the_cancellation_state_enum_matches_the_decoder() {
    KIND.check_enum(
        &["cancellation_state"],
        &[
            "not_requested",
            "termination_requested",
            "acknowledged_terminated",
            "acknowledged_already_terminal",
            "termination_unknown",
        ],
    );
}

#[test]
fn the_acknowledgement_kind_enum_matches_the_decoder() {
    KIND.check_enum(
        &[ACK, "kind"],
        &["terminated", "already_authoritatively_terminal"],
    );
}

/// Members the decoder binds by equality refuse every substitution made to
/// one of them alone, so they are swept together: the same substitution at
/// every bound member. The acknowledged vector and a termination-unknown
/// binding each cover their own evidence member.
#[test]
fn bound_members_sweep_together() {
    const TERMINATION_ID: &[&str] = &["termination_request", "termination_request_id"];
    let acknowledged = KIND.vector_object();
    let checked = KIND.sweep_groups(
        &acknowledged,
        &[
            (vec![&["request_id"], &[ACK, "execution_request_id"]], false),
            (
                vec![&["request_digest"], &[ACK, "execution_request_digest"]],
                false,
            ),
            (
                vec![TERMINATION_ID, &[ACK, "termination_request_id"]],
                false,
            ),
            (vec![&["termination_reason_code"]], true),
        ],
    );
    let unknown = in_state("termination_unknown");
    let checked = checked
        + KIND.sweep_groups(
            &unknown,
            &[
                (
                    vec![&["request_id"], &[UNRESOLVED, "execution_request_id"]],
                    false,
                ),
                (
                    vec![
                        &["request_digest"],
                        &[UNRESOLVED, "execution_request_digest"],
                    ],
                    false,
                ),
                (
                    vec![TERMINATION_ID, &[UNRESOLVED, "termination_request_id"]],
                    false,
                ),
                (vec![&[UNRESOLVED, "reason_code"]], true),
                (vec![&[UNRESOLVED, "recorded_at"]], false),
            ],
        );
    assert!(checked >= 40_000, "sweep too small: {checked}");
}
