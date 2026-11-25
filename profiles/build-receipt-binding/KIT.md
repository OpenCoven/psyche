# Experimental native Build receipt verifier kit

This kit runs the Psyche receipt-binding verifier offline for one native Rust target. It requires OpenSSH with SSHSIG support (`ssh-keygen -Y verify` on PATH) and a writable private temporary directory. The golden signer is a disposable fixture. Use its public policy only with this example. For a real binding, obtain the allowed-signers policy from the operator and the expected context, canonical request and native receipt from the original trusted operation.

## Fetch and pin

Obtain the full producer commit, target triple, immutable Actions artifact ID, outer ZIP SHA-256 and inner tar.gz SHA-256 from the reviewed producer handoff. Download the artifact by ID using GitHub’s artifact download endpoint. Check the complete ZIP against its published digest, then unpack its single file, `verify-build-binding-<target>.tar.gz`. Check this file against the independently recorded inner digest before extracting or running it. Use `sha256sum`, `shasum -a 256`, or PowerShell’s `Get-FileHash -Algorithm SHA256`. Retain the exact inner archive and pins with the consumer’s test fixtures for restoration after Actions retention expires.

Extract the verified tar.gz into an empty directory for that target. The manifest records the source revision, release profile, target, compiler and SHA-256 for every payload file, including the verifier, schema and golden inputs. Check the manifest revision and target against the reviewed handoff. The archive digest binds this manifest. The producer kit builder also provides an offline check that compares the pins and exercises all four dispositions in a fresh directory.

## Run the golden case

From the extracted root, run the following command. On Windows, use `.\bin\verify-build-binding.exe` for the executable.

```sh
./bin/verify-build-binding golden/allowed_signers golden/context.json golden/request.json golden/receipt.json golden/binding.json golden/binding.sig
```

The golden case exits 0 and returns `{"code":"binding_verified","redispatch":false,"result":"verified"}`. An absent signature returns `unknown/recovery_required`. Different receipt bytes return `quarantine/conflicting_evidence`. An empty signer policy returns `denied/untrusted_issuer`. Each of these three refusals exits 1 and keeps `redispatch` false. Preserve the exact original receipt bytes, including any final newline, when supplying journal evidence.

The kit contains a public fixture key policy and detached signature. Its signing key was generated only to sign the checked-in golden binding and discarded. The kit does not provision an operational issuer. Verification attests correlation to the signed evidence. It gives no authority to submit another effect.

## Upgrade and rollback

Negotiate the exact profile and target before changing the pinned archive. Retain the prior archive, digest and native receipt bytes so the consumer can restore its previous verifier. Hold an unknown version or unavailable verifier for recovery. The kit is an opt-in experimental distribution with no storage migration.
