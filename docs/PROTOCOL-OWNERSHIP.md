# OpenCoven protocol ownership

The accountable owner approved the assignments and compatibility policy below for the first Build profile. Broader domain entries remain planning guidance. [Owner decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)

Last reconciled on 24 November 2026.

This document prevents implementation order from becoming accidental canonical ownership.

## Principle

A durable identity or consequential transition has exactly one canonical owner. Products and adapters may cache, project, or reference protocol state, but must not create a second source of truth.

Transport location, process identity, filesystem paths, tmux panes, worktrees, branches, provider sessions, Beads, GitHub issues, and UI selections are implementation references—not protocol identity or authority.

## Recorded Telegram boundary

The separate Rust Telegram adapter belongs in Psyche. Familiar Contract owns familiar/person identity, Threads owns protected-surface authorization, Psyche composes local admission and orchestration, and Coven owns execution, adoption and fencing. The recorded scope permits interface and denial-vector planning. Actual integration depends on G4/Coven #1230, production child dispatch on G6, and cutover on applicable G10. The first-profile decisions below govern rebind migration and historical spec provenance. [Decision and gates](https://github.com/OpenCoven/psyche/issues/12#issuecomment-6303089462) · [Product placement](https://github.com/OpenCoven/psyche/issues/12#issuecomment-6303108240)

## Ownership by domain

| Domain | Canonical owner | Consumer relationship |
|---|---|---|
| Familiar identity and person binding | `OpenCoven/familiar-contract` | Psyche references stable familiar/person bindings |
| Protected-surface authorization decision | `OpenCoven/coven-threads` | Psyche carries and enforces the decision/constraints; it does not redefine policy |
| Orchestration task/graph/lane/attempt/delegation/delivery | `OpenCoven/psyche` | Products and runtimes implement versioned adapters |
| Orchestration lease, approval, receipt correlation, cancellation and recovery vocabulary | `OpenCoven/psyche` | Build retains native receipt bytes. Psyche issues the request-to-receipt binding. |
| Daemon sessions, execution admission, adoption, fencing and native terminal state | `OpenCoven/coven` | Psyche carries correlated orchestration evidence through typed boundaries. The #19 graph host issues a disposition using containment evidence from each acting resource owner. |
| Runtime capability descriptors | `OpenCoven/coven-runtimes` | Psyche negotiates a pinned supported capability profile |
| Coding cockpit and local product behavior | `OpenCoven/psyche-build` | First Psyche reference client; panes/worktrees remain product-local |
| Human oversight and approval UI | `OpenCoven/coven-cave` | Renders/requests protocol state; does not infer completion |
| Public constrained client API | `OpenCoven/sdk` | Consumes an approved read/mutation profile |
| Read-only memory access | `OpenCoven/coven-memory` | Cannot become memory mutation authority |
| Documentation and compatibility ledger | `OpenCoven/coven-docs` | Generated from canonical artifacts; not a contract owner |

## Psyche ownership

Psyche canonically owns:

- orchestration record kinds and identifier namespaces;
- task graph and execution correlation;
- lane, attempt, delegation, and delivery semantics;
- capability/lease lifecycle;
- approval lifecycle;
- action and receipt lifecycle;
- cancellation acknowledgement and unresolved outcomes;
- orchestration rules for ambiguity reconciliation, idempotency and consuming fence evidence;
- artifact/evidence correlation;
- recovery and terminal-state vocabulary;
- canonical serialization, digest, version, and structured-denial behavior;
- adapter conformance profiles.

Psyche does not own product layout, terminal rendering, worktree mechanics, provider launch details, daemon storage internals, authorization policy, or familiar identity.

## Dependency direction

The intended release dependency is acyclic:

```text
familiar-contract ─┐
coven-threads ─────┼─> psyche protocol profile
coven-runtimes ────┤          │
coven ─────────────┘          ├─> psyche-build reference adapter
                              ├─> coven-cave oversight adapter
                              ├─> sdk constrained client
                              └─> coven-memory read-only client
```

A consumer may depend on a released Psyche profile. Psyche must not depend on a product repository's implementation to define the profile.

## Psyche Build mapping rule

Psyche Build may retain:

