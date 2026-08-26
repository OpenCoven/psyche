# Psyche

Psyche is OpenCoven's canonical orchestration protocol and reference implementation for durable, identity-preserving agent work.

It defines how tasks, graphs, lanes, execution attempts, delegations, deliveries, approvals, receipts, cancellation, recovery, and terminal outcomes are represented and verified without coupling those identities to a UI, tracker, process, transport, or persistence provider.

> **Status:** pre-1.0 protocol foundation. The current repository has passed the documented G2 contract-foundation evidence. Consumer-ready protocol v1 artifacts and the first Psyche Build compatibility canary are tracked on the active roadmap. Do not infer support beyond retained evidence.

## Canonical ownership

Psyche owns:

- orchestration identity and correlation;
- task, graph, lane, attempt, delegation, and delivery contracts;
- canonical serialization, digests, structured denials, and compatibility behavior;
- capability/lease, approval, receipt, cancellation, recovery, and terminal-state semantics;
- adapter conformance requirements.

Psyche does not own:

- familiar or person-binding identity (`OpenCoven/familiar-contract`);
- protected-surface authorization decisions (`OpenCoven/coven-threads`);
- daemon session, persistence, or runtime authority (`OpenCoven/coven`);
- product UI, tmux panes, worktrees, branches, provider sessions, or tracker state;
- the Psyche Build product (`OpenCoven/psyche-build`).

See [Protocol ownership](docs/PROTOCOL-OWNERSHIP.md) for the proposed cross-repository boundary and [the roadmap](docs/ROADMAP.md) for graduation gates.

## Repository map

| Path | Responsibility |
|---|---|
| `crates/psyche-core` | Canonical contracts, IDs, validation, digests, schemas, and errors |
| `crates/psyche-store` | Durable records, append-only transitions, migrations, quarantine, and retention |
| `crates/psyche-coven` | Typed Coven adapter boundary |
| `crates/psyche-surfaces` | Bounded surface ports |
| `crates/psyche-runtime` | Runtime composition and lifecycle |
| `crates/psyche-cli` | CLI and daemon process boundary |
| `crates/psyche-config` | Configuration loading and validation |
| `crates/psyche-test-support` | Reusable adapter conformance, property, fault, and restart evidence |
| `packages/psyche-npm` | Dependency-free npm distribution wrapper |
| `docs` | Architecture, schemas, testing, evidence, ownership, and roadmap |
| `scripts` | Evidence and repository verification tools |

The dependency direction and crate ownership are documented in [Architecture](docs/ARCHITECTURE.md).

## Requirements

- Rust **1.88.0**, pinned by `rust-toolchain.toml`
- Cargo with the locked dependency graph
- Python 3 for evidence verification
- Node.js 20+ and npm for the distribution wrapper checks
- Git
- A POSIX shell for the repository helper scripts

The GitHub workflow remains the source of truth for the full Linux, macOS, Windows, supply-chain, secret, and npm matrix.

## Quick start

```sh
./scripts/agent-bootstrap
./scripts/agent-check fast
```

Run the complete local gate before requesting review:

```sh
./scripts/agent-check full
```

Direct Cargo entrypoints remain available:

```sh
cargo test --workspace --locked
cargo test -p psyche-test-support --test conformance
cargo run -p psyche-cli -- --help
```

See [CLI](docs/CLI.md), [Configuration](docs/CONFIGURATION.md), and [Testing](docs/TESTING.md).

## Change policy

Psyche is an R4 repository: small mistakes can fork identity, widen authority, lose durable evidence, or make an ambiguous effect appear complete.

Changes to canonical IDs, record shapes, digests, migrations, dispatch, persistence, cancellation, recovery, compatibility, or release evidence require:

1. an owning roadmap issue;
2. explicit positive and negative acceptance criteria;
3. migration and rollback analysis;
4. focused contract tests before implementation;
5. exact-head CI evidence;
6. affected-consumer and compatibility-canary review.

Read [AGENTS.md](AGENTS.md) before making changes and [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## Roadmap

The active 90-day program is tracked in:

- [Roadmap document](docs/ROADMAP.md)
- [Roadmap control issue](https://github.com/OpenCoven/psyche/issues/9)
- [Repository and agent-readiness foundation](https://github.com/OpenCoven/psyche/issues/10)
- [Protocol v1 publication](https://github.com/OpenCoven/psyche/issues/11)
- [Cross-repository ownership](https://github.com/OpenCoven/psyche/issues/12)
- [Psyche Build reference-client canary](https://github.com/OpenCoven/psyche/issues/13)
- [Security, release, support, and governance](https://github.com/OpenCoven/psyche/issues/14)

## Security

Do not post credentials, private keys, raw prompts, unrestricted terminal output, private repository contents, environment dumps, or sensitive paths in public issues or retained evidence. Follow [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
