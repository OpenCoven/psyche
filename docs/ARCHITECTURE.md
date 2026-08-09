# Architecture

Psyche keeps contracts and authority in Rust. Dependency arrows point from a
dependency to its consumer:

```text
psyche-core <- psyche-config
psyche-core <- psyche-store
psyche-core <- psyche-coven
psyche-core <- psyche-surfaces
psyche-core + psyche-coven + psyche-surfaces + psyche-store <- psyche-test-support
psyche-config + psyche-store <- psyche-runtime <- psyche-cli
psyche-config <- psyche-cli
psyche-store <- psyche-cli
```

`psyche-core` owns canonical contracts, validation, IDs, digests, and error
vocabulary. `psyche-store` owns durable records, transitions, quarantine, and
retention. `psyche-coven` owns the typed Coven boundary but delegates canonical
contract decisions to core. `psyche-surfaces` owns bounded surface ports.
`psyche-test-support` depends on the complete boundary so its reusable suites
can test adapters without becoming production authority. Runtime composes
configuration and storage. Runtime owns opening the store during startup. The
CLI is its outer process boundary and reads configuration.
The CLI uses its direct store dependency only for doctor data-directory preparation.
