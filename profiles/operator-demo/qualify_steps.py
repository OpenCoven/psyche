#!/usr/bin/env python3
"""Drive the published stepped launcher from a terminal and inspect it from a second process."""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import time


PROMPT = b"Enter to continue, q to quit > "
CASES = [
    ("acknowledged", "Missing receipt", "unknown", "recovery_required"),
    ("acknowledged", "Verified receipt", "verified", "binding_verified"),
    ("acknowledged", "Quarantined conflict", "quarantine", "conflicting_evidence"),
    ("lost-ack", "Lost acknowledgment", "unknown", "reconciliation_required"),
]


def require(condition, detail):
    if not condition:
        raise RuntimeError(detail)


def read_terminal(fd, until_prompt):
    output = bytearray()
    end = time.monotonic() + 180
    while True:
        remaining = end - time.monotonic()
        if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
            raise RuntimeError("Timed out waiting for the presenter terminal")
        try:
            chunk = os.read(fd, 16384)
        except OSError as error:
            if error.errno != errno.EIO:
                raise
            chunk = b""
        if not chunk:
            break
        output.extend(chunk)
        if until_prompt and output.endswith(PROMPT):
            break
    if until_prompt and not output.endswith(PROMPT):
        raise RuntimeError("Launcher exited before its prompt: " + output.decode(errors="replace")[-500:])
    return output.decode().replace("\r", "")


def native_snapshot(root):
    result = {}
    for name in ("acknowledged", "lost-ack"):
        lane = root / name
        require((lane / "native/effects").read_bytes() == b"submit\n", f"{name}: handler count changed")
        require(not (lane / "native/fixture-credentials.json").exists(), f"{name}: retained a control token")
        result[name] = tuple(hashlib.sha256((lane / file).read_bytes()).hexdigest()
                             for file in ("receipt.bin", "native/.psyche/runtime/events.ndjson"))
    require(not list(root.glob("issuer-*")), "Found a disposable issuer directory during inspection")
    return result


def rehearse(build, cli, exit_at, key, keep, expected_exit):
    with tempfile.TemporaryDirectory(prefix="ps step ", dir="/tmp") as work:
        argv = [sys.executable, str(Path(__file__).with_name("run.py")), "--build-checkout", str(build), "--step"]
        if cli:
            argv.extend(["--psyche", str(cli)])
        if keep:
            argv.append("--keep")
        pid, fd = pty.fork()
        if pid == 0:
            os.environ["TMPDIR"] = work
            os.execv(sys.executable, argv)
        try:
            output = read_terminal(fd, True)
            commands = {name: shlex.split(line) for name, line in
                        re.findall(r"^Reopen (acknowledged|lost-ack): (.+)$", output, re.M)}
            require(set(commands) == {"acknowledged", "lost-ack"}, "Both reopen commands must precede the first pause")
            root = Path(commands["acknowledged"][2]).parent
            require(root.parent.resolve() == Path(work).resolve(), "Launcher used unexpected session directory")
            before = native_snapshot(root)
            views = {}
            for name, args in commands.items():
                vid = (root / name / "verification-id").read_text().strip()
                require(args[1:] == ["--config", str(root / "psyche.toml"), "verification", "show", vid],
                        "Unexpected read-only inspection command")
                views[name] = json.loads(subprocess.check_output(args, cwd=work, timeout=30))
                require(views[name]["verification_id"] == vid and views[name]["redispatch"] is False,
                        "Reopened the wrong verification")
            require(views["lost-ack"]["observation_count"] == 0, "Lost acknowledgment was supplied before inspection")
            for index, (name, label, result, code) in enumerate(CASES):
                if index:
                    output = read_terminal(fd, True)
                args = commands[name]
                require(f"Checkpoint: {label} ({args[-1]})" in output, "Unexpected checkpoint or ID")
                inspect = re.search(r"^Inspect: (.+)$", output, re.M)
                require(inspect and shlex.split(inspect[1]) == args, "Printed inspection command differs")
                view = json.loads(subprocess.check_output(args, cwd=work, timeout=30))
                require((view["result"], view["code"], view["redispatch"]) == (result, code, False), label)
                if index == 0:
                    require(view["missing_evidence"] == ["receipt"], "Missing receipt was not identified")
                if index == 1:
                    require("denied/untrusted_issuer" in output, "Missing wrong-signer denial")
                if index >= 1:
                    require(view["original_receipt_digest"] == "sha256:" + before[name][0], "Original receipt changed")
                if index == 2:
                    require(view["quarantine_count"] == 1 and view["original_result"] == "verified", label)
                if index == 3:
                    require(view["native_outcome"] == "ambiguous", label)
                views[name] = view
                require(native_snapshot(root) == before, "Native evidence changed at a pause")
                if index == exit_at:
                    os.write(fd, key)
                    break
                if index == 0:
                    os.write(fd, b"invalid\n")
                    require("Press Enter" in read_terminal(fd, True), "Unrecognized input advanced the demo")
                os.write(fd, b"\n")
            tail = read_terminal(fd, False)
            _, status = os.waitpid(pid, 0)
            pid = None
            require(os.waitstatus_to_exitcode(status) == expected_exit, "Unexpected launcher exit status")
            require(("PASS  Both restarts" in tail) == (exit_at is None), "Incorrect completion message")
            if keep:
                require(f"Private operator session: {root}" in tail, "Retained session missing from output")
                for name, args in commands.items():
                    require(json.loads(subprocess.check_output(args, cwd=work, timeout=30)) == views[name],
                            "Saved state differs after exit")
                require(native_snapshot(root) == before, "Native evidence changed after exit")
            else:
                require(not root.exists(), "Session survived without --keep")
        finally:
            os.close(fd)
            if pid is not None:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-checkout", type=Path, required=True)
    parser.add_argument("--psyche", type=Path)
    options = parser.parse_args()
    for label, index, key, keep, status in [
        ("complete", None, b"", True, 0),
        ("early quit", 0, b"q\n", True, 0),
        ("terminal EOF", 1, b"\x04", True, 0),
        ("terminal interrupt", 2, b"\x03", False, 130),
    ]:
        started = time.monotonic()
        rehearse(options.build_checkout, options.psyche, index, key, keep, status)
        print(f"PASS  {label} ({time.monotonic() - started:.1f}s)", flush=True)
