# Psyche configuration contract

The root config declares `schema_version = "psyche.config.v1"`. This build
accepts that exact value and denies every other, including future versions —
an unknown version is reported as an unsupported version, not as an unknown
field.

Unknown fields are errors. The only exception is the `extensions` table, whose
keys must themselves be versioned identifiers: at least one non-empty dotted
segment followed by a final `.v<digits>` — for example `psyche.experiment.v1`,
or `a.v0` at the minimum. This is enforced at load time, not merely
documented: a key that does not match is rejected with an error naming the key.

Loading stops after 1 MiB and reports the configuration as too large. The limit
is applied to the bytes actually read rather than to a stated file size, so it
also holds for a path that reports no size at all — a FIFO or a character
device such as `/dev/zero`.

Secrets are named by reference (for example `op://VAULT/ITEM/token`), never
written as literal values. The `SecretRef` type that enforces this lives in
`psyche-core` and rejects a literal at parse time — but no field in this release
is typed as one yet, so nothing enforces it today. Enforcement arrives with the
first secret-bearing field, in a later workstream.

## Minimal example

    schema_version = "psyche.config.v1"
    data_dir = "/var/lib/psyche"

    [coven]
    socket = "/run/coven.sock"
    required_api_version = "coven.daemon.v1"

## Fields

| Field | Type | Required | Meaning |
|---|---|---|---|
| `schema_version` | string | yes | Must be `psyche.config.v1`. |
| `data_dir` | path | yes | Directory owning local Psyche state. |
| `coven.socket` | path | yes | Coven daemon socket path. |
| `coven.required_api_version` | string | yes | Named daemon contract required before dependent actions. |
| `extensions` | table | no | Versioned escape hatch for forward-compatible additions. |

Account, principal-binding, and streaming tables are **not** part of this
release; they arrive with the surface workstreams.

## For consumers

A `Config` is obtainable only through `load_str` or `load_path`. The type does
not implement `Deserialize`, and the wire representation that does is private,
so there is no way to construct a `Config` that skipped the version check —
including by nesting it inside another `#[derive(Deserialize)]` struct.

`schema_version` is a method, not a field: a validated `Config` can only ever
hold the one accepted value, so storing it would add nothing and making it
writable would weaken the guarantee.

Extension values are read through `Extensions::get`, which deserialises one
table into a caller-owned type. `Extensions` deliberately does not expose the
underlying `toml::Table` — that keeps `toml` out of this crate's public API, and
its `Debug` redacts values, since a future extension may carry a secret.
