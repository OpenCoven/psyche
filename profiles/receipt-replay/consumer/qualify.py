"""Run in an isolated directory containing the pinned archive and this consumer fixture only."""

import errno
import hashlib
import json
import runpy
import socket
import subprocess
import sys
from pathlib import Path
from zipfile import ZipFile

PYTHON = [sys.executable, "-I"]
ARCHIVE = Path("receipt-replay.pyz")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def command(expected, *args, archive=ARCHIVE):
    run = subprocess.run([*PYTHON, str(archive), *args], capture_output=True, check=False)
    result = json.loads(run.stdout)
    if expected == "passed":
        require(run.returncode == 0 and result.get("result") == expected, result)
    else:
        require(run.returncode != 0 and result == {"result": "denied", "code": expected}, result)
    print(json.dumps(result, sort_keys=True))
    return result


def network_disabled():
    # UDP connect does a route lookup without sending application data.
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.connect(("192.0.2.1", 9))
    except OSError as error:
        require(error.errno in {errno.EPERM, errno.EACCES, errno.ENETUNREACH, errno.EHOSTUNREACH}, error)
        return
    raise RuntimeError("consumer network is available")


def qualify(pin):
    network_disabled()
    require(hashlib.sha256(ARCHIVE.read_bytes()).hexdigest() == pin, "wrong archive pin")
    witness_controls = runpy.run_path(str(Path(__file__).with_name("check_witness.py")))
    witness_controls["run_controls"]()
    recovery = runpy.run_path(str(Path(__file__).with_name("recovery.py")))
    receipt, effects, restart_delta = recovery["exercise_recovery"]()
    args = ["request.json", "first.json", "replay.json"]
    passed = command("passed", *args)
    require(effects == 1 and restart_delta == 0, "independent effect count disagrees")
    require(passed["receipt_digest"] == "sha256:" + hashlib.sha256(receipt).hexdigest(), "wrong receipt")
    controls = runpy.run_path(str(Path(__file__).with_name("check_inputs.py")))
    controls["run_controls"](command, args)
    replay = json.loads(Path("replay.json").read_bytes())
    for change, denial in [
        ({"schema_version": "psyche.experimental.receipt-replay.v2"}, "unsupported_version"),
        ({"request_digest": "sha256:" + "0" * 64}, "request_digest_mismatch"),
        ({"request_id": "req_01J00000000000000000000043"}, "request_identity_mismatch"),
        ({"receipt_hex": "00"}, "receipt_mismatch"), ({"executions": 2}, "execution_count_mismatch"),
        ({"executions": True}, "invalid_observation"), ({"executions": 1.5}, "invalid_observation"),
    ]:
        Path("changed.json").write_text(json.dumps({**replay, **change}))
        command(denial, *args[:2], "changed.json")
    Path("integral.json").write_text(json.dumps({**replay, "executions": 1.0}))
    command("passed", *args[:2], "integral.json")
    request = json.loads(Path("request.json").read_bytes())
    for cwd, expected in [("/fixture/sub", "passed"), ("/fixture-other", "cwd_outside_project_root")]:
        raw = json.dumps({**request, "cwd": cwd}, sort_keys=True, separators=(",", ":")).encode()
        Path("path-request.json").write_bytes(raw)
        bound = {**replay, "request_digest": "sha256:" + hashlib.sha256(raw).hexdigest()}
        Path("path-observation.json").write_text(json.dumps(bound))
        command(expected, "path-request.json", "path-observation.json", "path-observation.json")
    command("missing_file", *args[:2], "absent.json")
    with ZipFile(ARCHIVE) as original:
        Path("golden-request.json").write_bytes(original.read("golden/request.json"))
        Path("golden-observation.json").write_bytes(original.read("golden/observation.json"))
        golden = command("passed", "golden-request.json", "golden-observation.json", "golden-observation.json")
        require(golden["request_digest"] == "sha256:75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04", "wrong golden")
        for denial in ("file_digest_mismatch", "missing_file"):
            altered = Path(denial + ".pyz")
            with ZipFile(altered, "w") as archive:
                for name in original.namelist():
                    data = original.read(name)
                    if name == "golden/request.json":
                        if denial == "missing_file":
                            continue
                        data += b" "
                    archive.writestr(name, data)
            command(denial, *args, archive=altered)
    print(json.dumps({"archive_sha256": pin, "network": "disabled", "durable_effects": effects,
                      "executions_after_restart": restart_delta, "platform": sys.platform, "result": "qualified"}))


if __name__ == "__main__":
    qualify(sys.argv[1])