- pane IDs;
- tmux session/pane IDs;
- worktree paths;
- branches and commits;
- provider session IDs;
- UI selection/focus state;
- Beads and GitHub planning references.

Each may reference a Psyche ID. None may replace the Psyche ID, prove authority, or prove completion.

Migration must introduce protocol IDs alongside local IDs, maintain reversible compatibility reads, and remove duplicate local semantics only after pinned canaries prove equivalence.

## Contract-change policy

A stable contract change must include:

1. canonical owner and affected record/transition;
2. old/new version behavior;
3. positive and denial vectors;
4. unknown-major/enum behavior;
5. migration, downgrade, and rollback;
6. producer and consumer inventory;
7. immutable artifact/profile pin;
8. exact-head downstream canary results;
9. release order;
10. removal/deprecation window.

Do not merge a change that creates circular release ownership or requires a consumer's unreleased implementation to define the producer contract.

## Authority and evidence

- A decoded request is not necessarily authorized.
- A dispatched request is not necessarily complete.
- UI/process activity is not a terminal state.
- An ambiguous effect remains unknown or recovery-required until canonical reconciliation proves otherwise.
- Retained evidence identifies source/artifact digest, exact command/action, correlation IDs, result, and unavailable proof gaps.
- Evidence must be bounded and exclude protected user/project content.

## First Build profile decisions

The accountable owner decided the following against revision 3 of the ownership packet. These decisions govern the first Build profile. Stable publication, runtime certification and live client activation each need separate evidence. [Owner decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214) · [Packet and row definitions](https://github.com/OpenCoven/psyche/pull/29)

| Decision | Boundary | Remaining evidence and owner |
|---|---|---|
| Replace legacy `coven.familiar.identityRebind.v1` | Preserve Familiar/person authority. Map each historical guarantee to its producer, current contract/version, executable canary and denial. | #12 tracks prior-session survival, unresolved adoption and restart. An identity change keeps an unproven route blocked. |
| Correct the historical spec reference | Spec pin `9b07c500` is unrecoverable. The preserved Coven #1230 acceptance text and 20 October matrix retain the obligations. | Historical obligations remain open under [Coven #1230](https://github.com/OpenCoven/coven/issues/1230). The owner assigned no replacement SHA. |
| Fence a process-bound attempt | The graph-owning host issues the #19 disposition after authenticated containment evidence from every resource owner that can still act. Coven supplies evidence for its sessions. A fence resolves only the old attempt. | [#19](https://github.com/OpenCoven/psyche/issues/19) and [Coven #804](https://github.com/OpenCoven/coven/issues/804) own the in-process enforcer and surviving-resource/restart proof. Indeterminate attempts stay blocked. Fresh dispatch requires its own admission and the G2 redispatch test needs a separately reviewed change. |
| Bind a Build receipt to a Psyche request | Psyche issues a versioned binding from request ID and canonical request digest to the exact native receipt bytes’ digest. Build keeps the bytes and supplies opaque task/action or idempotency references. The pinned verifier checks issuer provenance and both digests. | [#11](https://github.com/OpenCoven/psyche/issues/11) owns wire format, distribution and verification. [#13](https://github.com/OpenCoven/psyche/issues/13) owns the native canary. Missing means unknown/recovery, ambiguity stays unknown until reconciliation, and conflicts preserve the original and quarantine the conflicting evidence. None permits dispatch. Effect counts require independent evidence. |

Producers are approved for rows 1, 2, 4–9, 15 and 16. Row 4 excludes rebind. Row 9 gives Coven authority over sessions, execution admission, adoption, fencing and native terminal state. Rows 5–7 need released representations. Rows 11 and 13 return for owner approval with fence and receipt-binding evidence respectively. Rows 10, 12, 14 and 17–21 are deferred. The policy requires an exact immutable pin and rollback. Emergency and deprecation procedures remain proposed until exercised. [Decision and row dispositions](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)

The merged experimental lifecycle producer and Build's pinned native canary provide evidence for their exercised fixture paths. The released receipt-binding verifier, stable consumer profile and live Build activation remain open. The roadmap lists the exact producer and consumer pins, owners, prerequisites and acceptance gates. [Roadmap](ROADMAP.md)
