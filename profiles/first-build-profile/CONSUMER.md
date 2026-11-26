# Experimental first Build profile prerelease kit

This kit lets a downstream repository check the first Build profile offline, without a Psyche checkout, a network connection or any credential. It is an **experimental prerelease** (`psyche.experimental.first-build-profile.v1`). No kind in it is stable, the profile is not published as a release, and nothing in it authorizes an execution, a retry or a redispatch.

## Fetch and pin

Obtain the full producer commit, target triple, immutable Actions artifact ID, outer ZIP SHA-256 and inner `first-build-profile-<target>.tar.gz` SHA-256 from the reviewed producer handoff on Psyche issue #11. Download the artifact by ID, check the ZIP against its digest, unpack its single file and check that file against the inner digest **before** extracting it (`sha256sum`, `shasum -a 256` or `Get-FileHash -Algorithm SHA256`). Extract it into an empty directory. Keep the exact archive and pins with your test fixtures so you can restore them after Actions retention expires.

## Check the profile

From the extracted root (on Windows, add `.exe` to each executable):

```sh
./bin/verify-profile . --source <producer commit> --target <target triple>
```

Both pins are required. Exit 0 prints `{"result":"passed","code":"profile_verified",...,"redispatch":false}`. The selection, the vectors and the cases are compiled into the runner, so a kit cannot narrow them by editing its manifest. The runner checks that:

- the directory holds exactly this profile's member set for the target, each with the SHA-256 the manifest lists, with no extra member, empty directory, link or special file;
- the manifest is this profile's format and major, has no unknown, duplicate or oversized field, and names the commit and target you passed. An older or newer major gives `unsupported_profile_version`, another profile gives `unknown_profile`, a widened or ambiguous manifest gives `manifest_invalid` and a different producer gives `provenance_mismatch`;
- the selection is unchanged (`selection_mismatch`) and each component pin matches the bytes of the member it names (`component_mismatch`), including the receipt-replay archive digest used below;
- each selected kind's vector decodes, re-encodes to exactly its own bytes (RFC 8785) and matches both its manifest digest and the profile's own vector;
- `denials.json` is the profile's own generated case set, complete. Every case gives its pinned result. An unknown or older major, an unknown field, schema or enum value, duplicate keys, trailing data, truncation and an unsafe integer are rejected with the stated reason. Reordered or re-spaced spellings re-encode to the pinned canonical bytes.

Any other result exits 1 with `{"result":"denied","code":...}`. Treat it as a refusal, keep the previously accepted kit, and do not proceed.

`denials.json` is portable: a consumer in another language can run its own decoder over the same inputs and compare its classification with each `expect`.

## Check the receipt binding (ADR 4)

```sh
./bin/verify-build-binding binding/golden/allowed_signers binding/golden/context.json binding/golden/request.json binding/golden/receipt.json binding/golden/binding.json binding/golden/binding.sig
```

The golden case exits 0 with `binding_verified`. The verifier checks the issuer signature, the canonical request digest and the exact receipt digest. An absent signature returns `unknown/recovery_required`. Different receipt bytes return `quarantine/conflicting_evidence`. An empty signer policy returns `denied/untrusted_issuer`. Every disposition carries `redispatch: false`. It needs OpenSSH `ssh-keygen` with SSHSIG support. The golden signer is a disposable fixture: obtain a real signer policy from the operator. See `binding/binding.schema.json`.

## Check restart behavior

Copy `receipt-replay/consumer/*` and `receipt-replay/receipt-replay.pyz` to a fresh directory and run, with networking denied (Linux: `sudo -n unshare --net -- setpriv --reuid=$(id -u) --regid=$(id -g) --clear-groups python3 -I qualify.py <pin>`; macOS: `sandbox-exec -p '(version 1)(allow default)(deny network*)' python3 -I qualify.py <pin>`), where `<pin>` is `components.receipt-replay.archive_sha256` from the manifest. The synthetic consumer restarts its durable store and must report one effect and zero additional executions after restart, with the same request ID and receipt bytes. Its controls refuse an unsupported version, digest and identity mismatches and missing files. It requires Python 3.9 or newer.

## What this kit does not do

- It does not make any kind stable. `proposed_class` records the publication-surface proposal awaiting owner review.
- It does not cover row 13 (`surface_effect`, `delivery`) or row 11 fencing, which stay held.
- It is not signed. Its integrity rests on the independently recorded archive digest. A signing identity is an open owner decision.
- It provisions no issuer key and activates nothing in Build.

## Upgrade and rollback

Pin one exact kit. To change it, negotiate the new producer commit and digest first, keep the previous archive and its pins, and restore them to roll back. A reader that cannot verify a kit keeps the stored bytes and blocks the write. The kit needs no storage migration.
