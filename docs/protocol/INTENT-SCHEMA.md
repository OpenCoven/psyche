# Experimental `psyche.intent.v1` JSON Schema

**Status:** experimental and unpublished, under [#11](https://github.com/OpenCoven/psyche/issues/11). This is not a release, not a stable contract and not a selection of the publication surface. [#60](https://github.com/OpenCoven/psyche/pull/60) proposes intent as a `stable-v1` target, and that selection still belongs to the #11 owner.

The schema is [`schemas/psyche.intent.v1.schema.json`](../../schemas/psyche.intent.v1.schema.json) (Draft 2020-12, `$id` `urn:opencoven:psyche:schema:psyche.intent.v1`). It describes the documents that `psyche_core::contracts::decode_document` accepts today. **The typed decoder is the authority on validity.** The schema is meant to accept every document the decoder accepts. It also accepts the parser-only cases below, which only the decoder can refuse. A consumer that validates with the schema alone must therefore treat a pass as "structurally plausible", never as "valid".

## Verifying

```sh
cargo test --locked -p psyche-core --test intent_schema -- --nocapture
```

The test needs no network. The validator ([boon](https://crates.io/crates/boon) 0.6, a dev-dependency of `psyche-core` only) carries the 2020-12 metaschema and is given a loader that refuses every URL. The corpus is derived in code from the #50 vector `crates/psyche-core/tests/fixtures/vectors/psyche.intent.v1.json` (SHA-256 `8ef6700f6d72a7666b57cc051342bc04491c47782f73950cc60fb2666071f5ed`, as pinned in `SHA256SUMS`). Each case is one mutation of that vector, or a deliberate rewrite of its spelling, and pins the decoder's exact `ContractError` and the schema's verdict:

| Test | What it proves |
|---|---|
| `the_schema_compiles_offline_against_draft_2020_12` | The schema is a valid 2020-12 document and resolves without loading anything. |
| `the_schema_names_exactly_the_typed_members` | `properties` equals the members of the typed record's canonical encoding, `required` is every member except `surface_event_id`, unknown members are refused and `schema_version` is the exact constant. This is the drift guard for a hand-written file. |
| `the_pinned_vector_keeps_its_bytes_and_digest_through_both` | The vector matches its #50 digest and checksum line, decodes, re-encodes to identical canonical bytes and passes the schema. |
| `schema_and_decoder_agree_on_the_corpus` | Positive cases pass both. Rejection cases fail both, with the decoder's error pinned. Four of them (empty, truncated, trailing data, trailing comma) are not JSON text and fail before the schema runs. Parser-only cases fail only the decoder. Parser-dependent cases fail the decoder and this test's JSON parser. |
| `the_canonical_form_of_every_accepted_case_passes_both` | The canonical re-encoding of every accepted case passes the schema and is a fixed point of decode and encode. |
| `single_character_sweeps_never_reject_what_the_decoder_accepts` | Every single-character substitution, drawn from all 128 ASCII characters and four non-ASCII ones, at every position of the three IDs, the digest and `created_at`. For the IDs and the digest the two verdicts must be equal. For `created_at` the schema must accept whatever the decoder accepts. |
| `every_parser_only_case_is_documented` | The table below lists exactly the corpus's parser-only and parser-dependent cases. |
| `every_rejection_case_mutates_the_vector` | No rejection case is accidentally a byte-for-byte copy of the vector. |

The rejection corpus covers:
- unknown members and every missing required member;
- unsupported majors (`v0`, `v2`), and unknown and other schema kinds;
- wrong ID prefixes on all three ID members, and an unregistered prefix;
- malformed ULIDs (lowercase, overflowing first character, excluded letter, wrong length);
- empty and over-length strings and lists, and wrong JSON types;
- unsafe integers in `constraints`, including nested ones;
- malformed digests and timestamps;
- malformed input (empty, truncated, trailing data, trailing comma, non-object root).

## What the schema states

- Exactly the eleven members, `additionalProperties: false`, `schema_version` equal to `psyche.intent.v1`.
- `intent_id`, `familiar_snapshot_id` and `surface_event_id` use their record prefixes (`int_`, `ids_`, `sev_`) followed by a canonical uppercase ULID whose first character is `0`–`7`. `surface_event_id` may be `null` or absent.
- Lengths: `principal_id` and `project_id` are 1–255 long, `requested_outcome` 1–16384, and each `required_evidence` item and each `constraints` key 1–256. `required_evidence` holds at most 1024 items, duplicates allowed.
- `constraints` is an object whose values may be any JSON. Every integer in it, at any depth, must lie within ±(2^53−1). An integral number such as `1e20` or `9007199254740992.0` counts as an integer.
- `digest` is `sha256:` and 64 lowercase hex digits.
- `created_at` matches the RFC 3339 shape the decoder accepts. Field ranges are bounded. Calendar validity is not. The date and time may be separated by any single ASCII character, because that is what the decoder's `time` parser accepts.

Patterns use only ASCII character classes and escapes, so ECMA-262 and Rust `regex` read them identically. Python's `re` lets `$` match before a final newline, so a Python validator may accept a trailing newline in `created_at` or `digest` that the decoder and this test refuse (cases `created_at_trailing_newline` and `digest_trailing_newline`). The schema avoids `format: date-time` because it is annotation-only in 2020-12, and an asserting validator would refuse separators that the decoder accepts.

## Parser-only rules

Each row is a corpus case that the decoder refuses and a schema-only consumer may not. Rows marked *parser-dependent* are bytes this test's JSON parser refuses but RFC 8259 leaves to the implementation. Common parsers accept some of them. The table is checked by `every_parser_only_case_is_documented`.

<!-- parser-only-cases:start -->
| Case | Decoder result | Why a structural schema cannot state it |
|---|---|---|
| `duplicate_top_level_key` | `InvalidShape { Error, "json" }` | JSON Schema sees the parsed value, and an ordinary parser keeps one of the duplicates. |
| `duplicate_constraints_key` | `InvalidShape { Error, "json" }` | Same, inside free-form `constraints`. |
| `nesting_beyond_limit` | `InvalidShape { Error, "json" }` | Values more than 64 levels below the root are refused while parsing. `nesting_at_limit` is the accepted side of the boundary. |
| `constraints_number_overflow` | `NonInteroperableNumber` | `1e400` overflows an IEEE 754 double. A validator compares it as a number and cannot see the overflow. |
| `principal_id_multibyte_over_255_bytes` | `InvalidShape { Intent, "document" }` | The decoder bounds UTF-8 bytes. `maxLength` counts code points, so the two agree only for ASCII. A byte bound always implies the code-point bound, so the schema never rejects a document for length that the decoder accepts. |
| `project_id_multibyte_over_255_bytes` | `InvalidShape { Intent, "document" }` | Byte bound, as above. |
| `requested_outcome_multibyte_over_16384_bytes` | `InvalidShape { Intent, "document" }` | Byte bound, as above. |
| `required_evidence_item_multibyte_over_256_bytes` | `InvalidShape { Intent, "document" }` | Byte bound, as above. |
| `constraints_key_multibyte_over_256_bytes` | `InvalidShape { Intent, "document" }` | Byte bound, as above. |
| `created_at_impossible_date` | `InvalidShape { Intent, "document" }` | Calendar validity (`2026-02-30`). |
| `created_at_leap_second_off_boundary` | `InvalidShape { Intent, "document" }` | A second of `60` is accepted only where the `time` crate places a leap second. |
| `document_over_1_mib` | `DocumentTooLarge` | Raw input over 1 MiB. |
| `constraints_canonical_over_1_mib` | `InvalidShape { Intent, "document" }` | The RFC 8785 encoding of `constraints` over 1 MiB, although the raw input is smaller. |
| `byte_order_mark` | `InvalidShape { Error, "json" }` | *Parser-dependent.* RFC 8259 lets a parser ignore a leading BOM. |
| `invalid_utf8` | `InvalidShape { Error, "json" }` | *Parser-dependent.* Some parsers substitute U+FFFD. |
| `lone_surrogate_escape` | `InvalidShape { Error, "json" }` | *Parser-dependent.* `"\ud800"` is accepted by JavaScript `JSON.parse` and Python `json`. |
<!-- parser-only-cases:end -->

Also outside the schema, and not refused by either implementation: the decoder does not recompute `digest` (case `digest_not_recomputed`).

## Decoder behaviour recorded by this corpus

These are current behaviours, pinned so that any change is deliberate. They are inputs to the disposition #60 asks for, not decisions made here. [Decoder report](https://github.com/OpenCoven/psyche/issues/11#issuecomment-7078559204)

- **Lost denial detail.** A wrong-kind ID prefix and an oversized canonical `constraints` reach the caller as `InvalidShape { Intent, "document" }`. `Intent::validate` builds `WrongRecordKind` and `InvalidShape { Intent, "constraints" }`, but it runs inside `Deserialize`, and `decode` maps every serde error to `"document"`. A published denial vocabulary (#11 decision D5) would need the specific error to survive.
- **Timestamps are not normalized to one spelling.** Any single ASCII character separates date and time, including `t`, a space, a digit, a tab and NUL, and each re-encodes as `T`. `z` re-encodes as `Z`, and so does `-00:00`. Any other offset survives into the canonical bytes, so the same instant has several digests. More than nine fractional digits are truncated. `2026-12-31T23:59:60Z` re-encodes as `2026-12-31T23:59:59.999999999Z`.
- **Optional member.** An absent `surface_event_id` decodes like `null`, and both re-encode as `null`.
- **Free-form numbers.** `-0`, fractional values and `1e-400` in `constraints` are accepted.

## Rollback and non-goals

Rolling back is reverting the commit. The slice adds the schema, this document, one test file and one dev-dependency. It changes no record, decoder, vector, fixture or wire format, and no shipped crate depends on the validator.

Not in scope: publishing or signing the schema, adding it to the #68 kit, schemas for the other fifteen kinds, a generator, or any change to decoder behaviour.
