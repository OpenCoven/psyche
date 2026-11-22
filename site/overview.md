# Psyche

## What Psyche does

A familiar is a persistent AI identity in Coven. Psyche defines the records used to track what that familiar was asked to do, which execution belongs to the request, and what evidence came back. The Rust workspace implements those contracts alongside a durable store and a foreground daemon.

## Run a local check

From a Psyche checkout, save the following as `psyche.toml`. Use a fresh data directory for this example. On Unix, Psyche creates it with owner-only permissions.

```toml
schema_version = "psyche.config.v1"
data_dir = "./psyche-data"

[coven]
socket = "./coven.sock"
required_api_version = "coven.daemon.v1"
```

Run the operator CLI with the repository’s pinned Rust toolchain and lockfile.

```sh
cargo run --locked -p psyche-cli --bin psyche -- doctor --json
```

Look for `"failed": 0` in the report. The first run can report a warning when it creates the data directory. The socket check reports the configured path, so this example works before you start Coven. See the [command line reference](CLI.md#psychedoctorv1) for every check and exit code.

## Follow a request

Start with the [v1 schemas](SCHEMAS.md) to see how an intent, an execution binding, and a delivery are identified. The [architecture](ARCHITECTURE.md) shows which Rust crate validates each record and which crate stores it. The [testing guide](TESTING.md) explains how the workspace checks these contracts across replay, failure, and restart.

## Protocol roadmap

The public roadmap tracks publication of a standalone protocol bundle and adoption by Psyche Build as a reference client. Follow the [Psyche roadmap](https://github.com/OpenCoven/psyche/issues/9) for milestones and acceptance criteria.
