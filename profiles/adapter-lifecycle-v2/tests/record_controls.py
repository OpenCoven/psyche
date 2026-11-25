"""Exercise refusal and native-record disagreements against the pinned driver."""
from contextlib import closing
import json
from pathlib import Path
import runpy
import subprocess
import sys
from tempfile import TemporaryDirectory
from unittest.mock import patch
from zipfile import ZipFile


def child(archive, role, case, golden, audit):
    sys.path.insert(0, archive)
    from reference_fixture import adapt, observe, rows, submit

    message = json.load(sys.stdin)
    if role == "adapter":
        reply = adapt(message, Path(golden).read_bytes(), "conforming", Path(audit))
        if case == "replay-count" and Path.cwd().name == "success" and "state" in reply:
            if message["phase"] == "replay":
                submit(rows()[0]["binding"], Path(audit))
            reply["executions"] = len(rows())
        if case == "refusal-count" and reply.get("code") and reply["executions"] == 1:
            reply["executions"] = 0
        if case == "refusal-fields" and reply.get("code"):
            reply["schema_version"] = 99
    else:
        reply = observe(message, "conforming")
        if message["operation"] == "binding":
            if case == "denial-state" and Path.cwd().name == "agent_approval":
                reply["state"] = "succeeded"
            if case == "terminal-refusal" and Path.cwd().name == "success":
                reply["refusal"] = "operator_required"
    print(json.dumps(reply))


def check(archive, pin):
    sys.path.insert(0, str(archive))
    driver = runpy.run_path(str(archive))
    raw, comparator = driver["load_bundle"](archive, pin)
    with TemporaryDirectory() as folder:
        root = Path(folder)
        golden = root / "request.json"
        golden.write_bytes(raw)
        for case, expected in (("refusal-count", "invalid_refusal"), ("refusal-fields", "invalid_refusal"),
                               ("replay-count", "effect_count_mismatch"), ("denial-state", "refused_native_state_changed"), ("terminal-refusal", "unexpected_native_refusal")):
            commands = [[sys.executable, "-I", __file__, "child", str(archive), role, case, str(golden), str(root / case)]
                        for role in ("adapter", "observer")]
            result = subprocess.run([sys.executable, "-I", str(archive), "--sha256", pin, "--observer",
                                     json.dumps(commands[1]), "--", *commands[0]], capture_output=True, timeout=120)
            if result.returncode != 1 or json.loads(result.stdout).get("code") != expected:
                raise AssertionError((case, result.returncode, result.stdout, result.stderr))
            if case == "replay-count":
                trace = [json.loads(line) for line in (root / case).read_text().splitlines()]
                if len(trace) != 2:
                    raise AssertionError(("unexpected replay effect count", len(trace)))
                first, replay = trace
                if first["binding"] != replay["binding"] or first["execution_id"] == replay["execution_id"]:
                    raise AssertionError(("unexpected replay trace", trace))
            print(json.dumps(dict(control=case, expected_refusal=expected)))

        # Inject a storage discrepancy in the parent-owned disposable ledger immediately after the first append.
        # Run the complete driver so the check also covers how qualification consumes the witness.
        from effect_sink import EffectSink
        invoke = EffectSink.invoke

        def store_inconsistent_id(store, binding):
            issued = invoke(store, binding)
            with closing(store.connect("rw")) as db, db:
                db.execute("UPDATE effects SET execution_id = ? WHERE execution_id = ?", ("fixture-mismatch", issued))
            return issued

        fixture = root / "reference_fixture.py"
        with ZipFile(archive) as package:
            fixture.write_bytes(package.read(fixture.name))
        commands = [[sys.executable, "-I", str(fixture), role, "conforming", str(golden), str(root / "audit")]
                    for role in ("adapter", "observer")]
        scenario = root / "ledger-control"
        scenario.mkdir()
        with patch.object(EffectSink, "invoke", store_inconsistent_id):
            try:
                driver["qualify"](scenario, raw, comparator, *commands, [])
            except ValueError as error:
                if str(error) != "independent_ledger_mismatch":
                    raise
            else:
                raise AssertionError("qualification accepted an inconsistent ledger execution ID")
        print(json.dumps(dict(control="ledger-id", expected_refusal="independent_ledger_mismatch")))


if __name__ == "__main__":
    if sys.argv[1] == "child":
        child(*sys.argv[2:])
    else:
        check(Path(sys.argv[1]).resolve(), sys.argv[2])
