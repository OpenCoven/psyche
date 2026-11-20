# Psyche

Psyche is OpenCoven's canonical orchestration protocol and reference implementation for durable, identity-preserving agent work.

It defines how tasks, graphs, lanes, execution attempts, delegations, deliveries, approvals, receipts, cancellation, recovery, and terminal outcomes are represented and verified without coupling those identities to a UI, tracker, process, transport, or persistence provider.

> **Status:** pre-1.0 protocol foundation. The retained G2 evidence covers the implementation at its recorded source and run. Consumer-ready protocol v1 artifacts and the first Psyche Build compatibility canary are tracked on the active roadmap. Do not infer support beyond retained evidence.

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

Read each crate’s Cargo manifest to identify its dependencies. For example, `psyche-config` depends on `psyche-core`. The architecture diagram’s arrow legend has an open correction in #21. [Config crate manifest](crates/psyche-config/Cargo.toml) · [Architecture](docs/ARCHITECTURE.md)

## Requirements

- Rust **1.89.0**, pinned by `rust-toolchain.toml`
- Cargo with the locked dependency graph
- Python 3.9+ for evidence verification
- Node.js 20+ and npm for the distribution wrapper checks
- Git
- Bash for both helpers (Git Bash on Windows), including its standard Unix tools
- jq for the separate `bash scripts/check-g2-workspace.sh` workspace check
- rustfmt and Clippy from the pinned toolchain. Put the rustup proxies first on PATH when another Rust installation is present.

The GitHub workflow remains the source of truth for the full Linux, macOS, Windows, supply-chain, secret, and npm matrix.

## Quick start

```sh
./scripts/agent-bootstrap
./scripts/agent-check fast
```

The first bootstrap fetches locked Cargo dependencies and requires network access. For a previously populated Cargo cache, run `PSYCHE_BOOTSTRAP_OFFLINE=1 ./scripts/agent-bootstrap`. Both verification modes accept staged, unstaged and untracked edits. They compare the index and working-tree contents at entry and exit and report changes made during verification. Run the complete local gate before requesting review.

```sh
./scripts/agent-check full
```

Direct Cargo entrypoints remain available:

```sh
cargo test --workspace --locked
cargo test --locked -p psyche-test-support --test conformance
cargo run --locked -p psyche-cli --bin psyche -- --help
```

The `psyche-cli` crate ships the operator binary `psyche` and the foreground daemon `psyched`. To run a credential-free operator check, create a disposable configuration and empty data directory outside the checkout. Python writes native paths on all three platforms.

```bash
scratch="$(python3 -c 'import tempfile; print(tempfile.mkdtemp(prefix="psyche-onboarding-"))')"
python3 - "$scratch" <<'PYCONFIG'
from pathlib import Path
import json
import sys

root = Path(sys.argv[1])
data = root / "data"
data.mkdir(mode=0o700)
(root / "psyche.toml").write_text(
    'schema_version = "psyche.config.v1"\n'
    + 'data_dir = ' + json.dumps(data.as_posix()) + '\n[coven]\n'
    + 'socket = ' + json.dumps((root / "coven.sock").as_posix()) + '\n'
    + 'required_api_version = "coven.daemon.v1"\n',
    encoding="utf-8",
)
PYCONFIG
cargo run --locked -p psyche-cli --bin psyche -- --config "$scratch/psyche.toml" doctor --json
cargo run --locked -p psyche-cli --bin psyche -- --config "$scratch/psyche.toml" status --json
cargo run --locked -p psyche-cli --bin psyche -- --config "$scratch/psyche.toml" stop
```

Expect `failed: 0` from doctor and `observed: false` with a null state from status. Stop exits 4 because this build has no daemon IPC. The disposable directory can be removed after inspection. [CLI](docs/CLI.md) · [Configuration](docs/CONFIGURATION.md) · [Testing](docs/TESTING.md)

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
