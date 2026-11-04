# Contributing to Psyche

Psyche is a protocol and durable-state repository. Contributions are welcome, but compatibility and failure behavior matter as much as the happy path.

## Before starting

1. Read `README.md`, `AGENTS.md`, and `docs/ROADMAP.md`.
2. Find or open one owning issue with outcome, acceptance criteria, dependencies, non-goals, and evidence requirements.
3. For cross-repository work, read `docs/PROTOCOL-OWNERSHIP.md`.
4. Classify the change using the risk classes in `AGENTS.md`.

Do not begin an R3/R4 contract change from an unreviewed implementation assumption.

## Setup

```sh
git clone https://github.com/OpenCoven/psyche.git
cd psyche
./scripts/agent-bootstrap
./scripts/agent-check fast
```

The pinned Rust version is declared in `rust-toolchain.toml`. Use the locked Cargo dependency graph.

## Development loop

Keep each pull request focused on one contract or capability boundary.

```sh
./scripts/agent-check fast
# make a bounded change
./scripts/agent-check fast
# run the directly affected conformance/migration/fault tests
./scripts/agent-check full
```

Add behavior-level tests before changing canonical records, transitions, persistence, dispatch, or recovery.

## Pull request contract

A pull request should include:

- the owning issue;
- objective and non-goals;
- current behavior and invariant;
- risk class;
- public records, schemas, migrations, or consumers affected;
- exact commands and results;
- negative, crash, restart, ambiguity, and downgrade coverage where relevant;
- generated artifacts and generator command;
- migration and rollback;
- security/privacy impact;
- remaining proof gaps;
- exact final head SHA.

A merged source diff is not enough to close a runtime or compatibility outcome. Link retained evidence.

## Compatibility

Do not:

- change a stable record or ID without versioning;
- accept unknown versions/enums by guessing;
- silently widen capabilities;
- reuse process/UI/provider identifiers as protocol IDs;
- remove compatibility reads before migration evidence exists;
- convert ambiguous or indeterminate effects into success;
- modify golden bytes without an explicit compatibility decision.

Cross-repository changes must identify producers, consumers, release order, immutable canary pins, and rollback.

## Generated files

Use canonical generators. Never hand-edit generated schemas, fixtures, packed artifacts, or evidence summaries to make a check pass.

After generation:

```sh
git diff --check
git status --short
```

Review every generated change.

## Security and protected data

Follow `SECURITY.md`. Public issues, PRs, fixtures, and evidence must not contain secrets, credentials, private keys, raw prompts, unrestricted terminal output, private repository contents, full environment dumps, infrastructure details, or unnecessary personal paths.

## Review

R2–R4 changes require review from the owner of the affected surface. Resolve every current review finding and rerun required checks on the exact final head.

Keep changes reversible until compatibility and runtime evidence is complete.
