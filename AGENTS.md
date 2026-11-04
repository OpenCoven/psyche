# Psyche agent guide

This is the repository entrypoint for coding agents and human contributors. Keep it short and route detailed work to the canonical documents below.

## Mission

Psyche is the canonical orchestration protocol for OpenCoven. Preserve identity, authority, correlation, durability, and recovery across runtimes and surfaces.

Psyche Build is the first planned reference client; it is not the protocol owner. Coven, Threads, and Familiar Contract retain their own canonical domains.

## Start here

1. Read `README.md`.
2. Read `docs/ROADMAP.md` and confirm the work has an owning issue.
3. Read `docs/PROTOCOL-OWNERSHIP.md` before changing a cross-repository boundary.
4. Read the scoped architecture/testing documents relevant to the change.
5. Run `./scripts/agent-bootstrap`.
6. Run `./scripts/agent-check fast` before editing and after each bounded slice.
7. Run `./scripts/agent-check full` before requesting review.

## Canonical documents

- Architecture and dependency direction: `docs/ARCHITECTURE.md`
- Protocol/schema inventory: `docs/SCHEMAS.md`
- Deterministic evidence model: `docs/TESTING.md`
- Current G2 evidence: `docs/G2-EVIDENCE.md`
- CLI boundary: `docs/CLI.md`
- Configuration: `docs/CONFIGURATION.md`
- Program roadmap: `docs/ROADMAP.md`
- Cross-repository ownership: `docs/PROTOCOL-OWNERSHIP.md`
- Contribution and review contract: `CONTRIBUTING.md`
- Security reporting and protected data: `SECURITY.md`
- Machine-readable repository contract: `agent/manifest.yaml`

## Ownership by path

| Path | Canonical responsibility |
|---|---|
| `crates/psyche-core/**` | IDs, records, validation, canonicalization, digests, errors |
| `crates/psyche-store/**` | durable storage, transitions, migrations, quarantine, retention |
| `crates/psyche-coven/**` | typed Coven adapter; never replaces core decisions |
| `crates/psyche-surfaces/**` | bounded surface ports |
| `crates/psyche-runtime/**` | runtime composition and shutdown |
| `crates/psyche-cli/**` | process/CLI boundary |
| `crates/psyche-test-support/**` | reusable conformance evidence, not production authority |
| `packages/psyche-npm/**` | npm wrapper and packed distribution |
| `.github/**`, `scripts/**` | CI, evidence, release, and verification controls |

## Risk classes

- **R0:** prose and examples with no contract claim.
- **R1:** pure implementation that cannot mutate durable state or authority.
- **R2:** local mutable state, migrations, or recovery helpers.
- **R3:** network, credentials, external systems, or user data.
- **R4:** canonical identity, authorization, persistence, dispatch, receipts, cancellation, recovery, evidence, and release.

Psyche is an R4 repository. Any R2–R4 change needs explicit failure, rollback, and retained-evidence criteria. Do not widen authority to simplify a test.

## Invariants

Never:

- derive durable identity from a tracker, process, tmux pane, path, transport, UI component, or provider session;
- treat successful decoding or dispatch as authorized completion;
- replace an unknown or ambiguous result with success;
- redispatch an indeterminate effect without the canonical reconciliation/fence rule;
- mutate append-only records or rewrite history to “repair” state;
- accept unknown major versions, enums, kinds, or widened capabilities silently;
- hand-edit generated fixtures or evidence where a canonical generator exists;
- weaken negative, crash, restart, quarantine, or structured-denial tests;
- expose secrets or unrestricted user/project content in evidence.

## Change workflow

1. Claim one issue and one bounded outcome.
2. Record current behavior and the invariant being preserved.
3. Add or identify focused positive and negative tests.
4. Implement the smallest compatible slice.
5. Run the fast gate.
6. Run the relevant conformance, migration, property, or crash tests.
7. Run the full gate.
8. Inspect `git diff --check` and ensure generated outputs are canonical.
9. Open a focused PR with exact commands/results, risk, migration, rollback, affected consumers, and remaining proof gaps.
10. Do not close the outcome until durable evidence is linked.

## Protected surfaces

Treat these as requiring focused owner review:

- canonical ID prefixes and record schemas;
- canonical JSON and digest construction;
- structured error/denial vocabulary;
- migrations and persisted compatibility;
- adoption, dispatch, fencing, cancellation, receipts, and recovery;
- quarantine and retention;
- runtime shutdown/checkpoint ordering;
- conformance fixtures and evidence checker allowlists;
- CI, supply-chain, secret, and release controls.

## Generated and evidence files

Do not edit generated artifacts manually. Use the documented generator or update the generator and artifact in the same PR.

`docs/G2-EVIDENCE.md` is retained evidence tied to immutable commits and runs. It is not a mutable status dashboard. New evidence must identify source SHA, command, result, and immutable artifact/run.

## Verification

```sh
./scripts/agent-check fast
./scripts/agent-check full
```

The full local script intentionally does not replace GitHub's cross-platform, cargo-deny, or full-history secret jobs. A local pass is necessary, not sufficient.

## Cross-repository changes

Before changing a public contract:

- update the producer/consumer ownership map;
- identify every affected OpenCoven repository;
- define version negotiation and rollback;
- update immutable compatibility pins/canaries;
- preserve old reads until migration evidence permits removal;
- do not make Psyche Build, Cave, Coven, or Threads a hidden source of canonical truth.

## Completion evidence

A completion comment or PR must include:

- objective and non-goals;
- files intentionally changed;
- risk class and protected surfaces touched;
- exact tests and results;
- compatibility and consumer impact;
- migration and rollback;
- generated artifact provenance;
- unresolved uncertainty or unavailable platform evidence;
- exact commit SHA.
