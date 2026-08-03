# Psyche configuration contract

The root config declares `schema_version = "psyche.config.v1"`. This build
accepts that exact value and denies every other, including future versions —
an unknown version is reported as an unsupported version, not as an unknown
field.

Unknown fields are errors. The only exception is the `extensions` table, whose
keys must themselves be versioned identifiers.

Raw secret values are never valid configuration. Secrets are named by reference
(for example `op://VAULT/ITEM/token`); a literal value is rejected at parse
time.

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
