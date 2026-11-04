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

## Process-attempt fencing consumer profile (`coven-agents`)

This profile qualifies the existing `ReconciliationDisposition::Fenced`
contract for a consumer reconciling an abandoned process-bound invocation
attempt (consumer: OpenCoven/coven#804). It adds no token shape, retry
authority, or graph ownership.

**Issuer and enforcer.** Only the Psyche host that owns the execution graph
issues a `Fenced` disposition, through `CovenPort` reconciliation. It issues
one only after it has itself fenced every resource capable of acting for the
old attempt: the process, delegated descendants, and already dispatched tools.
A consumer, adapter, or process-local registry is never an issuer. The fence
token is opaque; a consumer must not mint, parse, or infer one.

**Correlation binding.** The Psyche host supplies the immutable
`ExecutionCorrelation` (request/digest, familiar snapshot, project, graph,
node, attempt, validity). The consumer stores the host-supplied correlation
next to its local invocation and process-attempt identifiers and compares it
exactly. It must not create graph, node, attempt, or familiar identities. An
invocation with no host-supplied correlation cannot be reconciled and stays
non-dispatchable.

**Evidence.** `validate_for` proves only bounds, exact correlation, exact
ambiguity digest, and a recording time not earlier than correlation creation.
It does not prove containment. Authority comes from the issuing host's durable
disposition record. The consumer persists the validated disposition (id, token,
correlation, digest, recorded time) before acting, and on restart re-requests
replay of the same request and re-validates it. If the host cannot replay it,
the state is `Unresolved`.

**Permitted transition.** A validated, durably recorded `Fenced` disposition
permits exactly one transition: the abandoned attempt moves from unresolved to
reconciled-fenced. It does not permit a new attempt. A new attempt requires a
separate retry/recovery decision from the Psyche authority that owns the
graph (recovery policy remains deferred above). Duplicate exact dispositions
are idempotent. A different disposition for the same request is an
`IntentConflict`. A disposition whose correlation, digest, or issuer differs,
or that predates the correlation, is rejected and the state is unchanged.

**Stays blocked.** `Unresolved`, a lost disposition write, a surviving
child or tool, process death, a dropped future, an empty live-attempt
registry, and string validation alone never authorize recovery.

**Fixtures.** `crates/psyche-coven/tests/fencing_profile.rs` covers exact,
wrong-correlation, wrong-digest, stale, malformed-token, and unresolved cases.
`psyche-test-support`'s Coven conformance suite covers lost disposition write,
duplicate reconciliation, and replay after reopen against the fake port.
Surviving child/tool and issuer identity are host-side obligations that the
consumer cannot verify from the value alone; they remain unresolved until a
host publishes enforcement evidence.
