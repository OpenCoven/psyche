# G2 Schemas

## Registry and decoding

The closed v1 registry is `psyche.identity_snapshot.v1`, `psyche.intent.v1`,
`psyche.surface_event.v1`, `psyche.graph.v1`, `psyche.graph_node.v1`,
`psyche.delegation.v1`, `psyche.budget.v1`, `psyche.approval.v1`,
`psyche.execution_binding.v1`, `psyche.evidence.v1`, `psyche.verdict.v1`,
`psyche.recovery.v1`, `psyche.addon.v1`, `psyche.surface_effect.v1`,
`psyche.delivery.v1`, and `psyche.error.v1`.

Typed G2 records deny unknown fields. An unknown kind, unknown major version,
or unknown enum value is a strict decode failure and becomes a quarantinable
document, never a dispatchable record. `psyche.error.v1` exhaustively decodes
every `ErrorCode::ALL` value but is not persistable. Canonical JSON follows RFC
8785; every digest is SHA-256 over complete canonical typed content, and a
claimed digest is recomputed before authority or persistence accepts it.

## Stored records and identity

`RecordKind` has exactly fifteen identities. An execution binding is the one
`Attempt` record with the `att_` prefix: `SchemaKind::ExecutionBinding ->
RecordKind::Attempt -> att_`. There is no duplicate binding-named record kind.
Delivery is authoritative at `del_`; the related delegation identity is the
distinct derived `dlg_` prefix.

The canonical delivery v1 fields are `schema_version`, `delivery_id`,
`intent_id`, `action_class`, `account_id`, `chat_id`, `topic`, `relationship`,
`effect`, `effect_digest`, `surface_decision`, `logical_response_id`,
`logical_part`, `state`, `attempt_count`, and `telegram_message_id`. Surface
event/effect envelopes are core-owned, bounded, schema-versioned types;
adapters cannot add fields or widen payloads.

The store-owned `Transition` validates record identity, nonempty from/to state,
strictly increasing version, canonical UTC `created_at`, and its canonical
digest. Transition history is append-only.

## Quarantine and retention

`QuarantineId` is the owned strict `qua_` identity with one canonical uppercase
ULID suffix. A resolution records resolver, reason, canonical resolution
details and digest, and resolution time. Exact replay is idempotent and
concurrent resolution has one durable winner. Unresolved quarantine,
execution-binding revisions, transition-history rows, and audit-event rows are
excluded from automated retention. Content referenced by an unresolved or
live record remains retained.

## Cancellation and results

The G2 provisional `CancellationState` vocabulary is core-owned. A claimed O5
acknowledged state requires matching core-owned
`CancellationAcknowledgementEvidence`, including request, session, execution
request, digest, authority evidence, kind, and acknowledgement time. Raw Coven
ledger statuses (`created`, `running`, `idle`, `completed`, `failed`, `killed`,
and `orphaned`) never manufacture that evidence. Unresolved outcomes use the
separate core-owned unresolved evidence contract.

`ResultBundle` owns `session_id`, complete execution correlation, one primary
content reference, and ordered artifacts. Every result/artifact content
reference contains canonical `digest`, `media_type`, `size_bytes`, and
`expires_at`; every artifact repeats the bundle lifetime/correlation and
session. The retention owner is the durable store, not an adapter.

## Deferred owners

G2 deliberately defers routing policy, delivery retry policy, budget policy,
approval policy, evidence/verdict policy, recovery policy, addon policy,
surface-specific presentation, artifact blob transport, and automated
quarantine adjudication to their later named authority owners. The schemas
freeze interoperability; they do not silently assign those decisions to a
boundary adapter.

## Proposed process-attempt fencing consumer profile

