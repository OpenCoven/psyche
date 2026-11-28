#!/usr/bin/env python3
"""Present two real Build fixture actions through the Psyche operator CLI."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile

BUILD = "dfd4b3cf2f06c7d415e647f778f007c6042fdbba"
PRODUCER = "557c579ed19f97e4177d4a6760e183eab1df11b2"
ARCHIVES = {
    ("Darwin", "arm64"): ("aarch64-apple-darwin", "9256f1ff1b8a907d9f713d04c1eb984938ad843a5a766e283cf19760cb146d1a"),
    ("Linux", "x86_64"): ("x86_64-unknown-linux-gnu", "bdd7a63ce527df07982c0cda9084cf4744bcf11c66bc01fd2d8a1fe153fe405c"),
}
RECOVERY = ("unknown", "recovery_required")
VERIFIED = ("verified", "binding_verified")
AMBIGUOUS = ("unknown", "reconciliation_required")
CONFLICT = ("quarantine", "conflicting_evidence")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def prepare_build(checkout, root):
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    require(head == BUILD, f"Build checkout must be at {BUILD}")
    status = subprocess.check_output(["git", "--no-optional-locks", "status", "--porcelain", "--untracked-files=all"], cwd=checkout)
    require(not status, "Use a clean, dedicated Build checkout")
    require((checkout / "node_modules/tsx/dist/loader.mjs").is_file(), "Install Build's frozen pnpm dependencies first")
    installed = checkout / "node_modules/.pnpm/lock.yaml"
    require(installed.is_file() and installed.read_bytes() == (checkout / "pnpm-lock.yaml").read_bytes(),
            "Installed dependencies must match Build's frozen lockfile")

    host = (platform.system(), platform.machine())
    require(host in ARCHIVES, "This demo has pinned verifier kits for arm64 macOS and x64 Linux")
    target, expected = ARCHIVES[host]
    fixtures = checkout / "protocol-fixtures"
    binding_fixture = fixtures / "psyche-receipt-binding"
    archive = (binding_fixture / f"verify-build-binding-{target}.tar.gz").read_bytes()
    require(hashlib.sha256(archive).hexdigest() == expected, "Verifier archive digest mismatch")
    verifier = root / "verify-build-binding"
    request = root / "request.json"
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as kit:
        for name, destination in [("bin/verify-build-binding", verifier), ("golden/request.json", request)]:
            members = [member for member in kit.getmembers() if member.name == name]
            require(len(members) == 1 and members[0].isfile(), f"Invalid pinned kit entry {name}")
            destination.write_bytes(kit.extractfile(members[0]).read())
    verifier.chmod(0o500)
    spec = importlib.util.spec_from_file_location("build_receipt_canary", binding_fixture / "canary.py")
    canary = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(canary)
    print(f"Build {BUILD}\nVerifier {PRODUCER} ({target}, archive SHA-256 {expected})")
    return canary, fixtures / "psyche-lifecycle"


def build_cli():
    repo = Path(__file__).resolve().parents[2]
    output = subprocess.check_output(
        ["cargo", "build", "--locked", "-p", "psyche-cli", "--bin", "psyche", "--message-format=json-render-diagnostics"],
        cwd=repo, text=True,
    )
    for line in output.splitlines():
        message = json.loads(line)
        if message.get("executable") and message["target"]["name"] == "psyche":
            return Path(message["executable"]).resolve()
    raise ValueError("Cargo did not report a psyche executable")


def sign_sample(canary, root, lane, keys, native):
    context = dict(task_id=native.task, action_id=native.action, idempotency_key=native.key,
                   outcome="ambiguous" if native.ambiguous else "terminal")
    request_bytes = (root / "request.json").read_bytes()
    (lane / "context.json").write_bytes(canary.canonical(context))
    (lane / "receipt.bin").write_bytes(native.receipt)
    binding = dict(context, schema_version="psyche.build_receipt_binding.v1", issuer=canary.ISSUER,
                   request_id=json.loads(request_bytes)["request_id"], request_digest=canary.digest(request_bytes),
                   receipt_digest=canary.digest(native.receipt))
    canary.sign(lane, "binding.json", binding, keys / "trusted")
    if not native.ambiguous:
        canary.sign(lane, "wrong.json", binding, keys / "other")
        conflict = dict(binding, receipt_digest=canary.digest(native.receipt + b"\n"))
        canary.sign(lane, "conflict.json", conflict, keys / "trusted")


def command(psyche, root, native, label, args, expected, exit_code):
    proc = subprocess.run([psyche, "--config", root / "psyche.toml", "verification", *args],
                          cwd=root, capture_output=True, timeout=30)
    require(proc.returncode == exit_code, f"{label}: expected exit {exit_code}, got {proc.returncode}: "
            + proc.stderr.decode(errors="replace")[:400])
    view = json.loads(proc.stdout)
    decision = (view["result"], view["code"])
    require(decision == expected and view["redispatch"] is False, f"{label}: unexpected verification decision {decision}")
    require((view["task_id"], view["action_id"], view["idempotency_key"]) == (native.task, native.action, native.key),
            f"{label}: native correlation changed")
    require(view["native_outcome"] == ("ambiguous" if native.ambiguous else "terminal"), f"{label}: native outcome changed")
    witness = native.check_unchanged()  # Compares the native journal bytes and the independent handler witness.
    print(f"  {label:<21} Build {native.state:<9} Psyche {decision[0]}/{decision[1]}   calls={witness['native_effects']}")
    return view


def prepare_lane(psyche, root, node, canary, fixture, keys, ambiguous):
    name = "lost-ack" if ambiguous else "acknowledged"
    lane = root / name
    lane.mkdir()
    request = root / "request.json"
    native = canary.NativeAction(lane / "native", node, fixture, request.read_bytes(), ambiguous)
    sign_sample(canary, root, lane, keys, native)
    before = native.check_unchanged()
    print(f"\n{name}  native task={native.task}  action={native.action}")
    print(f"  Build handler witness before verification: {before['native_effects']} call")
    submitted = command(psyche, root, native, "Submit request",
                        ["submit", "--request", request, "--context", lane / "context.json",
                         "--allowed-signers", root / "allowed_signers"], RECOVERY, 0)
    require(submitted["missing_evidence"] == ["receipt", "binding", "signature"],
            "Registration did not identify its pending evidence")
    print("  Awaiting: " + ", ".join(submitted["missing_evidence"]))
    verification_id = submitted["verification_id"]
    require(isinstance(verification_id, str) and verification_id, "Missing verification ID")
    require(submitted["request_digest"] == canary.digest(request.read_bytes())
            and submitted["request_id"] == json.loads(request.read_bytes())["request_id"]
            and submitted["observation_count"] == 0, "Unexpected request correlation or initial state")
    (lane / "verification-id").write_text(verification_id + "\n", encoding="ascii")
    print(f"  Psyche verification ID: {verification_id}")

    return native, verification_id


def show_command(psyche, root, verification_id):
    return shlex.join([str(psyche), "--config", str(root / "psyche.toml"), "verification", "show", verification_id])


class PresenterQuit(Exception):
    pass


def checkpoint(psyche, root, verification_id, decision):
    print(f"\nCheckpoint: {decision} ({verification_id})")
    print("Inspect: " + show_command(psyche, root, verification_id))
    while True:
        try:
            answer = input("Enter to continue, q to quit > ").strip().lower()
        except EOFError:
            raise PresenterQuit from None
        if not answer:
            return
        if answer in ("q", "quit"):
            raise PresenterQuit
        print("Press Enter to continue or type q to quit.")


def present_lane(psyche, root, native, verification_id, step):
    lane = native.cwd.parent
    before = native.check_unchanged()
    print(f"\n{lane.name}  verification={verification_id}")

    def invoke(label, args, expected, exit_code):
        result = command(psyche, root, native, label, [args[0], verification_id, *args[1:]], expected, exit_code)
        require(result["verification_id"] == verification_id, f"{label}: verification ID changed")
        return result

    def supply(label, expected, *, record="binding.json", signature="binding.json.sig", receipt="receipt.bin"):
        return invoke(label, ["supply", "--verifier", root / "verify-build-binding", "--receipt", lane / receipt,
                              "--binding", lane / record, "--signature", lane / signature], expected, 0 if expected == VERIFIED else 5)

    if not native.ambiguous:
        missing = supply("Missing receipt", RECOVERY, receipt="absent.bin")
        require(missing["missing_evidence"] == ["receipt"], "Incorrect missing evidence")
        if step:
            checkpoint(psyche, root, verification_id, "Missing receipt")
        supply("Wrong signer", ("denied", "untrusted_issuer"), signature="wrong.json.sig")
    expected = AMBIGUOUS if native.ambiguous else VERIFIED
    original = supply("Signed receipt", expected)
    receipt_digest = before["receipt_digest"]
    require((original["original_result"], original["original_code"], original["original_receipt_digest"])
            == (*expected, receipt_digest), "Authenticated original evidence missing")
    native.reopen()  # Fresh native reader using only the original Build IDs and its journal.
    reopened = invoke("Reopen both", ["show"], expected, 0)  # A new psyche process opens its configured data.
    require(reopened == original, "Restart changed the persisted operator view")
    if step:
        checkpoint(psyche, root, verification_id, "Lost acknowledgment" if native.ambiguous else "Verified receipt")
    if not native.ambiguous:
        quarantined = supply("Signed conflict", CONFLICT, record="conflict.json", signature="conflict.json.sig")
        require((quarantined["original_result"], quarantined["original_code"], quarantined["original_receipt_digest"])
                == (*expected, receipt_digest) and quarantined["quarantine_count"] == 1,
                "Quarantine did not preserve the authenticated original")
        again = invoke("Reopen quarantine", ["show"], CONFLICT, 0)
        require(again == quarantined, "Restart changed the quarantine or the original evidence")
        print(f"  Quarantined observations: {again['quarantine_count']}. "
              f"Retained original: {again['original_result']}/{again['original_code']}")
        if step:
            checkpoint(psyche, root, verification_id, "Quarantined conflict")
    after = native.check_unchanged()
    require(before == after, "Build changed while verifying")
    print(f"  Original receipt {receipt_digest}\n  Unchanged Build journal {after['journal_digest']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-checkout", required=True, type=Path, help="clean pinned Build checkout with frozen dependencies")
    parser.add_argument("--psyche", type=Path, help="use this built Psyche CLI (default: cargo build --locked)")
    parser.add_argument("--keep", action="store_true", help="keep the private session directory for manual CLI inspection")
    parser.add_argument("--step", action="store_true", help="pause at four decisions for read-only inspection")
    args = parser.parse_args()
    os.umask(0o077)
    cli = args.psyche.resolve(strict=True) if args.psyche else build_cli()
    node = shutil.which("node")
    require(node, "Node is required for Build's native fixture")
    checkout = args.build_checkout.resolve(strict=True)

    root = Path(tempfile.mkdtemp(prefix="psyche-operator-")).resolve()
    retained = False
    try:
        canary, fixture = prepare_build(checkout, root)
        os.environ.clear()
        os.environ.update(HOME=str(root), TMPDIR=str(root), PATH=os.defpath, TSX_DISABLE_CACHE="1")
        config = ('schema_version = "psyche.config.v1"\n' + 'data_dir = ' + json.dumps(str(root / "data"))
                  + '\n[coven]\nsocket = ' + json.dumps(str(root / "coven.sock")) + '\nrequired_api_version = "coven.daemon.v1"\n')
        (root / "psyche.toml").write_text(config, encoding="utf-8")
        with tempfile.TemporaryDirectory(dir=root, prefix="issuer-") as scratch:
            keys = Path(scratch)
            for name in ("trusted", "other"):
                canary.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "operator-demo", "-f", name], cwd=keys)
            (root / "allowed_signers").write_bytes(canary.ISSUER.encode() + b" " + (keys / "trusted.pub").read_bytes())
            print("Preparing two synthetic Build handlers and signing the examples with a disposable issuer.")
            lanes = [prepare_lane(cli, root, node, canary, fixture, keys, ambiguous) for ambiguous in (False, True)]
            require(lanes[0][1] != lanes[1][1], "Independent native actions must have separate verification IDs")
        # Both effects and registrations are complete. Inspections only use the saved evidence.
        for credential in root.glob("*/native/fixture-credentials.json"):
            credential.unlink()
        if args.step:
            for native, verification_id in lanes:
                print(f"Reopen {native.cwd.parent.name}: {show_command(cli, root, verification_id)}")
        exit_code = 0
        try:
            for native, verification_id in lanes:
                present_lane(cli, root, native, verification_id, args.step)
        except PresenterQuit:
            print("\nStopped at the presenter's request.")
        except KeyboardInterrupt:
            print("\nInterrupted.")
            exit_code = 130
        else:
            print("\nPASS  Both restarts preserved the receipts. Each Build handler ran once.")
        for native, _ in lanes:
            native.check_unchanged()
        if args.keep:
            retained = True
            print(f"CLI binary: {cli}\nPrivate operator session: {root}")
        return exit_code
    finally:
        if not retained:
            shutil.rmtree(root)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
    except subprocess.CalledProcessError as error:
        # Native canary subprocesses capture stderr. Identify a missing host identity without printing child data.
        if error.stderr and b"No user exists for uid" in error.stderr:
            sys.exit("Demo stopped: OpenSSH could not resolve the current OS user ID")
        sys.exit(f"Demo stopped: {error}")
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(f"Demo stopped: {error}")
