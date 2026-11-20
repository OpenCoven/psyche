# G2 consumer publication inventory

Survey of Psyche revision `1e47b40dbbb9db21962b4884afe49763321f04b0` for #11. The current decoder knows sixteen v1 document kinds and fifteen durable record prefixes. All sixteen rows are **experimental** for external publication. The approved stable and deprecated sets are empty. Storage, harness and diagnostic internals are identified below. [Registry](../../crates/psyche-core/src/contracts/mod.rs) · [Schema notes](../SCHEMAS.md) · [Ownership decisions](https://github.com/OpenCoven/psyche/issues/12)

## Records and prospective authority

Each row spells a schema as `psyche.<kind>.v1`. All rows share strict fields and enum values, a canonical uppercase ULID for each Psyche record ID, and positive and rejection cases required for promotion. “Inline” indicates source tests or typed construction. A standalone record vector is missing wherever there is no named file. Producer names describe the orchestration record and the distinct authority it references. Publication also needs the approved ownership revision and consumer-facing rejection vectors for each row.

| Kind | Prefix | Proposed producer and authority dependency | Existing input or required independent case |
|---|---|---|---|
| identity_snapshot | ids_ | Psyche snapshot of Familiar Contract identity/person binding | Inline timestamp/provenance case. Need mismatched binding and historical rebind disposition. |
| intent | int_ | Psyche after principal resolution | intent-local.json. Unknown field rejection in contracts.rs. |
| surface_event | sev_ | Psyche envelope, surface adapter for observed event | surface-event.json. Reject malformed actor, locator or oversized content. |
| graph | grf_ | Psyche graph admission and lifecycle | Inline graph. Reject impossible binding shape and unknown state. |
| graph_node | nod_ | Psyche orchestration | node-root.json. Reject unknown state and wrong related ID. |
| delegation | dlg_ | Psyche scope projection with applicable upstream authority | Inline ID/prefix. Need narrowed scope and widening vectors. |
| budget | bud_ | Psyche accounting record, resource enforcement belongs to its provider | Inline numeric boundaries. Need enforceable limit and widened reservation cases. |
| approval | apr_ | Psyche lifecycle referencing the named decision authority | Inline nullable decision and expiry. Need consume-once and stale authority cases. |
| execution_binding | att_ | Psyche Attempt record, Coven admission/session/adoption/termination evidence | Inline immutable/revised bindings. Need portable stale correlation, ambiguity, crash and fencing cases. |
| evidence | evd_ | Psyche correlation, originating authority supplies evidence | Inline timestamp and size. Need content/correlation mismatch pair. |
| verdict | vrd_ | Psyche record referencing reviewer and policy revision | Inline creation time. Need sealed-evidence mismatch and authority cases. |
| recovery | rcv_ | Psyche recovery record, fence issuer and enforcement awaiting #12/#19 | Inline numeric boundary. Need unresolved/reconciled paths with surviving resource coverage. |
| addon | adn_ | Psyche registration, policy for provenance/allowlist/revocation deferred | Typed producer. Need canonical positive and revoked or altered contribution pair. |
| surface_effect | sfx_ | Psyche effect correlation, Threads protected-surface authorization | surface-effect.json. Reject wrong ID, altered digest and malformed payload. |
| delivery | del_ | Psyche delivery ledger, adapter observes external outcome | delivery-ready.json. Reject invalid state, sent without message ID and wrong prefix. |
| error | — | Psyche structured envelope, causal authority supplies fact | error-storage-unavailable.json and error-codes-v1.json. Unknown code, shape and persistence attempts reject. |

The request prefix `req_` is ephemeral. The store owns `qua_` for quarantine. Both use the same canonical ULID suffix grammar. Coven session, artifact and disposition IDs and opaque fence tokens retain producer-specific identity. A released profile must specify how downstream task, lane, action, receipt and lease IDs relate to the registry. Main defines no separate record prefix for any of those five names. [ID parsers](../../crates/psyche-core/src/id.rs) · [Quarantine](../../crates/psyche-store/src/quarantine.rs) · [Build mapping](https://github.com/OpenCoven/psyche-build/blob/73dc6f5d733745894385f906c2f0993f1fd1f818/docs/PSYCHE-COMPATIBILITY-MAP.md)

## Canonical bytes and digests

These observations use each source document after `decode_document`, then `canonical_bytes` and `digest`. A second decode and canonicalization reproduced identical bytes. The six pretty-printed input files differ from their canonical representations. The separate error-code fixture contains an array of 36 individual envelopes. Consumer pins must identify the approved package and its individual files.

| Producer input under crates/psyche-core/tests/fixtures | Canonical bytes | SHA-256 of canonical typed document |
|---|---:|---|
| intent-local.json | 479 | b6c6e3cdec0628b2a588abac4cb567a697031fd955fc542f3f8a1f974dcc5bfb |
| node-root.json | 340 | aab8c7902cfdeb07a55c4a0823e1cac2c8a94f71dce7dfd01ae3332ce4499036 |
| delivery-ready.json | 975 | c4f7b567b2abbc9ec94f50a13ee42f5b8eba276033ace75c218ad1f472bbdbf4 |
| error-storage-unavailable.json | 227 | 6d58339a99ecde35000d22d49b5d4f8085be83188b87d09efd5368b43b469377 |
| surface-event.json | 431 | bab220ec9dd03e9ddfc7187cfe028bcf2980164bb91fc2393d09917c905c0488 |
| surface-effect.json | 666 | e98c8ee8f28a6d2c3757bd7f2f030c6cbf87cba1eaa1f7797052dc8579d4d507 |

Two experimental Coven port vectors are already byte-exact canonical JSON without a trailing newline. The launch request hashes to `75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04`, and the input request to `c8c3d0cad99f65d0fdac7b2bb577cf1278412a7ea6255d443e45394109311c61`. Both bind operation-specific fields, ordered artifact requirements and RFC3339 timestamps. `result-bundle.json` supplies the separate result-binding input. [Golden assertions](../../crates/psyche-coven/tests/request_digest.rs) · [Result assertions](../../crates/psyche-coven/tests/bindings.rs)

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

**Publication gate.** #10 supplies a qualified clean-checkout interface. #12 selects the approved rows, version rules and authority owners, including fence issuance and historical identity rebind. Then generate schemas, canonical bytes and independent denial/restart vectors from the approved revision. Compare two clean builds, verify the signed/checksummed immutable package in an empty consumer directory, run a deliberate drift and downgrade failure, and hand Build the release, artifact digest, supported range, runner command and observed results. [Publication requirements](https://github.com/OpenCoven/psyche/issues/11)
