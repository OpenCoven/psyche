# Build receipt binding v1

ADR 4 assigns issuance of the first Build receipt binding to Psyche. This directory specifies the wire record and provides a reference-verifier fixture for #11. Build retains its native journal. Its adapter supplies the original opaque identifiers, the exact receipt bytes and the native outcome classification from the authoritative effect path. [Owner decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117194956) · [Wire schema](binding.schema.json)

## What the issuer attests

Before signing, the Psyche issuer must authenticate the source of the native journal evidence, check that the task and action or idempotency reference belong to the expected execution request, and retrieve the receipt from the authoritative outcome path. It validates the typed Psyche execution request, recomputes its canonical digest, hashes the exact receipt bytes and signs the resulting canonical binding. A self-reported success, UI state or matching pair of digests supplies insufficient evidence for issuance. An effect-count claim requires an independent witness. Key provisioning and connecting a live issuer to Build are separate rollout work.

| Signed field | Meaning |
|---|---|
| `schema_version`, `issuer` | Exact profile version and expected issuer principal. |
| `request_id`, `request_digest` | Original typed Psyche request ID and SHA-256 of its canonical JSON bytes. |
| `task_id`, `action_id`, `idempotency_key` | Opaque Build references. The task is required, together with at least one action or idempotency reference. Where both exist, bind both. |
| `receipt_digest` | SHA-256 of the original native receipt bytes. A change in whitespace, encoding or trailing newline changes this digest. |
| `outcome` | `terminal` means the authoritative path has a final disposition, including a terminal failure. `ambiguous` requires reconciliation. |

The detached OpenSSH SSHSIG covers the complete RFC 8785 canonical binding bytes in namespace `psyche.build-receipt-binding.v1`. The signing principal expected by the verifier is `psyche-receipt-issuer`. The verifier accepts an allowed-signers policy from the operator’s independently trusted configuration. Production operators must pin the authorized issuer keys and namespace, protect that policy from writes by the evidence supplier, and establish rotation and revocation before rollout. The `issuer` string in a record cannot establish trust in a key.

## Run the reference verifier

Build the verifier with `cargo build --locked -p psyche-test-support --bin verify-build-binding`. It requires OpenSSH’s `ssh-keygen` with SSHSIG support and a writable private temporary directory for its verification snapshots. From a trusted checkout, run the following command.

```sh
target/debug/verify-build-binding ALLOWED_SIGNERS CONTEXT.json REQUEST.json RECEIPT.bin BINDING.json BINDING.sig
```

Each input must be a regular file of at most 65,536 bytes. An opaque reference is a string of 1–255 printable ASCII characters excluding space. Omit unavailable optional references. An explicitly null reference is invalid.

Supply the expected context and canonical request from the original trusted operation. The context is a JSON object containing the expected task and action or idempotency references and the native outcome (`terminal`, `ambiguous`, or `missing`). Source this context independently of the proposed binding. Supply the exact native receipt from the journal. The verifier compares all supplied reference values and their presence, the typed request ID, the request digest and the receipt digest. The detached signature must be for the supplied canonical record under the expected principal and namespace.

| Disposition | Consumer behavior |
|---|---|
| Verified binding | Use the verified correlation. Preserve the native receipt’s actual terminal disposition. |
| Missing evidence, including an absent detached signature | Report unknown, keep the request pending recovery and seek the authoritative evidence. |
| Ambiguous outcome | Report unknown until the canonical reconciliation path resolves it. |
| Conflicting authenticated evidence | Preserve the original and quarantine the conflicting evidence with its provenance. |
| Unavailable verifier | Report `denied/verifier_unavailable` if the verifier cannot create its snapshots or launch the verification backend. Keep the evidence for a later verification. |
| Unsupported version, malformed input or untrusted issuer | Refuse the binding. Keep any previously accepted record intact. |

The verifier reads its inputs and emits a bounded JSON decision with a result, a code and the redispatch flag. Exit 0 means the binding verified. Every other disposition exits nonzero. A disagreement on authenticated correlation or receipt bytes is a conflict. When those facts agree but either side records ambiguity, the outcome remains unknown. The adapter owns persistence and quarantine. Every result carries `redispatch: false`. A verified binding alone cannot authorize execution, retries or a claim about how often an effect occurred.

## Golden bytes and negotiation

The [golden binding](golden/binding.json) uses the [existing launch request](../../crates/psyche-coven/tests/fixtures/execution-request-launch.json), the [native receipt](golden/receipt.json) and [expected context](golden/context.json). Each digest below is SHA-256 over the file bytes. Run `cargo test --locked -p psyche-test-support --test build_receipt_binding` to exercise this fixture and the denial cases. The tests generate disposable signing keys.

| Input | Digest |
|---|---|
| Canonical request | `75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04` |
| Exact native receipt | `478f95fe977e50aaeaa22d450fd3e33cbf1a0a56526660b98331115fec257c24` |
| Canonical binding | `9def34c2cb39bfec001be9d7fcfb56f821f5b90fb655abd452bfab13dbf45843` |

Negotiate the exact binding version, signer policy and verifier revision out of band before enabling this profile. A peer without that version must preserve stored bytes and report that verified binding is unavailable. The existing experimental comparator remains available for its observation-only use. Rolling back the verifier restores that earlier capability and keeps signed evidence available for a compatible reader. This candidate adds an opt-in wire format alongside the existing G2 storage registry. It requires no migration of existing journals. Stable publication, production key distribution and live Build activation have separate approval gates. [Experimental replay profile](../receipt-replay/README.md) · [Build adoption tracker](https://github.com/OpenCoven/psyche-build/issues/253)
