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

The producer and consumer are reviewing the following obligations under [Psyche #19](https://github.com/OpenCoven/psyche/issues/19) and [Coven #804](https://github.com/OpenCoven/coven/issues/804). A runtime eligible for this profile needs a named issuer, an authenticated replay route and independent evidence for every resource that can still act. Authority selection remains open under [#12](https://github.com/OpenCoven/psyche/issues/12).

The proposed graph-owning host supplies the immutable reconciliation request. The consumer stores its source alongside the local invocation, attempt and process instance. Each lookup uses that original binding. An authenticated response must validate against the entire request. Checking the disposition shape alone cannot identify its issuer. Only the host can establish that the prior process, delegated descendants and already dispatched tools have all lost the ability to act.

The consumer records one complete validated `Fenced` disposition for the bound attempt in an atomic transition. Exact replay after reopening confirms that record. A different disposition for the same request conflicts and preserves the original. If the host is unavailable, returns `Unresolved` or cannot reproduce its disposition, the consumer keeps the retained history and blocks dispatch. Reconciliation never creates an attempt. A new attempt requires a separate recovery decision from the graph-owning authority.

`fencing_profile.rs` checks the existing Rust value validator and exercises a file-backed consumer journal with modeled host channels. Its subprocess exits on either side of commit. The parent reopens the database, requests replay and checks the one retained transition. `psyche-test-support` cases C-S6/C-S11 cover before-disposition faults, lost committed replies, exact replay and changed-request conflicts against retained in-memory fake state. C-S6 observes `EligibleAfterFence` without dispatch. Real host authentication, loss of effect capability for the old process and every child/tool, and replay across an actual host restart require producer and consumer qualification.
