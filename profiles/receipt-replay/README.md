# Experimental receipt replay profile v1

A downstream consumer can use this profile to check a synthetic launch across a process restart. It binds the same Psyche execution request to the exact bytes of a consumer-owned terminal receipt and checks that the effect ran once. The case follows Build’s durable exact-outcome sidecar replay. Build’s task, action, approval, and receipt representations remain under owner review. [Stage 0 map](https://github.com/OpenCoven/psyche-build/pull/262) · [Build replay case](https://github.com/OpenCoven/psyche-build/blob/4ed92de5903ac0d7e6442d72eb7143098045f26a/__tests__/controlRuntime.test.ts#L315) · [Ownership decision](https://github.com/OpenCoven/psyche/issues/12)

## Run offline

Download the preview artifact from the PR’s **Offline receipt replay** workflow and unpack the outer GitHub artifact. Match the SHA-256 of the inner `receipt-replay.pyz` against the digest in the reviewed handoff before executing it. The archive contains executable Python, its manifest, two JSON Schemas and the golden inputs. Python 3.9 or newer supplies all runtime dependencies.

Submit the synthetic request to your test consumer, save its observation as `first.json`, stop the process, reopen the same durable store in a new process and submit the identical request. Save that observation as `replay.json`. Use a fresh fixture store so the cumulative effect count starts at zero. Each observation contains the original request ID, the SHA-256 of the canonical request bytes with a `sha256:` prefix, the exact durable receipt bytes encoded as lowercase hex, the cumulative number of actual effects and the schema version `psyche.experimental.receipt-replay.v1`. Count the effect independently of the receipt lookup. Run this command from the consumer directory.

```sh
python3 -I receipt-replay.pyz request.json first.json replay.json
```

Exit 0 emits the request ID, request and receipt digests, total executions and additional executions after restart. A denial exits 1 and prints a JSON code. The runner checks the observations the consumer supplies. The qualification fixture additionally queries its durable effects and receipts directly. The profile grants no authority to launch or repeat an actual operation.

## Wire files and scope

`launch.schema.json` describes the supported synthetic launch. Its fixed fields come from the canonical Rust request golden, whose SHA-256 is `75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04`. Consumers may supply their own request ID, principal and fixture paths within the schema. Use canonical JSON bytes without a trailing newline. This fixture exercises historical launch data offline. Its timestamps confer no current authorization.

`observation.schema.json` defines the two observations. The runner compares raw receipt bytes, enforces each observation’s binding to the request and requires the effect count to equal one both times. Unknown versions, unsupported launch shapes, digest mismatches, missing files and replay discrepancies have distinct failures. The manifest pins every other archive member. Its own integrity and the executable entry point are covered by the external archive pin.

## Reproduce the preview

Run `bash profiles/receipt-replay/qualify.sh` in a clean Git checkout on Linux with passwordless sudo for a network namespace, or on macOS with sandbox-exec. The script builds from two separate exports of HEAD, compares the archives byte for byte and copies the archive and independent fixture to a new directory. The fixture runs with networking disabled, restarts its SQLite-backed synthetic consumer and runs positive and negative controls. To retain the archive, pass an absolute output path. The packager reuses the packaging probe’s sorted member list, fixed metadata and per-file SHA-256 manifest. [Original packaging results](https://github.com/OpenCoven/psyche/issues/11#issuecomment-7036825643)

Consumers can discard this preview by deleting the archive and their synthetic fixture directory. No production data migration or Build activation is part of this profile.
