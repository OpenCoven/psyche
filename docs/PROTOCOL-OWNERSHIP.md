# OpenCoven protocol ownership

**Status:** proposed baseline pending [OpenCoven/psyche#12](https://github.com/OpenCoven/psyche/issues/12)  
**Last reconciled:** 2026-08-26

This document prevents implementation order from becoming accidental canonical ownership.

## Principle

A durable identity or consequential transition has exactly one canonical owner. Products and adapters may cache, project, or reference protocol state, but must not create a second source of truth.

Transport location, process identity, filesystem paths, tmux panes, worktrees, branches, provider sessions, Beads, GitHub issues, and UI selections are implementation references—not protocol identity or authority.

## Proposed ownership matrix

| Domain | Canonical owner | Consumer relationship |
|---|---|---|
| Familiar identity and person binding | `OpenCoven/familiar-contract` | Psyche references stable familiar/person bindings |
| Protected-surface authorization decision | `OpenCoven/coven-threads` | Psyche carries and enforces the decision/constraints; it does not redefine policy |
| Orchestration task/graph/lane/attempt/delegation/delivery | `OpenCoven/psyche` | Products and runtimes implement versioned adapters |
| Lease, approval, receipt, cancellation, recovery, terminal outcome | `OpenCoven/psyche` | Adapters preserve exact correlation and fail-closed behavior |
| Daemon session, persistence, and runtime authority | `OpenCoven/coven` | Psyche references canonical daemon/runtime resources through typed boundaries |
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
- ambiguity reconciliation, idempotency, and fencing;
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

## Open decisions

The owning issue must approve:

- exact integration of Familiar and Threads identifiers/decisions;
- first stable Psyche consumer profile;
- Coven and runtime version-negotiation boundary;
- Cave/iOS approval and reconnect flow;
- SDK mutation profiles;
- release-order and emergency compatibility policy.

Until approved, this document is a proposed boundary and must not be cited as shipped protocol support.
