# G2 consumer publication inventory

Survey of Psyche main `b1fe944d8e50d0faa3e83567325f06fce60cd1dc` for #11, checked against source on 22 November 2026. The current decoder knows sixteen v1 document kinds and fifteen durable record prefixes. All sixteen rows are **experimental** for external publication. The approved stable and deprecated sets are empty. Storage, harness and diagnostic internals are identified below. [Registry](../../crates/psyche-core/src/contracts/mod.rs) · [Schema notes](../SCHEMAS.md) · [Ownership decisions](https://github.com/OpenCoven/psyche/issues/12)

## State of publication

Main publishes no protocol artifact. It has no schema file, vector set or standalone runner, and the repository manifest lists `psyche.protocol.v1` as planned (`agent/manifest.yaml:72`). Every input named below is a test fixture or a Rust test that needs a Psyche checkout and Cargo. No registry row has a pinned byte-exact vector.

Three open pull requests, as read on 22 November 2026, add experimental profiles under `profiles/`. Their heads moved during this survey, so the table names no revision. Each describes its content as experimental. None is merged, none is the stable v1 profile, and none changes a crate or adds a registry kind. This inventory makes no claim about their correctness.

| Pull request | Adds | Stated scope |
|---|---|---|
| [#39](https://github.com/OpenCoven/psyche/pull/39) | `profiles/receipt-replay/**` | Offline receipt replay profile with its own schema string, `psyche.experimental.receipt-replay.v1`. Binds the launch golden to a consumer-owned receipt and compares observations before and after a restart. |
| [#43](https://github.com/OpenCoven/psyche/pull/43), draft | a later revision of `profiles/receipt-replay/**` | Combined source for #39 on the store base after #40. Its description says it corrects the profile's fixture, qualification script, README and archive pin. |
| [#41](https://github.com/OpenCoven/psyche/pull/41), draft | `profiles/adapter-lifecycle/**` | External adapter lifecycle kit that embeds the #39 receipt checker. |

Their descriptions tie stable publication to the #12 decisions. #12 is open, and its proposed decision packet is draft [#29](https://github.com/OpenCoven/psyche/pull/29).

## Records and prospective authority

Each row spells a schema as `psyche.<kind>.v1` (`crates/psyche-core/src/contracts/mod.rs:442`). The same file maps a kind to its record kind (`crates/psyche-core/src/contracts/mod.rs:475`) and a record kind to its prefix (`crates/psyche-core/src/contracts/mod.rs:357`). An execution binding is stored as the Attempt record, and the error envelope has no record kind and is refused by the store (`crates/psyche-store/src/records.rs:143`). All rows share strict fields and enum values and a canonical uppercase ULID for each Psyche record ID. Producer names describe the orchestration record and the distinct authority it references.

The Defined column is relative to `crates/psyche-core/src/contracts/`. Test references are relative to `crates/psyche-core/tests/` unless a full path is given, and fixture files are in its `fixtures/` directory. A round trip compares the typed document's canonical bytes with the canonical bytes of the same file read as untyped JSON, so it pins no literal. Inline is a document built inside a test and passed through `decode_document`. Typed only means a test deserializes the Rust type directly and the kind never passes through `decode_document` successfully.

Every row still needs a schema document, a canonical vector file with a pinned SHA-256, consumer-facing rejection vectors and the approved ownership revision. The last column lists the authority dependency and what the row needs beyond that.

| Kind | Prefix | Defined | Existing fixture and tests | Class | Producer, authority dependency and remaining Phase 1 obligation |
|---|---|---|---|---|---|
| identity_snapshot | ids_ | `identity.rs:21` | No file. Inline at `contracts.rs:171`, revision bound at `contracts.rs:1483`. | experimental | Psyche snapshot of Familiar Contract identity/person binding. Need mismatched binding and the historical rebind disposition. |
| intent | int_ | `intent.rs:14` | `intent-local.json`, decoded at `contracts.rs:253`, unknown field rejected at `contracts.rs:242`. No round trip. Store replay and tamper tests use a typed intent (`crates/psyche-store/tests/records.rs:375`). | experimental | Psyche after principal resolution. Need a round trip of the file and a published unknown-field rejection. |
| surface_event | sev_ | `surface.rs:15` | `surface-event.json`, round trip at `contracts.rs:313`, rejections at `contracts.rs:372`. | experimental | Psyche envelope, surface adapter for the observed event. Publish malformed actor, locator and oversized content rejections. |
| graph | grf_ | `graph.rs:29` | No file. Typed only (`contracts.rs:1493`). Unknown state rejected at `decode.rs:130`. | experimental | Psyche graph admission and lifecycle. Need a first decoded positive, impossible binding shape and unknown state. |
| graph_node | nod_ | `graph.rs:98` | `node-root.json`, decoded at `contracts.rs:253` and `contracts.rs:1507`, unknown state rejected at `decode.rs:150`. No round trip. | experimental | Psyche orchestration. Need a round trip of the file, unknown state and wrong related ID. |
| delegation | dlg_ | `foundation.rs:25` | No file. Inline at `contracts.rs:344`. | experimental | Psyche scope projection with applicable upstream authority. Need narrowed scope and widening vectors. |
| budget | bud_ | `foundation.rs:66` | No file. Typed only (`contracts.rs:1517`). | experimental | Psyche accounting record, resource enforcement belongs to its provider. Need a first decoded positive, an enforceable limit and widened reservation cases. |
| approval | apr_ | `foundation.rs:94` | No file. Inline with a null decision at `contracts.rs:191`. | experimental | Psyche lifecycle referencing the named decision authority. Need consume-once and stale authority cases. |
| execution_binding | att_ | `execution.rs:224` | No file. Built by `valid_binding` (`contracts.rs:701`), Attempt identity at `contracts.rs:361`, cancellation evidence at `contracts.rs:793`. Revision chain at `crates/psyche-store/tests/records.rs:1266`. | experimental | Psyche Attempt record, Coven admission/session/adoption/termination evidence. Need portable stale correlation, ambiguity, crash and fencing cases. |
| evidence | evd_ | `foundation.rs:124` | No file. Inline at `contracts.rs:202`, size bound at `contracts.rs:1558`. | experimental | Psyche correlation, originating authority supplies evidence. Need content/correlation mismatch pair. |
| verdict | vrd_ | `foundation.rs:161` | No file. Inline at `contracts.rs:218`. | experimental | Psyche record referencing reviewer and policy revision. Need sealed-evidence mismatch and authority cases. |
| recovery | rcv_ | `foundation.rs:197` | No file. Typed only (`contracts.rs:1564`). | experimental | Psyche recovery record, fence issuer and enforcement awaiting #12/#19. Need a first decoded positive and unresolved/reconciled paths with surviving resource coverage. |
| addon | adn_ | `foundation.rs:225` | None. No test builds, decodes or stores an addon document. Only the schema string is exercised (`decode.rs:278`). | experimental | Psyche registration, policy for provenance/allowlist/revocation deferred. Need a first positive of any kind and a revoked or altered contribution pair. |
| surface_effect | sfx_ | `surface.rs:61` | `surface-effect.json`, round trip at `contracts.rs:313`, rejections at `contracts.rs:372`. | experimental | Psyche effect correlation, Threads protected-surface authorization. Publish wrong ID, altered digest and malformed payload rejections. |
| delivery | del_ | `surface.rs:180` | `delivery-ready.json`, round trip at `contracts.rs:301`, prefix at `contracts.rs:329`, rejections at `contracts.rs:436`. Store round trip of a typed delivery at `crates/psyche-store/tests/records.rs:356`. | experimental | Psyche delivery ledger, adapter observes external outcome. Publish invalid state, sent without message ID and wrong prefix. |
| error | — | `error.rs:173` | `error-storage-unavailable.json` (`decode.rs:94`) and `error-codes-v1.json`, 36 envelopes decoded one at a time (`contracts.rs:515`). Store refusal at `crates/psyche-store/tests/records.rs:416`. | experimental | Psyche structured envelope, causal authority supplies fact. Publish unknown code, shape and persistence rejections. |

The following documents and identifiers exist outside the registry. A released profile must either include or explicitly exclude each one.

| Name | Defined | Existing vector or test | Class |
|---|---|---|---|
| `psyche.execution_request.v1` | `crates/psyche-coven/src/port.rs:17`, type at `crates/psyche-coven/src/port.rs:140` | Launch and input goldens with pinned bytes and digests (`crates/psyche-coven/tests/request_digest.rs:13`, `crates/psyche-coven/tests/request_digest.rs:48`). `decode_document` rejects it as an unknown schema. | experimental |
| result bundle, no schema string | `crates/psyche-coven/src/port.rs:899` | `result-bundle.json` pinned byte for byte (`crates/psyche-coven/tests/bindings.rs:39`). No digest of the bundle is pinned. | experimental |
| `req_` request ID | `crates/psyche-core/src/id.rs:139` | `crates/psyche-core/src/id.rs:265` | ephemeral, never stored |
| `qua_` quarantine ID | `crates/psyche-store/src/quarantine.rs:10` | `crates/psyche-store/tests/retention.rs:165` | internal to the store |
| `psyche.config.v1` | `crates/psyche-core/src/schema.rs:21` | `crates/psyche-core/src/schema.rs:60` | not classified, operator configuration |
| `psyche.status.v1`, `psyche.doctor.v1` | `crates/psyche-cli/src/status.rs:24`, `crates/psyche-cli/src/doctor.rs:89` | `crates/psyche-cli/tests/cli.rs:186`, `crates/psyche-cli/tests/cli.rs:347` | not classified, CLI output |

Request and quarantine IDs use the same canonical ULID suffix grammar as record IDs. Coven session, artifact and disposition IDs and opaque fence tokens retain producer-specific identity. A released profile must specify how downstream task, lane, action, receipt and lease IDs relate to the registry. Main defines no separate record prefix for any of those five names. A lease ID appears only as a bounded string on the recovery record (`crates/psyche-core/src/contracts/foundation.rs:201`), a fence token is an opaque string there and in the port's fenced disposition (`crates/psyche-coven/src/port.rs:590`), and no crate source mentions a receipt. [ID parsers](../../crates/psyche-core/src/id.rs) · [Quarantine](../../crates/psyche-store/src/quarantine.rs) · [Reference-client slices](https://github.com/OpenCoven/psyche/issues/13)

## Canonical bytes and digests

These observations use each source document after `decode_document`, then `canonical_bytes` and `digest`. A second decode and canonicalization reproduced identical bytes, and an independent sorted-key compact serialization of the raw files outside Rust gave the same lengths and digests. The six pretty-printed input files differ from their canonical representations. No test or file in the repository contains these six digests, so they are measurements and not pins. The separate error-code fixture contains an array of 36 individual envelopes. Consumer pins must identify the approved package and its individual files.

| Producer input under crates/psyche-core/tests/fixtures | Canonical bytes | SHA-256 of canonical typed document |
|---|---:|---|
| intent-local.json | 479 | b6c6e3cdec0628b2a588abac4cb567a697031fd955fc542f3f8a1f974dcc5bfb |
| node-root.json | 340 | aab8c7902cfdeb07a55c4a0823e1cac2c8a94f71dce7dfd01ae3332ce4499036 |
| delivery-ready.json | 975 | c4f7b567b2abbc9ec94f50a13ee42f5b8eba276033ace75c218ad1f472bbdbf4 |
| error-storage-unavailable.json | 227 | 6d58339a99ecde35000d22d49b5d4f8085be83188b87d09efd5368b43b469377 |
| surface-event.json | 431 | bab220ec9dd03e9ddfc7187cfe028bcf2980164bb91fc2393d09917c905c0488 |
| surface-effect.json | 666 | e98c8ee8f28a6d2c3757bd7f2f030c6cbf87cba1eaa1f7797052dc8579d4d507 |

Two experimental Coven port vectors are already byte-exact canonical JSON without a trailing newline. The launch request hashes to `75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04`, and the input request to `c8c3d0cad99f65d0fdac7b2bb577cf1278412a7ea6255d443e45394109311c61`. Both bind operation-specific fields, ordered artifact requirements and RFC3339 timestamps. They are the only whole-document digests asserted as literals in the crates. The nested effect digests in `surface-effect.json` and `delivery-ready.json` are recomputed on every decode (`crates/psyche-core/src/contracts/surface.rs:113`, `crates/psyche-core/src/contracts/surface.rs:229`). `result-bundle.json` supplies the separate result-binding input. [Golden assertions](../../crates/psyche-coven/tests/request_digest.rs) · [Result assertions](../../crates/psyche-coven/tests/bindings.rs)

Protocol digests have the literal prefix `sha256:` followed by 64 lowercase hexadecimal characters. Typed structured content uses RFC 8785 canonical serialization. Identity pins include declaration, identity file, identity, soul and role/skill digests. Delegations bind scope and evidence scope. Addons carry package, provenance, contributions and allowlist digests. Evidence binds content and verdicts bind sealed evidence. Surface envelopes bind adapter event, effect and decision request digests. Adoption binds context manifest, delegation, budget, payload, artifact requirements and the complete request. Reconciliation binds the ambiguity and request. Cancellation binds request and authority evidence. Results bind raw content, media type, size, correlation and lifetime. Record revisions bind prior canonical revision bytes. Store-only integrity fields bind transition tuples, full rejected raw input, retained prefixes, quarantine metadata and resolution details. A digest comparison establishes byte identity within its declared domain. [Canonicalization](../../crates/psyche-core/src/digest.rs) · [Port bindings](../../crates/psyche-coven/src/port.rs) · [Store transitions](../../crates/psyche-store/src/transitions.rs)

## Enum and transition inventory

The following external candidate vocabularies are experimental. Graph and node state membership alone supplies no admissible transition policy. Review each transition's authority and durable evidence before specifying a public state machine.

| Typed document vocabulary | Accepted values |
|---|---|
| GraphState | draft, admitted, rejected, running, waiting_approval, waiting_evidence, cancelling, completed, failed, cancelled, recovery_required |
| NodeState | proposed, admitted, rejected, blocked, ready, skipped, reserved, dispatching, adopted, adoption_unknown, proven_not_adopted, failed, running, waiting_approval, candidate, awaiting_verification, verified, escalation_required, cancelling, cancelled, termination_unknown, recovery_required |
| AdoptionState | not_submitted, submitting, adopted, proven_not_adopted, adoption_unknown, fenced |
| CancellationState | not_requested, termination_requested, acknowledged_terminated, acknowledged_already_terminal, termination_unknown |
| CancellationAcknowledgementKind | terminated, already_authoritatively_terminal |
| DeliveryRelationship | reply_same_dm, reply_same_group, reply_same_topic, cross_chat, broadcast |
| DeliveryDecisionState | reserved, consumed |
| DeliveryState | ready, sending, sent, retryable, delivery_unknown, failed, abandoned, dead_letter, resolving_unknown, compensated |

The experimental port advertises `stable_adoption`, `ambiguity_fence`, `ordered_events`, `authoritative_termination` and `content_addressed_results`. Execution input operations are launch and input. Rust adoption outcomes are Adopted, ProvenNotAdopted, Unknown. Reconciliation returns Returned, Fenced or Unresolved. Termination responds Acknowledged or Unresolved. Surface delivery returns Applied, Rejected or Unknown. Most port enums have no serialized representation. Publishing the Rust variant labels as wire strings would create a new contract. [Coven port](../../crates/psyche-coven/src/port.rs) · [Surface port](../../crates/psyche-surfaces/src/port.rs)

The experimental structured denial envelope lists 36 exact `ErrorCode` values with retryability, bounded message, correlation and string details. The array fixture enumerates each spelling and an accepted envelope. Decoder rejection families are TooLarge, UnknownSchema, UnsupportedMajor, UnknownEnumValue and InvalidShape. Production store resolution states are internal, including schema_now_supported, confirmed_invalid and duplicate_payload. The internal Rust ContractError, port/persistence errors, IngestOutcome, StoreError, quarantine resolution results and fake operation/fault enums remain implementation diagnostics. Fake Verified and ExpectedUnsupported have distinct meanings. [All 36 cases](../../crates/psyche-core/tests/fixtures/error-codes-v1.json) · [Error bounds](../../crates/psyche-core/src/contracts/error.rs) · [Rejection types](../../crates/psyche-core/src/contracts/mod.rs)

The internal append-only transition ledger checks ID/kind, contiguous per-record version, previous state, bounded next state, UTC timestamp and recomputed transition digest. Execution-binding revision replay is idempotent for identical bytes. Rebinding, changed previous digest, timestamp regression and revision forks reject. Quarantine ingest, resolve/replay, pruning exceptions and crash-reopen belong to the store. External profiles still need observable postconditions for all of these boundaries. [Store evidence manifest](../../scripts/g2-test-manifest.json)

## Reusable suites and Build canary gaps

Each row is an executable producer-side case in `cargo test --locked -p psyche-test-support --test conformance`. Its scripted fixture models the named authority. #13 needs portable inputs, exact expected bounded output and a check against the consuming adapter. The following pairs specify what to carry forward. [Suite entry points](../../crates/psyche-test-support/tests/conformance.rs) · [Suite definitions](../../crates/psyche-test-support/src/suites/coven.rs)

| Suite | Positive case | Rejection or recovery case for the released runner |
|---|---|---|
| C-S1 negotiation | Exact version and capabilities | Unsupported major, missing required capability or authority widening |
| C-S2 session | Same opaque session for stable correlation | Invalid session transition leaves durable state untouched |
| C-S3 snapshot | Matching principal, snapshot, graph, project and attempt | Stale or substituted correlation rejects without side effect |
| C-S4 adoption | Replay returns same durable disposition | Changed request under same identity rejects, lost response recovers |
| C-S5 proof | Durable exact non-adoption permits modeled next step | Unknown or adopted disposition cannot become non-adoption proof |
| C-S6 ambiguity | Durable return/fence matches exact correlation | Lost evidence or unresolved state blocks redispatch across restart |
| C-S7 cursor | Ordered pages resume from committed cursor | Gap, duplicate and drift reject without advancing |
| C-S8 terminal | Correlated durable terminal observation | Process/raw status and volatile terminal observation confer no terminal authority |
| C-S9 cancellation | Exact acknowledged or unresolved evidence persists | Raw status, changed binding or invalid lifetime rejects |
| C-S10 results | Ordered artifacts match session, request and lifetime | Mutated digest, media type, size, expiry or correlation rejects |
| C-S11 persistence | Reopen yields committed outcome | Fault at each boundary preserves uncertainty or previous committed state |
| C-S12 denial | Known code retains the bounded envelope | Unknown schema, major and enum route to quarantine |

**Missing downstream cases.** Exercise an incompatible downgrade against previously persisted data, disable the adapter and reopen the same task, bind approval/one effect/receipt through disconnect and restart, and reject stale or wider credentials after reconnect. The unknown-major unit test covers a single document. Neither that test nor the scripted restart proves old-client rollback, preserved task identity or product persistence. Check the single-task scenario against the merged Build compatibility map when the consumer owner activates #253. [Reference-client tracker](https://github.com/OpenCoven/psyche/issues/13) · [Build delivery](https://github.com/OpenCoven/psyche-build/issues/253)

## Remaining slices for #11

The order follows Phase 1 of the roadmap. Each slice adds protected paths, so each needs a candidate scope attestation and R4 review. [Phase 1](../ROADMAP.md) · [Publication requirements](https://github.com/OpenCoven/psyche/issues/11)

1. **Versioned machine-readable schemas.** One schema document per registry kind and one for the execution request, labelled experimental. Depends on this inventory and on a recorded choice between JSON Schema and a justified equivalent. A structural schema cannot state duplicate-key rejection (`crates/psyche-core/src/contracts/mod.rs:1103`), the safe-integer domain (`crates/psyche-core/src/digest.rs:60`) or recomputed digests. No generator exists and the manifest declares no generated path (`agent/manifest.yaml:43`), so this slice adds a generator or a drift test for hand-written files.
2. **Byte-exact canonical JSON and digest vectors.** One canonical file without a trailing newline and one pinned SHA-256 per kind, asserted the way the execution-request goldens are. Depends only on this inventory and the encoder (`crates/psyche-core/src/digest.rs:35`), so it can land before slice 1. It must author the first document for the ten kinds that have no file and turn the six measured digests above into pins.
3. **Positive, denial, ambiguity, restart, downgrade and unknown-version fixtures.** Depends on slice 2 for the bytes each rejection mutates and on a serialized expected-result format, because decode rejections and port dispositions are Rust enums with no published wire form (`crates/psyche-core/src/contracts/mod.rs:606`, `crates/psyche-coven/src/port.rs:525`). Unknown kind, major and enum already route to quarantine at ingest (`crates/psyche-store/src/records.rs:49`). The only downgrade refusal in main is the store rejecting a newer database version (`crates/psyche-store/src/lib.rs:65`); a document-level downgrade case is not established. Fence cases depend on the #12 issuer decision.
4. **Standalone conformance runner.** Depends on slices 1 to 3. The reusable suites are Rust functions over a Rust fixture (`crates/psyche-test-support/src/suites/coven.rs:1548`), so an adapter-facing process interface with bounded machine-readable output has to be defined. Whether the stable runner adopts the interface of either experimental profile is not established.
5. **Compatibility, migration, downgrade and deprecation policy.** Depends on #12, which owns version negotiation, release order and rollback. The policy must ratify or change current behavior: one supported major for every kind (`crates/psyche-core/src/contracts/mod.rs:500`), no minor version in the schema string, and unknown fields rejected (`crates/psyche-core/src/contracts/mod.rs:33`), so any added field is a breaking change today. It can be drafted beside slices 1 to 4 and approved only after #12.
6. **Immutable, checksummed artifact set.** Depends on slices 1 to 5, on #10 for the qualified clean-checkout interface, and on the approved #12 rows before any part is called stable. Compare two clean builds, verify the package in an empty consumer directory, run a deliberate drift and downgrade failure, and hand #13 the release, artifact digest, supported range, runner command and observed results. Signing and release publication are owner actions.

**Rows waiting on #12.** No row can be classified stable before #12 records the approved rows, which #11 lists as a dependency. These rows also have a named open decision. #12 names decisions and not rows, so the matching below is this inventory's reading of the fields each kind carries.

| Rows | Open decision in #12 |
|---|---|
| identity_snapshot, and the four kinds that pin a familiar snapshot ID: intent, graph_node, execution_binding, surface_effect | Retain, replace or supersede the historical `coven.familiar.identityRebind.v1` obligation. [Request](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7036420617) |
| recovery, execution_binding | Issuer and enforcing host of a process-attempt fence. The recovery record carries the fence token and the binding carries the `fenced` adoption state. [Request](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7036795474) · [#19](https://github.com/OpenCoven/psyche/issues/19) |
| approval, execution_binding, `psyche.execution_request.v1` | Which released record binds a consumer's task, action and terminal receipt to the request ID and digest, who issues and verifies it, and what a verifier reports when the evidence is missing or ambiguous. [Request](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7076215099) |
| surface_event, surface_effect, delivery, identity_snapshot, execution_binding | The split between Familiar Contract, Threads, Psyche and Coven is recorded for Telegram planning only. The general profile is undecided. [Bounded decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-6303089462) |
| budget, approval, evidence, verdict, recovery, addon, delivery | Their policy owners are deferred (`docs/SCHEMAS.md:66`). |
| all rows | Provenance of historical specification revision `9b07c500ad10a8d509d451d64b9739a1bdc4dc0f`. Which rows it constrains is not established. [Request](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7036420617) |

For graph and delegation no open decision beyond the general approval was found.
