# Experimental first Build profile prerelease

This directory produces the offline kit that a downstream repository, first Psyche Build, pins to check the first Build profile without a Psyche checkout. It packages the selection in [the publication surface](../../docs/protocol/PUBLICATION-SURFACE.md), the [canonical vectors](../../crates/psyche-core/tests/fixtures/vectors/README.md), the [ADR 4 receipt binding verifier](../build-receipt-binding/README.md) and the [receipt replay restart witness](../receipt-replay/README.md). The format is `psyche.experimental.first-build-profile.v1`. It is a prerelease candidate for [#11](https://github.com/OpenCoven/psyche/issues/11). It is not stable, not signed and not published as a release. [Consumer guide](CONSUMER.md)

## Kit contents

| Member | Source | What a consumer checks |
|---|---|---|
| `manifest.json` | built by `kit.py` | Format and major, producer commit, target, toolchain, `Cargo.lock` digest, each member's SHA-256, each kind's proposed class and vector digest, the two excluded kinds and the component pins. |
| `vectors/psyche.<kind>.v1.json` ×14 | `crates/psyche-core/tests/fixtures/vectors/` | Canonical RFC 8785 bytes and SHA-256 of the 3 proposed stable targets and 11 experimental kinds. `surface_effect` and `delivery` are excluded while row 13 is held. |
| `denials.json` | `make_denials.py` (generated) | 52 rejection and 28 canonicalization cases with portable expected results. |
| `bin/verify-profile` | `crates/psyche-test-support/src/bin/verify-profile.rs` | The offline conformance runner. |
| `bin/verify-build-binding`, `binding/**` | ADR 4 verifier and golden inputs | Issuer signature, request digest and receipt digest, with four dispositions. |
| `receipt-replay/**` | pinned `receipt-replay.pyz` and its consumer fixture | Restart across a durable store, with one effect and zero additional executions. |
| `README.md`, `LICENSE` | `CONSUMER.md`, root license | |

## Produce and qualify

From a clean checkout at the commit to package, on the native host for the target:

```sh
python3 profiles/first-build-profile/kit.py build --source "$(git rev-parse HEAD)" --target <triple> --archive /abs/path/first-build-profile-<triple>.tar.gz --require-restart
```

The build requires the pinned Rust toolchain. It drops every `CARGO_*`, `RUSTC*`, `RUSTFLAGS*` and `RUSTDOC*` environment override and refuses any ambient `.cargo/config` file. It remaps build paths so the binaries name no host directory, and refuses a dirty checkout or stale `denials.json`. It rebuilds `receipt-replay.pyz` and checks it against its committed pin. It then runs `smoke`. Smoke authenticates the archive digest before reading a member and extracts it into a fresh directory. It runs the profile runner, four fail-closed controls (a tampered vector, an older major, a newer major, a different producer commit), the four binding dispositions and the network-isolated restart witness, then re-verifies the extracted kit. Windows has no supported network isolation for the restart witness, so its job omits `--require-restart` and logs the skip. The workflow `.github/workflows/first-build-profile.yml` runs this on five native targets and retains each archive.

The producer commit is the pull request head the workflow built. Psyche squash-merges, so that commit stays reachable through `refs/pull/<n>/head` rather than through `main`. A consumer that needs the bytes past Actions retention keeps the archive and its pins with its own fixtures.

Regenerate the cases after changing a vector or the selection with `python3 -I profiles/first-build-profile/make_denials.py`. `cargo test --locked -p psyche-test-support --test first_build_profile` checks every case against the Rust decoder and runs the runner against tampered, missing, extra, linked, repinned, downgraded and newer-major kits.

## Against the #11 publication contract

| #11 item | This kit |
|---|---|
| Inventory and classification | Covered by the inventory and surface documents. The manifest records `proposed_class` and requires every registry kind to be selected or excluded. |
| Schemas | Only the ADR 4 binding has a JSON Schema. The record kinds have none. The vectors and decoder serve as the definition for now. That gap remains open. |
| Canonical bytes and SHA-256 vectors | 14 kinds. |
| Positive, denial and unknown-version fixtures | 14 positive vectors, 28 canonicalization cases, 52 rejections including unknown and older majors per kind. |
| Stale-correlation, crash/restart and downgrade fixtures | Restart through receipt replay. Profile downgrade and newer-major refusal through the runner. Stale-correlation vectors for `execution_binding` remain open (rows 11 and 12). |
| Standalone runner, bounded machine-readable results, no credentials | `verify-profile` and `verify-build-binding`, with JSON output under 1 KiB and no network access. |
| Drift detection | Vector pins, `make_denials.py --check`, the decoder test and the kit's exact blob checks. |
| Versioning, compatibility and rollback | The single-pin POLICY in the surface document, plus the consumer guide. |
| Signed or checksummed immutable artifact set | Checksummed through Actions artifact IDs and digests. Not signed (an open owner decision). |

## Rollback

Delete the kit and its pins. No record, journal or store migrates. Reverting this directory, its workflow, the runner and its test removes the producer.
