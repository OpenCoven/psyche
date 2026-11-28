# Verify a Psyche Build receipt

An operator can register a request, supply evidence for it and reopen the decision by ID. This walkthrough uses two native Build actions with synthetic browser handlers. One acknowledges the action. The other loses its acknowledgment after the handler runs. Each verification command starts a new Psyche process backed by the same private SQLite store.

## Prepare the presenter machine

Use arm64 macOS or x86_64 Linux with Python 3.9+, OpenSSH with SSHSIG support, Git, Node 24, Corepack and the Rust toolchain in this checkout. Allow Build's fixture to open a local Unix socket. From the Psyche repository root, prepare a dedicated Build checkout once. Build's package manifest selects pnpm 10.34.5.

```sh
build=../psyche-build-operator-demo
git clone --no-checkout https://github.com/OpenCoven/psyche-build.git "$build"
git -C "$build" switch --detach dfd4b3cf2f06c7d415e647f778f007c6042fdbba
(cd "$build" && corepack pnpm install --frozen-lockfile)
cargo build --locked -p psyche-cli --bin psyche
```

## Launch

```sh
python3 profiles/operator-demo/run.py --build-checkout ../psyche-build-operator-demo --step --keep
```

The launch builds the local CLI with Cargo's lockfile and discovers its executable. Use `--psyche PATH` to select an already-built CLI. The presenter checks the clean Build source pin, the installed lockfile and the SHA-256 of the archived verifier for this host before executing the fixtures. The verifier comes from Psyche commit `557c579ed19f97e4177d4a6760e183eab1df11b2`. [Build fixture and archive pins](https://github.com/OpenCoven/psyche-build/tree/dfd4b3cf2f06c7d415e647f778f007c6042fdbba/protocol-fixtures/psyche-receipt-binding)

Both Build actions run once during setup, before the first pause. A synthetic browser handler appends one line to each local witness. Build's test operator grants a scoped lease and approves each action. A disposable issuer signs the samples. Both verification IDs are registered and the private signing keys and native control tokens are removed before inspection starts.

At each pause, copy the printed `Inspect` command into another terminal. It runs a fresh Psyche process using the session's config and verification ID. Press Enter to advance. Type `q` and press Enter, or send EOF (Ctrl-D on an empty line), to finish early with exit 0. Ctrl-C interrupts with exit 130. With `--keep`, finishing the demo or exiting at a pause retains both requests at the last decision shown. Either printed `Reopen` command works after exit. Without `--keep`, exit removes the private session. A setup failure discards the incomplete session.

For an unattended rehearsal, omit `--step`. This command completes the same decisions without reading input.

```sh
python3 profiles/operator-demo/run.py --build-checkout ../psyche-build-operator-demo --keep
```

## Five-minute presenter script

| Time | What to show and say |
|---|---|
| 0:00–0:45 | Launch the walkthrough. Point to the Build commit and verifier digest. “We’ll follow one operator request through submission, evidence and restart. Then we’ll repeat with a lost acknowledgment.” |
| 0:45–1:30 | Show the two sets of native task, action and verification IDs, then inspect the acknowledged request at the missing-receipt pause. “Build reports `succeeded`. Psyche has registered the request and its trusted issuer policy. The `Awaiting` line names the missing receipt, binding and signature.” |
| 1:30–2:15 | Advance to the verified-receipt pause and run its printed command. Read the three `supply` results. A missing receipt gives `unknown/recovery_required`. A signature from a second key gives `denied/untrusted_issuer`. The trusted signature over the original receipt gives `verified/binding_verified`. The handler count stays at one through all three submissions. |
| 2:15–3:15 | Advance to the quarantined-conflict pause. Show `Reopen both`. A new native reader reopens the receipt using the original Build IDs. A new Psyche process reads the saved decision by verification ID. Next, the trusted issuer signs a binding with a conflicting receipt digest. Point to `quarantine/conflicting_evidence`, the preserved original receipt digest and the unchanged journal. |
| 3:15–4:15 | Advance to the lost-acknowledgment pause and show its different action and verification IDs. “The handler ran once before the acknowledgment was lost. Build reports `unknown`. Psyche authenticates the evidence and reports `unknown/reconciliation_required`. An operator can investigate the receipt without issuing a second action.” Show the same decision after both processes reopen. |
| 4:15–5:00 | Press Enter to finish. Run both printed `Reopen` commands from a second terminal and show the acknowledged request. Point out the latest quarantine result alongside `original_result=verified` and `original_receipt_digest`. Show the lost-ack request and its one native witness. The operator can choose different candidate files at the CLI. |

## Use the operator controls

Set `session` to the private directory printed on exit with `--keep`. The default binary below assumes Cargo's normal target directory. Use the path printed by the walkthrough if your build uses another target directory.

```sh
session=/path/printed/by/the/walkthrough
psyche=target/debug/psyche
lane="$session/acknowledged"
id=$(cat "$lane/verification-id")
"$psyche" --config "$session/psyche.toml" verification show "$id"
```

To register a verification request yourself, use the original request bytes, expected native context and your own allowed-signers file. The operator obtains the context from the native reader and selects the trust policy independently of evidence senders. The sample's trusted public key is in `allowed_signers`. The CLI snapshots these inputs at submission. Identical inputs reopen the existing registration. Use the `verification_id` printed by this command for subsequent operations.

```sh
"$psyche" --config "$session/psyche.toml" verification submit \
  --request "$session/request.json" --context "$lane/context.json" \
  --allowed-signers "$session/allowed_signers"
```

Supply alternative evidence by changing any of the three file arguments below. A verified result exits 0. Denied, unknown and quarantined results return a JSON decision and exit 5. An omitted receipt, binding or signature represents missing evidence. For the original acknowledged receipt use `binding.json.sig`. To try the untrusted issuer use `wrong.json.sig`. To supply the authenticated conflict use `conflict.json` and `conflict.json.sig` together.

```sh
"$psyche" --config "$session/psyche.toml" verification supply "$id" \
  --verifier "$session/verify-build-binding" --receipt "$lane/receipt.bin" \
  --binding "$lane/binding.json" --signature "$lane/wrong.json.sig"
```

Read the other request and its native witness independently. Expect `native_outcome=ambiguous`, `result=unknown`, `code=reconciliation_required`, `redispatch=false` and one witness line.

```sh
lane="$session/lost-ack"
id=$(cat "$lane/verification-id")
"$psyche" --config "$session/psyche.toml" verification show "$id"
wc -l < "$lane/native/effects"
```

Repeat the launch for a second clean rehearsal. A fresh pair of Build actions produces fresh task, action and verification IDs. A passing run checks that the native journals remain byte-for-byte unchanged during verification and that each action's handler ran exactly once. Remove a retained session directory when you finish. The fixture uses a local synthetic issuer and a synthetic browser handler. Production key provisioning, a live browser action and release qualification are outside this demonstration.
