# Experimental record JSON Schemas

**Status:** experimental and unpublished, under [#11](https://github.com/OpenCoven/psyche/issues/11). This is not a release, not a stable contract and not a selection of the publication surface. [#60](https://github.com/OpenCoven/psyche/pull/60) proposes which kinds are `stable-v1` targets, and that selection still belongs to the #11 owner.

Each schema in [`schemas/`](../../schemas) describes the documents of one kind that `psyche_core::contracts::decode_document` accepts today, in Draft 2020-12, with `$id` `urn:opencoven:psyche:schema:psyche.<kind>.v1`. **The typed decoder is the authority on validity.** Each schema accepts every document the decoder accepts, in the encoding the decoder itself emits, and also accepts the parser-only cases below, which only the decoder can refuse. The one exception runs the other way: the decoder also takes two non-canonical encodings from serde, which six schemas reject on purpose. These are the [decoder-only cases](#decoder-only-cases). A consumer that validates with a schema alone must therefore treat a pass as "structurally plausible", never as "valid". The intent schema came first and has its own page, [INTENT-SCHEMA.md](INTENT-SCHEMA.md). This page covers the other kinds in the #50 vector set.

## Verifying

```sh
cargo test --locked -p psyche-core --test record_schemas
```

The test needs no network. It shares the harness `crates/psyche-core/tests/schema_parity/mod.rs` with `intent_schema`: the validator ([boon](https://crates.io/crates/boon) 0.6, a dev-dependency of `psyche-core` only) carries the 2020-12 metaschema and is given a loader that refuses every URL. Each kind's corpus is derived in code from its #50 vector in `crates/psyche-core/tests/fixtures/vectors/`, at the digest pinned in `SHA256SUMS`. Each case is one mutation of that vector, or a deliberate rewrite of its spelling, and pins the decoder's exact `ContractError` and the schema's verdict.

Every kind runs the same tests, in its own module of `crates/psyche-core/tests/record_schemas/`:

| Test | What it proves |
|---|---|
| `the_schema_compiles_offline_against_draft_2020_12` | The schema is a valid 2020-12 document and resolves without loading anything. |
| `the_schema_names_exactly_the_typed_members` | `properties` equals the members of the typed record's canonical encoding, `required` is every member except the optional ones, unknown members are refused and `schema_version` is the exact constant. Closed nested objects have their own `..._members` test. This is the drift guard for a hand-written file. |
| `the_pinned_vector_keeps_its_bytes_and_digest_through_both` | The vector matches its #50 digest and checksum line, decodes as its kind, re-encodes to identical canonical bytes and passes the schema. |
| `schema_and_decoder_agree_on_the_corpus` | Positive cases pass both. Rejection cases fail both, with the decoder's error pinned. Parser-only cases fail only the decoder. Parser-dependent cases fail the decoder and this test's JSON parser. Decoder-only cases pass only the decoder. |
| `the_canonical_form_of_every_accepted_case_passes_both` | The canonical re-encoding of every accepted and decoder-only case passes the schema and is a fixed point of decode and encode. A decoder-only case re-encodes to different bytes. |
| `single_character_sweeps_never_reject_what_the_decoder_accepts` | Every single-character substitution, drawn from all 128 ASCII characters and four non-ASCII ones, at every position of the kind's identifiers, digests and timestamps. Where the schema states the decoder's whole rule the two verdicts must be equal. Elsewhere the schema must accept whatever the decoder accepts, and the decoder must accept at least one substitution, or the sweep would check nothing. Members the decoder binds by equality are swept together, the same substitution at each (`bound_members_sweep_together` in the execution binding). |
| `every_parser_only_case_is_documented` | The shared table and the kind's table below together list exactly the kind's parser-only and parser-dependent cases, and the kind's decoder-only table, where it has one, lists exactly its decoder-only cases. |
| `every_non_canonical_form_the_decoder_accepts_is_declared` | Rewrites every string member of the vector and of every accepted case as a one-member object, and every object member of up to eight members as an array of its values in every order. The forms the decoder accepts and the schema rejects are exactly the kind's decoder-only cases. An object with more members must be open in the schema. |
| `every_rejection_case_mutates_the_vector` | No rejection case is accidentally a byte-for-byte copy of the vector. |

Kinds with a closed enum also run `..._enum` tests: the schema's `enum` equals the decoder's vocabulary, no listed value is refused as unknown, and the schema accepts every document the decoder accepts with each value.

Every corpus covers unknown members at the top level and in each closed nested object, every missing required member, unsupported majors (`v0`, `v2`), unknown and other schema kinds, and malformed input (empty, truncated, trailing data, trailing comma, non-object roots). Kind modules add wrong and unregistered ID prefixes, malformed ULIDs and digests, length bounds, integer bounds and spellings, timestamps, enum values and the kind's cross-member rules.

## What every schema states

- Exactly the kind's members, `additionalProperties: false` at the top level and in every closed nested object, and `schema_version` equal to `psyche.<kind>.v1`.
- Record identifiers use their kind's prefix followed by a canonical uppercase ULID whose first character is `0`–`7`. Request identifiers use `req_` the same way.
- Digests are `sha256:` and 64 lowercase hex digits.
- Timestamps match the RFC 3339 shape the decoder accepts, as in the intent schema: field ranges are bounded and calendar validity is not, and the date and time may be separated by any single ASCII character.
- String lengths are the decoder's bounds, as `minLength` and `maxLength`.
- Typed integers carry the decoder's range, and never exceed 2^53−1. Free-form values may be any JSON, with every integer at any depth within ±(2^53−1).

Patterns use only ASCII character classes and escapes, so ECMA-262 and Rust `regex` read them identically. Python's `re` lets `$` match before a final newline, so a Python validator may accept a trailing newline that the decoder and this test refuse (the `..._trailing_newline` cases). The schemas avoid `format: date-time` for the reason INTENT-SCHEMA.md gives.

## Parser-only rules shared by every kind

Each row is a corpus case that every kind carries. The decoder refuses it and a schema-only consumer may not. Rows marked *parser-dependent* are bytes this test's JSON parser refuses but RFC 8259 leaves to the implementation.

<!-- parser-only-cases:common:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `duplicate_top_level_key` | `InvalidShape { Error, "json" }` | JSON Schema sees the parsed value, and an ordinary parser keeps one of the duplicates. |
| `document_over_1_mib` | `DocumentTooLarge` | Raw input over 1 MiB. Here the vector is padded with whitespace, which a parsed value does not retain. |
| `byte_order_mark` | `InvalidShape { Error, "json" }` | *Parser-dependent.* RFC 8259 lets a parser ignore a leading BOM. |
| `invalid_utf8` | `InvalidShape { Error, "json" }` | *Parser-dependent.* Some parsers substitute U+FFFD. |
| `lone_surrogate_escape` | `InvalidShape { Error, "json" }` | *Parser-dependent.* `"\ud800"` is accepted by JavaScript `JSON.parse` and Python `json`. |
<!-- parser-only-cases:common:end -->

Three rules recur in the kind tables:

- **Byte bounds.** The decoder bounds UTF-8 bytes. `maxLength` counts code points, so the two agree only for ASCII. A byte bound always implies the code-point bound, so a schema never rejects a document for length that the decoder accepts. Each bounded member has a `..._multibyte_over_<n>_bytes` case.
- **Integer spelling.** The decoder reads a typed integer member only from an integer literal. JSON Schema calls `3.0` and `3e0` integers, so the `..._integral_fraction` and `..._exponent` cases are parser-only. Free-form values accept both spellings.
- **Calendar.** `..._impossible_date` (`2026-02-30`) and `..._leap_second_off_boundary` (a second of `60` where the `time` crate places no leap second).

## Decoder-only cases

The schemas describe each document in the encoding the decoder emits. `decode_document` also accepts two other encodings, which `serde_json::Value`'s deserializer allows and the decoder does not refuse:

- **A unit enum variant as a one-member object.** `"state": {"running": null}` decodes as `"state": "running"`. Enum inspection looks only at strings, so this form also skips it. An unknown value in this form, such as `{"paused": null}`, is refused later, as a shape error and not as `UnknownEnumValue` (`..._unknown_as_single_key_object`).
- **A closed object as a positional array.** `"provenance": ["home:synthetic-1", "resolver-synthetic-1"]` decodes as `familiar_home_id` and `resolver_version`, in the order the Rust type declares its fields. Where two members share a type, the array decodes in either order, with the values bound to whichever member sits at that position.

The six affected schemas reject both forms on purpose. Describing them would publish the Rust field order and serde's enum encoding as part of the contract. It would also mean rewriting each `if`/`then` condition, which compares a state with `const`: `{"not_requested": null}` takes the `else` branch. The canonical re-encoding of every such case is the ordinary form, which passes the schema. Each affected kind lists its cases in a decoder-only table, and `every_non_canonical_form_the_decoder_accepts_is_declared` searches every member of every accepted case for any form the decoder accepts that is not listed. The same search over the intent vector finds none (`intent_has_no_decoder_only_form`), so [INTENT-SCHEMA.md](INTENT-SCHEMA.md) needs no such table. Finding 9 below records the consequences for the #11 owner.

## Kinds

Kinds are ordered by #60's classification. "#11 minimum" names the kinds that carry a fact in the minimum consumer profile #11 lists for closing. No kind carries result and artifact binding: #60 defers that fact (row 14) and no #50 kind represents it.

| Kind | #60 class | #11 minimum | Schema |
|---|---|---|---|
| `identity_snapshot` | `stable-v1` target | none | [`psyche.identity_snapshot.v1.schema.json`](../../schemas/psyche.identity_snapshot.v1.schema.json) |
| `surface_event` | `stable-v1` target | none | [`psyche.surface_event.v1.schema.json`](../../schemas/psyche.surface_event.v1.schema.json) |
| `execution_binding` | `experimental` | fencing (`fenced`), cancellation acknowledgement, unresolved outcomes | [`psyche.execution_binding.v1.schema.json`](../../schemas/psyche.execution_binding.v1.schema.json) |
| `recovery` | `experimental` | fencing (`fence_token`) | [`psyche.recovery.v1.schema.json`](../../schemas/psyche.recovery.v1.schema.json) |
| `graph` | `experimental` | none | [`psyche.graph.v1.schema.json`](../../schemas/psyche.graph.v1.schema.json) |
| `graph_node` | `experimental` | none; its `termination_unknown` state names a result deferred row 12 produces | [`psyche.graph_node.v1.schema.json`](../../schemas/psyche.graph_node.v1.schema.json) |
| `delegation` | `experimental` | none | [`psyche.delegation.v1.schema.json`](../../schemas/psyche.delegation.v1.schema.json) |
| `budget` | `experimental` | none | [`psyche.budget.v1.schema.json`](../../schemas/psyche.budget.v1.schema.json) |
| `approval` | `experimental` | none | [`psyche.approval.v1.schema.json`](../../schemas/psyche.approval.v1.schema.json) |
| `evidence` | `experimental` | none | [`psyche.evidence.v1.schema.json`](../../schemas/psyche.evidence.v1.schema.json) |
| `verdict` | `experimental` | none | [`psyche.verdict.v1.schema.json`](../../schemas/psyche.verdict.v1.schema.json) |
| `addon` | `experimental` | none | [`psyche.addon.v1.schema.json`](../../schemas/psyche.addon.v1.schema.json) |
| `error` | `experimental` | none | [`psyche.error.v1.schema.json`](../../schemas/psyche.error.v1.schema.json) |
| `surface_effect` | `excluded` | none | [`psyche.surface_effect.v1.schema.json`](../../schemas/psyche.surface_effect.v1.schema.json) |
| `delivery` | `excluded` | none | [`psyche.delivery.v1.schema.json`](../../schemas/psyche.delivery.v1.schema.json) |

## `identity_snapshot`

**States:** `snapshot_id` with `ids_`. `familiar_id`, `principal_id` and both `provenance` members are 1–255 long. `revision` is 1 to 2^53−1. The five digests and `resolved_at` use the shared rules. `provenance` is closed.

<!-- parser-only-cases:identity_snapshot:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `familiar_id_multibyte_over_255_bytes` | `InvalidShape { IdentitySnapshot, "document" }` | Byte bound. |
| `principal_id_multibyte_over_255_bytes` | `InvalidShape { IdentitySnapshot, "document" }` | Byte bound. |
| `provenance.familiar_home_id_multibyte_over_255_bytes` | `InvalidShape { IdentitySnapshot, "document" }` | Byte bound. |
| `provenance.resolver_version_multibyte_over_255_bytes` | `InvalidShape { IdentitySnapshot, "document" }` | Byte bound. |
| `revision_integral_fraction` | `InvalidShape { IdentitySnapshot, "document" }` | Integer spelling. |
| `revision_exponent` | `InvalidShape { IdentitySnapshot, "document" }` | Integer spelling. |
| `resolved_at_impossible_date` | `InvalidShape { IdentitySnapshot, "document" }` | Calendar. |
| `resolved_at_leap_second_off_boundary` | `InvalidShape { IdentitySnapshot, "document" }` | Calendar. |
<!-- parser-only-cases:identity_snapshot:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:identity_snapshot:start -->
| Case | Form |
|---|---|
| `provenance_as_positional_array` | `[familiar_home_id, resolver_version]` |
<!-- decoder-only-cases:identity_snapshot:end -->

**Decoder behaviour recorded:** every rejection after the version probe, including a wrong-kind `snapshot_id` prefix, surfaces as `InvalidShape { IdentitySnapshot, "document" }`. As for intent, `validate()` runs inside `Deserialize` and its specific error is lost. The digests are not recomputed.

## `surface_event`

**States:** `surface_event_id` with `sev_`. `adapter_id` and `account_id` are 1–256 long. `actor`, `locator` and `content` are objects, possibly empty, whose keys are unbounded and whose values are free-form. `adapter_event_digest` and `received_at` use the shared rules.

<!-- parser-only-cases:surface_event:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `adapter_id_multibyte_over_256_bytes` | `InvalidShape { SurfaceEvent, "document" }` | Byte bound. |
| `account_id_multibyte_over_256_bytes` | `InvalidShape { SurfaceEvent, "document" }` | Byte bound. |
| `received_at_impossible_date` | `InvalidShape { SurfaceEvent, "document" }` | Calendar. |
| `received_at_leap_second_off_boundary` | `InvalidShape { SurfaceEvent, "document" }` | Calendar. |
| `duplicate_locator_key` | `InvalidShape { Error, "json" }` | A duplicate key inside a free-form object, as at the top level. |
| `content_nesting_beyond_limit` | `InvalidShape { Error, "json" }` | Values more than 64 levels below the root are refused while parsing. `content_nesting_at_limit` is the accepted side. |
| `content_number_overflow` | `NonInteroperableNumber` | `1e400` overflows an IEEE 754 double. A validator compares it as a number and cannot see the overflow. |
| `content_canonical_over_1_mib` | `InvalidShape { SurfaceEvent, "document" }` | The RFC 8785 encoding of one free-form member over 1 MiB, although the raw input is smaller. |
<!-- parser-only-cases:surface_event:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { SurfaceEvent, "document" }`. The free-form members accept `-0`, fractions and keys of any length, including the empty key.

## `execution_binding`

**#11 minimum.** This kind carries three facts from the minimum consumer profile #11 lists for closing: the `fenced` adoption state (fencing, held row 11 in #60), and `cancellation_acknowledgement` and `cancellation_unresolved` (cancellation acknowledgement and unresolved outcomes, deferred row 12). The schema describes their current shape only and gives them no published meaning.

**States:** `attempt_id` with `att_`, `familiar_snapshot_id` with `ids_`, and `request_id` and every evidence request ID with `req_`. `project_id`, `coven_contract_version`, `coven_session_id`, `event_cursor`, `terminal_state` and the evidence `acknowledgement_id`, `disposition_id` and `session_id` are 1–255 long. `revision` is 1 to 2^53−1. `revision_created_at` must be UTC (`Z`, `z`, `+00:00` or `-00:00`). Reason codes are lowercase ASCII words joined by single underscores, starting with a letter, at most 128 long. `authority_evidence_digest` may not be all zeros. The three evidence objects are closed. Through `allOf` conditions, the schema also states:

- `revision` 1 has no `previous_revision_digest`, and any later revision has one.
- `not_requested` carries all four cancellation members as `null`. Every other state carries `termination_request` and `termination_reason_code`.
- `termination_requested` carries no acknowledgement or unresolved evidence.
- `acknowledged_terminated` and `acknowledged_already_terminal` carry an acknowledgement of the matching `kind`, no unresolved evidence and a `coven_session_id`.
- `termination_unknown` carries unresolved evidence, no acknowledgement and a `coven_session_id`.

`previous_revision_digest`, `coven_session_id`, `event_cursor` and `terminal_state` may be absent. The four cancellation members may be `null` but not absent.

<!-- parser-only-cases:execution_binding:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `project_id_multibyte_over_255_bytes` | `InvalidShape { ExecutionBinding, "project_id" }` | Byte bound. |
| `coven_contract_version_multibyte_over_255_bytes` | `InvalidShape { ExecutionBinding, "coven_contract_version" }` | Byte bound. |
| `coven_session_id_multibyte_over_255_bytes` | `InvalidShape { ExecutionBinding, "coven_session_id" }` | Byte bound. |
| `event_cursor_multibyte_over_255_bytes` | `InvalidShape { ExecutionBinding, "event_cursor" }` | Byte bound. |
| `terminal_state_multibyte_over_255_bytes` | `InvalidShape { ExecutionBinding, "terminal_state" }` | Byte bound. |
| `acknowledgement_id_multibyte_over_255_bytes` | `CancellationEvidenceMismatch` | Byte bound, inside the acknowledgement. |
| `revision_integral_fraction` | `InvalidShape { ExecutionBinding, "document" }` | Integer spelling. |
| `revision_exponent` | `InvalidShape { ExecutionBinding, "document" }` | Integer spelling. |
| `request_created_at_impossible_date` | `InvalidShape { ExecutionBinding, "document" }` | Calendar. |
| `request_created_at_leap_second_off_boundary` | `InvalidShape { ExecutionBinding, "document" }` | Calendar. |
| `revision_created_at_impossible_date` | `InvalidShape { ExecutionBinding, "document" }` | Calendar. |
| `request_valid_until_not_after_created` | `InvalidShape { ExecutionBinding, "request_valid_until" }` | An ordering between two members. `request_valid_until` must be later than `request_created_at`. |
| `termination_valid_until_not_after_created` | `CancellationEvidenceMismatch` | Ordering: the termination request's `valid_until` must be later than its `created_at`. |
| `termination_created_before_request` | `CancellationEvidenceMismatch` | Ordering: the termination request may not predate the execution request. |
| `termination_request_id_equals_request_id` | `CancellationEvidenceMismatch` | Inequality between two members: the termination request needs its own ID. |
| `acknowledgement_other_termination_request` | `CancellationEvidenceMismatch` | Equality binding: the evidence must name the binding's termination request. |
| `acknowledgement_other_session` | `CancellationEvidenceMismatch` | Equality binding: the evidence session must equal `coven_session_id`. |
| `acknowledgement_other_execution_request` | `CancellationEvidenceMismatch` | Equality binding: the evidence must name `request_id`. |
| `acknowledgement_other_request_digest` | `CancellationEvidenceMismatch` | Equality binding: the evidence must carry `request_digest`. |
| `acknowledged_before_termination_request` | `CancellationEvidenceMismatch` | Ordering: evidence must fall inside the termination request's window. |
| `acknowledged_after_termination_window` | `CancellationEvidenceMismatch` | Ordering, as above. `acknowledged_at_window_end` is the accepted side. |
| `unresolved_other_session` | `CancellationEvidenceMismatch` | Equality binding, for unresolved evidence. |
| `unresolved_recorded_after_window` | `CancellationEvidenceMismatch` | Ordering, for unresolved evidence. |
<!-- parser-only-cases:execution_binding:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:execution_binding:start -->
| Case | Form |
|---|---|
| `adoption_state_as_single_key_object` | `{"adopted": null}` |
| `cancellation_state_as_single_key_object` | `{"acknowledged_terminated": null}` |
| `cancellation_acknowledgement.kind_as_single_key_object` | `{"terminated": null}` |
| `termination_request_as_positional_array` | `[termination_request_id, created_at, valid_until]` |
| `cancellation_acknowledgement_as_positional_array` | `[acknowledgement_id, termination_request_id, session_id, execution_request_id, execution_request_digest, kind, authority_evidence_digest, acknowledged_at]` |
| `cancellation_unresolved_as_positional_array` | `[disposition_id, termination_request_id, session_id, execution_request_id, execution_request_digest, reason_code, recorded_at]` |
<!-- decoder-only-cases:execution_binding:end -->

**Decoder behaviour recorded:**

- **Denial detail survives.** This is the one record kind that decodes its wire form first and then validates. A wrong-kind `attempt_id` or `familiar_snapshot_id` surfaces as `WrongRecordKind`, and a bound failure names its member, as in `InvalidShape { ExecutionBinding, "project_id" }`. Only a shape failure inside serde, such as a malformed ID or a wrong JSON type, becomes `InvalidShape { ExecutionBinding, "document" }`.
- **Cancellation errors collapse to one.** Every failure in the cancellation members, from an unknown evidence member to a wrong session, surfaces as `CancellationEvidenceMismatch`. The one exception is an unknown `cancellation_acknowledgement.kind`, which is refused as `UnknownEnumValue` before decoding.
- **State-dependent error for one bound.** An empty or over-long `coven_session_id` is `InvalidShape { ExecutionBinding, "coven_session_id" }` in `not_requested` and `CancellationEvidenceMismatch` in any other state.
- **Absent and null differ.** The four cancellation members use a custom deserializer, so the decoder refuses them when absent (`state_not_requested_members_absent`) although it accepts `null`. The other four optional members may be absent.
- **`fenced` has no further rule.** The decoder accepts `adoption_state: "fenced"` with any other member values (`adoption_state_fenced`).

## `recovery`

**#11 minimum.** `fence_token` carries the process-attempt fence (fencing, held row 11 in #60). The schema describes its current shape only and gives it no published meaning.

**States:** `recovery_id` with `rcv_` and `attempt_id` with `att_`. `lease_id` and `fence_token` are 1–255 long, and `ambiguity` and `operator_disposition` 1–256. `reconciliation_count` is 0 to 2^53−1. `fence_token` and `operator_disposition` may be `null` or absent.

<!-- parser-only-cases:recovery:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `lease_id_multibyte_over_255_bytes` | `InvalidShape { Recovery, "document" }` | Byte bound. |
| `fence_token_multibyte_over_255_bytes` | `InvalidShape { Recovery, "document" }` | Byte bound. |
| `ambiguity_multibyte_over_256_bytes` | `InvalidShape { Recovery, "document" }` | Byte bound. |
| `operator_disposition_multibyte_over_256_bytes` | `InvalidShape { Recovery, "document" }` | Byte bound. |
| `reconciliation_count_integral_fraction` | `InvalidShape { Recovery, "document" }` | Integer spelling. |
| `reconciliation_count_exponent` | `InvalidShape { Recovery, "document" }` | Integer spelling. |
<!-- parser-only-cases:recovery:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Recovery, "document" }`. `ambiguity` and `operator_disposition` are free text, not enums (`ambiguity_free_text`), although the vector spells `ambiguity` like an execution binding state.

## `graph`

**States:** `graph_id` with `grf_` and `root_intent_id` with `int_`. `owner_principal_id` and `policy_revision` are 1–255 long. `state` is one of the eleven graph states. `version` is 1 to 2^53−1.

<!-- parser-only-cases:graph:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `owner_principal_id_multibyte_over_255_bytes` | `InvalidShape { Graph, "document" }` | Byte bound. |
| `policy_revision_multibyte_over_255_bytes` | `InvalidShape { Graph, "document" }` | Byte bound. |
| `version_integral_fraction` | `InvalidShape { Graph, "document" }` | Integer spelling. |
| `version_exponent` | `InvalidShape { Graph, "document" }` | Integer spelling. |
<!-- parser-only-cases:graph:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:graph:start -->
| Case | Form |
|---|---|
| `state_as_single_key_object` | `{"waiting_evidence": null}` |
<!-- decoder-only-cases:graph:end -->

**Decoder behaviour recorded:** an unknown `state` string is refused as `UnknownEnumValue { Graph, "state" }` before decoding. A non-string `state` and every other rejection surface as `InvalidShape { Graph, "document" }`. No transition rule is checked: every state decodes alongside the vector's other members (`every_state_decodes_with_the_vector_members`).

## `graph_node`

**States:** `node_id` with `nod_`, `graph_id` with `grf_`, `familiar_snapshot_id` with `ids_`, `delegation_id` with `dlg_` and `budget_id` with `bud_`. `dependencies` is a list of `nod_` identifiers with no length bound, duplicates allowed. `required_evidence` holds at most 1024 items, each 1–256 long. `state` is one of the twenty-two node states. `version` is 1 to 2^53−1. `delegation_id` may be `null` or absent.

<!-- parser-only-cases:graph_node:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `required_evidence_item_multibyte_over_256_bytes` | `InvalidShape { GraphNode, "document" }` | Byte bound. |
| `version_integral_fraction` | `InvalidShape { GraphNode, "document" }` | Integer spelling. |
| `version_exponent` | `InvalidShape { GraphNode, "document" }` | Integer spelling. |
<!-- parser-only-cases:graph_node:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:graph_node:start -->
| Case | Form |
|---|---|
| `state_as_single_key_object` | `{"awaiting_verification": null}` |
<!-- decoder-only-cases:graph_node:end -->

**Decoder behaviour recorded:**

- An unknown `state` string is refused as `UnknownEnumValue { GraphNode, "state" }`, and every other rejection as `InvalidShape { GraphNode, "document" }`. Every state decodes alongside the vector's other members (`every_state_decodes_with_the_vector_members`).
- **Enum inspection follows the declared kind.** The same document relabelled `psyche.graph.v1` is refused as `UnknownEnumValue { Graph, "state" }`, because `awaiting_verification` is not a graph state, before its node members are seen (`schema_version_graph_with_node_state`).
- `dependencies` may name the node itself, repeat an entry or hold 2048 entries (`dependencies_self`, `dependencies_duplicates`, `dependencies_2048_items`). Only the 1 MiB document limit bounds it.

## `delegation`

**States:** `delegation_id` with `dlg_`, `parent_node_id` and `child_node_id` with `nod_`, and `budget_id` with `bud_`. `scope_digest` and `evidence_scope_digest` use the shared rule. `cancellation_policy` is 1–256 long.

<!-- parser-only-cases:delegation:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `cancellation_policy_multibyte_over_256_bytes` | `InvalidShape { Delegation, "document" }` | Byte bound. |
<!-- parser-only-cases:delegation:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Delegation, "document" }`. The decoder checks no relation between members: a node may delegate to itself (`parent_equals_child`), and `cancellation_policy` is free text. #60 asks for narrowed-scope and widening vectors, which need a policy this kind does not carry.

## `budget`

**States:** `budget_id` with `bud_` and `graph_id` with `grf_`. `resource_class` is 1–256 long. `limit`, `reserved`, `consumed` and `released` are each 0 to 2^53−1.

<!-- parser-only-cases:budget:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `resource_class_multibyte_over_256_bytes` | `InvalidShape { Budget, "document" }` | Byte bound. |
| `limit_integral_fraction` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `limit_exponent` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `reserved_integral_fraction` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `reserved_exponent` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `consumed_integral_fraction` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `consumed_exponent` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `released_integral_fraction` | `InvalidShape { Budget, "document" }` | Integer spelling. |
| `released_exponent` | `InvalidShape { Budget, "document" }` | Integer spelling. |
<!-- parser-only-cases:budget:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Budget, "document" }`. The amounts are independent: `consumed` above `limit`, and `reserved` and `released` above a zero `limit`, all decode (`consumed_above_limit`, `reserved_and_released_above_limit`), as #60 noted. An enforceable-limit rule would be a decoder change, not a schema one.

## `approval`

**States:** `approval_id` with `apr_` and `node_id` with `nod_`. `requester_principal_id` is 1–255 long. `decision` is 1–256 long, `null` or absent. `expires_at` uses the shared rule.

<!-- parser-only-cases:approval:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `requester_principal_id_multibyte_over_255_bytes` | `InvalidShape { Approval, "document" }` | Byte bound. |
| `decision_multibyte_over_256_bytes` | `InvalidShape { Approval, "document" }` | Byte bound. |
| `expires_at_impossible_date` | `InvalidShape { Approval, "document" }` | Calendar. |
| `expires_at_leap_second_off_boundary` | `InvalidShape { Approval, "document" }` | Calendar. |
<!-- parser-only-cases:approval:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Approval, "document" }`. `decision` is free text, not an enum (`decision_free_text`), and an `expires_at` in the past decodes (`expires_at_in_the_past`): expiry is not checked at decode time. #60's row 7 lists what an approval does not yet carry.

## `evidence`

**States:** `evidence_id` with `evd_`, `node_id` with `nod_` and `attempt_id` with `att_`. `content_digest` and `created_at` use the shared rules. `producer`, `collection_method`, `media_type` and `retention_policy` are 1–256 long. `size` is 0 to 2^53−1.

<!-- parser-only-cases:evidence:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `producer_multibyte_over_256_bytes` | `InvalidShape { Evidence, "document" }` | Byte bound. |
| `collection_method_multibyte_over_256_bytes` | `InvalidShape { Evidence, "document" }` | Byte bound. |
| `media_type_multibyte_over_256_bytes` | `InvalidShape { Evidence, "document" }` | Byte bound. |
| `retention_policy_multibyte_over_256_bytes` | `InvalidShape { Evidence, "document" }` | Byte bound. |
| `size_integral_fraction` | `InvalidShape { Evidence, "document" }` | Integer spelling. |
| `size_exponent` | `InvalidShape { Evidence, "document" }` | Integer spelling. |
| `created_at_impossible_date` | `InvalidShape { Evidence, "document" }` | Calendar. |
| `created_at_leap_second_off_boundary` | `InvalidShape { Evidence, "document" }` | Calendar. |
<!-- parser-only-cases:evidence:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Evidence, "document" }`. `media_type` is not checked as a media type, and `content_digest` is not recomputed. #60 asks for a content and correlation mismatch pair, which needs the evidence bytes and the attempt, neither of which this kind carries.

## `verdict`

**States:** `verdict_id` with `vrd_` and `node_id` with `nod_`. `sealed_evidence_digest` and `created_at` use the shared rules. `policy_revision`, `verdict_type`, `reviewer_id` and `outcome` are 1–256 long. `reason_codes` holds at most 1024 items, each 1–256 long.

<!-- parser-only-cases:verdict:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `policy_revision_multibyte_over_256_bytes` | `InvalidShape { Verdict, "document" }` | Byte bound. |
| `verdict_type_multibyte_over_256_bytes` | `InvalidShape { Verdict, "document" }` | Byte bound. |
| `reviewer_id_multibyte_over_256_bytes` | `InvalidShape { Verdict, "document" }` | Byte bound. |
| `outcome_multibyte_over_256_bytes` | `InvalidShape { Verdict, "document" }` | Byte bound. |
| `reason_codes_item_multibyte_over_256_bytes` | `InvalidShape { Verdict, "document" }` | Byte bound. |
| `created_at_impossible_date` | `InvalidShape { Verdict, "document" }` | Calendar. |
| `created_at_leap_second_off_boundary` | `InvalidShape { Verdict, "document" }` | Calendar. |
<!-- parser-only-cases:verdict:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Verdict, "document" }`. `outcome` and `verdict_type` are free text, not enums (`outcome_free_text`), and `reason_codes` items need not follow the execution binding's reason-code shape (`reason_codes_free_text`). The two kinds spell reason codes differently.

## `addon`

**States:** `addon_id` with `adn_`. `package`, `version` and `revocation_state` are 1–256 long. The four digests use the shared rule.

<!-- parser-only-cases:addon:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `package_multibyte_over_256_bytes` | `InvalidShape { Addon, "document" }` | Byte bound. |
| `version_multibyte_over_256_bytes` | `InvalidShape { Addon, "document" }` | Byte bound. |
| `revocation_state_multibyte_over_256_bytes` | `InvalidShape { Addon, "document" }` | Byte bound. |
<!-- parser-only-cases:addon:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { Addon, "document" }`. `version` is not checked as a version (`version_not_semver`), and `revocation_state` is free text (`revocation_state_free_text`).

## `error`

**Not a record.** An error envelope is never stored and has no identifier. No producer row in #60 covers it, and whether it becomes the serialized rejection result (#60 slice 3) is undecided.

**States:** `error` is a closed object. `code` is one of the thirty-six error codes. `message` is 1–4096 long and `correlation_id` 1–255. `retryable` is a boolean. `details` maps at most 128 keys, each 1–256 long, to strings 1–4096 long.

<!-- parser-only-cases:error:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `error.message_multibyte_over_4096_bytes` | `InvalidShape { Error, "message" }` | Byte bound. |
| `error.correlation_id_multibyte_over_255_bytes` | `InvalidShape { Error, "correlation_id" }` | Byte bound. |
| `details_key_multibyte_over_256_bytes` | `InvalidShape { Error, "details.key" }` | Byte bound. |
| `details_value_multibyte_over_4096_bytes` | `InvalidShape { Error, "details.value" }` | Byte bound. |
| `duplicate_details_key` | `InvalidShape { Error, "json" }` | A duplicate key inside `details`, as at the top level. |
<!-- parser-only-cases:error:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:error:start -->
| Case | Form |
|---|---|
| `error_as_positional_array` | `[code, correlation_id, retryable, message, details]` |
<!-- decoder-only-cases:error:end -->

**Decoder behaviour recorded:** like the execution binding, the envelope decodes first and then validates, so bound failures name their member (`InvalidShape { Error, "message" }`, `InvalidShape { Error, "correlation_id" }`, `InvalidShape { Error, "details" }`, `InvalidShape { Error, "details.key" }`, `InvalidShape { Error, "details.value" }`). Shape failures surface as `InvalidShape { Error, "document" }`, and an unknown `code` as `UnknownEnumValue { Error, "code" }`. Because the envelope's own schema kind is `Error`, its denials share a `schema` value with the probe and JSON errors (`InvalidShape { Error, "json" }`, `InvalidShape { Error, "schema_version" }`) that every kind returns. A consumer cannot tell from `schema` alone whether an error envelope or some other document was refused.

## `surface_effect`

**Excluded** from the first profile by #60, because row 13 is held. The schema describes the current decoder only.

**States:** `surface_effect_id` with `sfx_`, `intent_id` with `int_`, `graph_id` with `grf_`, `node_id` with `nod_`, `attempt_id` with `att_` and `familiar_snapshot_id` with `ids_`. `project_id`, `action_class` and `account_id` are 1–256 long. `locator` and `effect` are objects, possibly empty, with free-form values. `effect_digest` and `created_at` use the shared rules.

<!-- parser-only-cases:surface_effect:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `project_id_multibyte_over_256_bytes` | `InvalidShape { SurfaceEffect, "document" }` | Byte bound. |
| `action_class_multibyte_over_256_bytes` | `InvalidShape { SurfaceEffect, "document" }` | Byte bound. |
| `account_id_multibyte_over_256_bytes` | `InvalidShape { SurfaceEffect, "document" }` | Byte bound. |
| `created_at_impossible_date` | `InvalidShape { SurfaceEffect, "document" }` | Calendar. |
| `created_at_leap_second_off_boundary` | `InvalidShape { SurfaceEffect, "document" }` | Calendar. |
| `effect_digest_not_of_effect` | `InvalidShape { SurfaceEffect, "document" }` | `effect_digest` must equal the SHA-256 of the RFC 8785 encoding of `effect`. A schema cannot compute a digest. |
| `effect_changed_without_its_digest` | `InvalidShape { SurfaceEffect, "document" }` | The same rule, broken from the other side. |
<!-- parser-only-cases:surface_effect:end -->

**Decoder behaviour recorded:** every rejection after the version probe surfaces as `InvalidShape { SurfaceEffect, "document" }`. The decoder recomputes `effect_digest` (`the_effect_digest_is_the_sha256_of_the_canonical_effect` pins the construction), unlike the intent and identity digests, which it does not check.

## `delivery`

**Excluded** from the first profile by #60, because row 13 is held. The schema describes the current decoder only.

**States:** `delivery_id` with `del_` and `intent_id` with `int_`. `action_class`, `account_id`, `logical_response_id`, both `topic` members and the `surface_decision` labels are 1–256 long. `chat_id` is an optionally negative ASCII decimal and `telegram_message_id` an unsigned one, each at most 32 long. `relationship`, `state` and `surface_decision.state` are closed enums. `effect` is a non-empty object with free-form values. `logical_part` and `attempt_count` are 0 to 2^32−1. `topic` and `surface_decision` are closed. Through `if`/`then`, the schema states that `sent` carries a `telegram_message_id` and every other state carries none.

<!-- parser-only-cases:delivery:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `action_class_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `account_id_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `logical_response_id_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `topic.kind_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `topic.id_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `surface_decision.decision_id_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `surface_decision.policy_revision_multibyte_over_256_bytes` | `InvalidShape { Delivery, "document" }` | Byte bound. |
| `logical_part_integral_fraction` | `InvalidShape { Delivery, "document" }` | Integer spelling. |
| `logical_part_exponent` | `InvalidShape { Delivery, "document" }` | Integer spelling. |
| `attempt_count_integral_fraction` | `InvalidShape { Delivery, "document" }` | Integer spelling. |
| `attempt_count_exponent` | `InvalidShape { Delivery, "document" }` | Integer spelling. |
| `surface_decision.expires_at_impossible_date` | `InvalidShape { Delivery, "document" }` | Calendar. |
| `surface_decision.expires_at_leap_second_off_boundary` | `InvalidShape { Delivery, "document" }` | Calendar. |
| `effect_digest_not_of_effect` | `InvalidShape { Delivery, "document" }` | `effect_digest` must equal the SHA-256 of the RFC 8785 encoding of `effect`. |
<!-- parser-only-cases:delivery:end -->

[Decoder-only cases](#decoder-only-cases): the decoder accepts these and the schema rejects them.

<!-- decoder-only-cases:delivery:start -->
| Case | Form |
|---|---|
| `relationship_as_single_key_object` | `{"reply_same_topic": null}` |
| `state_as_single_key_object` | `{"sent": null}` |
| `surface_decision.state_as_single_key_object` | `{"consumed": null}` |
| `topic_as_positional_array` | `[id, kind]` |
| `surface_decision_as_positional_array` | `[decision_id, request_digest, policy_revision, expires_at, state]` |
<!-- decoder-only-cases:delivery:end -->

**Decoder behaviour recorded:** unknown enum strings are refused as `UnknownEnumValue` for `relationship`, `state` and `surface_decision.state`, and every other rejection surfaces as `InvalidShape { Delivery, "document" }`. Decimals may carry leading zeros (`chat_id_leading_zeros`). As for graph_node, the same document relabelled `psyche.graph.v1` is refused for its `state` before its members are seen (`schema_version_graph_with_delivery_state`).

## Findings for the #11 owner

These are current decoder behaviours across all sixteen kinds, including intent, pinned so that any change is deliberate. They are inputs to #11 decision D5 (a published denial vocabulary) and to the disposition #60 asks for, not decisions made here.

1. **Denial detail is lost for fourteen of the sixteen kinds.** Thirteen record kinds and intent run `validate()` inside `Deserialize`, and `decode_document` maps every serde error to `InvalidShape { <kind>, "document" }`. A wrong-kind identifier, a bound failure and a wrong JSON type are indistinguishable. Only the execution binding and the error envelope decode first and validate after, so their errors name the member or say `WrongRecordKind`. A published vocabulary would need the other kinds to do the same, or the vocabulary to stop at `document`.
2. **Cancellation failures collapse to one error.** In the execution binding, every failure in the cancellation members surfaces as `CancellationEvidenceMismatch`, from a missing evidence member to a wrong session. Only an unknown acknowledgement `kind` is distinguishable.
3. **Enum inspection precedes the typed decode and follows the declared kind.** After the size, JSON and version checks, an unknown enum *string* is refused as `UnknownEnumValue` before the typed decode sees any other member. A document relabelled as another kind is refused for that kind's enum first, when it has one (`schema_version_graph_with_node_state`, `schema_version_graph_with_delivery_state`). An enum value in the object form of finding 9 is not inspected at all.
4. **Error envelopes share `schema: Error` with probe and JSON errors.** A consumer cannot tell from `schema` alone whether an error envelope was malformed or some other document failed to parse.
5. **Absent and null differ for four members.** The execution binding's cancellation members may be `null` but not absent. Every other optional member in every kind may be either.
6. **Typed integers refuse `3.0` and `3e0`.** Free-form values accept both spellings. A consumer that normalizes number spelling can turn a valid document into an invalid one.
7. **`effect_digest` is the only digest recomputed.** In the surface effect and delivery it must equal the digest of `effect`. The execution binding also compares digests with each other (each evidence `execution_request_digest` must equal `request_digest`) and refuses an all-zero `authority_evidence_digest`. Every other digest member is taken as given.
8. **No cross-record or transition rule is checked at decode time.** Budgets may be overdrawn, approvals may be expired, nodes may depend on themselves, and any graph or node state decodes with any version. Those rules, if they exist, belong to the store or to policy owners that #60 lists as missing.
9. **The decoder accepts two non-canonical encodings.** A unit enum variant may be a one-member object and a closed object may be a positional array, in six kinds ([decoder-only cases](#decoder-only-cases)). Three consequences follow. Two documents that are different JSON values, not merely different spellings of one value, decode to the same record, so RFC 8785 canonicalization of the input alone does not identify a record. An array binds values to members by the Rust field order, which no contract states, and two members of one type may be swapped without any error. An unknown enum value in object form is not refused as `UnknownEnumValue`: it surfaces as `InvalidShape` (or `CancellationEvidenceMismatch` for an acknowledgement `kind`), and `Store::ingest`, which quarantines only unknown schemas, majors and enum values, returns it as a contract error instead of quarantining it. Refusing both forms in the decoder would remove all three, and is a decoder change outside this slice.

## Coverage and blocked kinds

Every #50 kind now has a schema and a parity corpus: intent on [INTENT-SCHEMA.md](INTENT-SCHEMA.md), and the fifteen others above. No kind was blocked. None needed an owner decision to describe its current decoder, because each schema states only what the decoder does today and gives no member a published meaning.

The #11 minimum names facts, not kinds. Fencing (`execution_binding.adoption_state: "fenced"`, `recovery.fence_token`), cancellation acknowledgement and unresolved outcomes (`execution_binding`) have a described shape here and no published meaning, because #60 holds or defers their rows (11 and 12). Result and artifact binding (row 14) has no #50 kind, so this slice cannot describe it.

## Rollback and non-goals

Rolling back is reverting the commits. The slice adds schemas, this document, one shared test harness and one test target. It changes no record, decoder, vector, fixture or wire format, adds no dependency, and no shipped crate depends on the schemas.

Not in scope: publishing or signing a schema, adding schemas to the #68 kit, selecting which kinds are stable, a generator, or any change to decoder behaviour.
