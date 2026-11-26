# Experimental record JSON Schemas

**Status:** experimental and unpublished, under [#11](https://github.com/OpenCoven/psyche/issues/11). This is not a release, not a stable contract and not a selection of the publication surface. [#60](https://github.com/OpenCoven/psyche/pull/60) proposes which kinds are `stable-v1` targets, and that selection still belongs to the #11 owner.

Each schema in [`schemas/`](../../schemas) describes the documents of one kind that `psyche_core::contracts::decode_document` accepts today, in Draft 2020-12, with `$id` `urn:opencoven:psyche:schema:psyche.<kind>.v1`. **The typed decoder is the authority on validity.** Each schema is meant to accept every document the decoder accepts. It also accepts the parser-only cases below, which only the decoder can refuse. A consumer that validates with a schema alone must therefore treat a pass as "structurally plausible", never as "valid". The intent schema came first and has its own page, [INTENT-SCHEMA.md](INTENT-SCHEMA.md). This page covers the other kinds in the #50 vector set.

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
| `schema_and_decoder_agree_on_the_corpus` | Positive cases pass both. Rejection cases fail both, with the decoder's error pinned. Parser-only cases fail only the decoder. Parser-dependent cases fail the decoder and this test's JSON parser. |
| `the_canonical_form_of_every_accepted_case_passes_both` | The canonical re-encoding of every accepted case passes the schema and is a fixed point of decode and encode. |
| `single_character_sweeps_never_reject_what_the_decoder_accepts` | Every single-character substitution, drawn from all 128 ASCII characters and four non-ASCII ones, at every position of the kind's identifiers, digests and timestamps. Where the schema states the decoder's whole rule the two verdicts must be equal. Elsewhere the schema must accept whatever the decoder accepts. |
| `every_parser_only_case_is_documented` | The shared table and the kind's table below together list exactly the kind's parser-only and parser-dependent cases. |
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
- `not_requested` carries no cancellation member. Every other state carries `termination_request` and `termination_reason_code`.
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

**Decoder behaviour recorded:**

- An unknown `state` string is refused as `UnknownEnumValue { GraphNode, "state" }`, and every other rejection as `InvalidShape { GraphNode, "document" }`. Every state decodes alongside the vector's other members (`every_state_decodes_with_the_vector_members`).
- **Enum inspection follows the declared kind.** The same document relabelled `psyche.graph.v1` is refused as `UnknownEnumValue { Graph, "state" }`, because `awaiting_verification` is not a graph state, before its node members are seen (`schema_version_graph_with_node_state`).
- `dependencies` may name the node itself, repeat an entry or hold 2048 entries (`dependencies_self`, `dependencies_duplicates`, `dependencies_2048_items`). Only the 1 MiB document limit bounds it.