**This section is a proposal. It is not a contract.** It is a draft for
producer review under
[Psyche #19](https://github.com/OpenCoven/psyche/issues/19) and
[#12](https://github.com/OpenCoven/psyche/issues/12), written for the
`coven-agents` consumer in
[Coven #804](https://github.com/OpenCoven/coven/issues/804). No producer has
approved it.
[#12 ADR 3](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)
decided that the graph-owning host issues a fence disposition for this profile,
and only on authenticated containment evidence from the owner of each resource
that can still act. Who enforces a fence in process is not decided, and neither
is the host binding. Until a profile is approved, a consumer keeps
every abandoned-running, termination-unknown and recovery-required invocation
non-dispatchable. Nothing here changes a G2 schema or permits a consumer to
act.

### What the existing validator checks

`ReconciliationDisposition::validate_for` checks a `Returned` or `Fenced` value
against one request. The disposition id and the session id or fence token must
be non-empty and at most 255 bytes. The correlation and the ambiguity digest
must equal the request's. The recorded time must be UTC and not earlier than
the correlation's creation time. `Unresolved` passes without any check.

It does not read the request's `reason_code`, does not call the request's own
`validate`, does not compare the recorded time with `valid_until`, and cannot
tell who issued the value. A passing result alone never permits recovery.

### Proposed consumer obligations

If a producer approves this proposal, a consumer would:

- Take the immutable reconciliation request from the graph-owning host and
  store it unchanged. It must not mint graph, node, attempt or familiar
  identities to satisfy the correlation shape.
- Use its local invocation, attempt and process-instance identifiers only to
  look up that request. A process identifier never keys a retained disposition.
  A request has one local binding, and a second binding is refused.
- Never mint, parse, order or infer a fence token. Never infer fencing from
  process death, a dropped future or an absent live-attempt entry.
- Validate the stored request and ask the bound host to replay the disposition
  for that exact request on every lookup, including after its journal is
  reopened.
- Retain one `Fenced` disposition per request, with its id, token, correlation,
  ambiguity digest and recorded time, in the same atomic step as the
  transition.
- Append to its history and never rewrite it.

The one transition the proposal would permit is from `abandoned-running`,
`termination-unknown` or `recovery-required` to `reconciled-fenced`, exactly
once per request. From any other state a valid `Fenced` value changes nothing
and is not retained. An exact replay of the retained value changes nothing. A
different value for the same request is a conflict: the original stays, and the
conflict is appended to the history.

The invocation stays blocked in every other case:

- the host is unavailable or cannot reproduce its disposition;
- the host returns `Unresolved` or `Returned`;
- the value fails the validator, including a stale one recorded before the
  correlation was created;
- the reply comes from a peer other than the bound one, or there is no binding;
- the stored request no longer decodes or validates;
- the outcome is anything the consumer does not recognise. Unknown stays
  blocked.

Reconciliation would never create an attempt or make one dispatchable. A
`Fenced` disposition resolves the ambiguity of its own attempt only. A new
dispatch needs its own fresh admission that names a new request and attempt for
the same work
([#12 ADR 3](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)).

### Not decided

- The in-process enforcer of a `Fenced` disposition. ADR 3 returned it to
  [#19](https://github.com/OpenCoven/psyche/issues/19) and
  [Coven #804](https://github.com/OpenCoven/coven/issues/804).
- How a consumer authenticates the issuing host.
- The issuer the suites exercise. C-S6 and C-S11 call `CovenPort::reconcile`,
  where the scripted Coven port issues the disposition. This proposal and
  ADR 3 have the graph-owning host issue it.
- The evidence that the old process, every delegated descendant and every
  already dispatched tool can no longer act. Only the host could establish it,
  and no fixture here does.
- Durable replay of a disposition after the issuing host restarts.

### Decided since this proposal

- The issuer of a `Fenced` disposition. [#12 ADR 3](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)
  decided that the graph-owning host issues it, for this profile only, on
  authenticated containment evidence from the owner of each resource that can
  still act. Coven is that owner for its sessions. The in-process enforcer is
  still open (see above).
- Redispatch after a fence. [#12 ADR 3](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)
  decided that a fence resolves only its own attempt and that a new dispatch
  needs its own admission. [PR #72](https://github.com/OpenCoven/psyche/pull/72)
  applied this to the conformance suite: a `Fenced` disposition now yields
  `RedispatchEligibility::AmbiguityResolvedRequiresAdmission` and
  `RedispatchDecision::AmbiguityResolvedRequiresAdmission`, and only a fresh
  admission yields `RedispatchDecision::FreshlyAdmitted`. `docs/TESTING.md`
  states the same rule. This matches the consumer obligation above.

### Evidence and its limits

`crates/psyche-coven/tests/fencing_profile.rs` calls the real Psyche API for
`AdoptionRequest::new`, `correlation()`, `ReconciliationRequest::validate`,
`validate_for` and the `ExecutionCorrelation` serde round-trip. Everything else
in that file is a consumer model written inside the test: the binding, the peer
check, replay, duplicate and conflict handling, the transition, the append-only
history, the dispatch spy and the crash handling. Its "wrong issuer" case
compares two strings and is not issuer authentication. No `coven-agents` code
runs.

In that model a subprocess exits on either side of commit. The parent reopens
the SQLite file, asks for replay and checks the one retained disposition and
the one appended transition. `psyche-test-support` cases C-S6 and C-S11 cover
faults before a disposition, a lost reply after a committed one, exact replay
after restart and changed-request conflicts, all against retained in-memory
fake state.
