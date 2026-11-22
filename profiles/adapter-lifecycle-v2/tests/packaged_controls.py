"""Exercise the verifier in the pinned zipapp using controlled positive and negative evidence."""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import subprocess
import sys
from tempfile import TemporaryDirectory
from unittest.mock import patch
from zipfile import ZipFile


def expect_error(group, name, code, callback):
    try:
        callback()
    except ValueError as error:
        if str(error) == code:
            print(json.dumps(dict(group=group, case=name, outcome="rejected", code=code)))
            return
        raise
    raise AssertionError(f"{name} accepted invalid evidence")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    parser.add_argument("pin")
    args = parser.parse_args()
    run_controls(args.archive.resolve(), args.pin)


def run_controls(archive, pin):
    if hashlib.sha256(archive.read_bytes()).hexdigest() != pin:
        raise ValueError("test input differs from the expected kit")
    # Load the shipped verifier and its own dependencies. Source checkout files
    # are deliberately absent from the isolated interpreter's import path.
    sys.path.insert(0, str(archive))
    driver = runpy.run_path(str(archive), run_name="packaged_controls")
    raw, _ = driver["load_bundle"](archive, pin)
    binding_type = driver["Binding"]
    with TemporaryDirectory() as tmp:
        root = Path(tmp)
        target = driver["Scenario"](root, "records", [], [], raw, [])
        launch = json.loads(raw)
        bind = binding_type(launch["request_id"], "sha256:" + hashlib.sha256(raw).hexdigest(),
                            launch["attempt_id"], "task-A", "action-A", "approval-A", "abc")
        receipt = b'{"state":"succeeded"}'
        native = dict(binding=vars(bind), request_hex=raw.hex(), state="succeeded", receipt_hex=receipt.hex())
        reply = dict(**vars(bind), state="succeeded", receipt_hex=receipt.hex(), executions=1)

        def terminal(native_record, reported, **extra):
            # Only the transport is replayed. The archived runner reads the real fixture sink.
            with patch.dict(driver["Scenario"].terminal.__globals__, invoke=lambda cmd, wd, msg, sink=None:
                            native_record if sink is None else reported):
                return target.terminal("first", "succeeded", **extra)

        eid = target.sink.invoke(bind)
        assert terminal(native, reply)[0] == eid
        for key in ("request_id", "request_digest", "attempt_id"):
            different = dict(native, binding=dict(native["binding"], **{key: "other"}))
            expect_error(2, key, "persisted_request_mismatch", lambda: terminal(different, reply))
        for key in ("task_id", "action_id"):
            expect_error(5, key, "native_binding_mismatch", lambda: terminal(native, dict(reply, **{key: "wrong"})))
        expect_error(6, "exact_receipt_bytes", "native_receipt_mismatch",
                     lambda: terminal(native, dict(reply, receipt_hex=(receipt + b" ").hex())))
        expect_error(6, "missing_receipt", "native_receipt_mismatch",
                     lambda: terminal(dict(native, receipt_hex=""), reply))
        expect_error(6, "different_execution_id", "execution_identity_mismatch",
                     lambda: terminal(native, reply, original=("other", bind, receipt)))
        expect_error(6, "unknown_promoted", "terminal_state_mismatch",
                     lambda: terminal(dict(native, state="unknown"), reply))
        from dataclasses import replace
        second = replace(bind, task_id="task-B", action_id="action-B")
        other = driver["Scenario"](root, "other-records", [], [], raw, [])
        for action, count, source in [(bind, 2, target), (second, 3, other)]:
            for _ in range(count):
                source.sink.invoke(action)
        observed = target.sink.observe() + other.sink.observe()
        assert len({row.execution_id for row in observed}) == 6
        expect_error(1, "six_observed_two_reported", "effect_count_mismatch", lambda: terminal(native, reply))
        print(json.dumps(dict(independent_invocations=len(observed), reported_per_action=[1, 1],
                              execution_ids=[row.execution_id for row in observed])))

        empty = driver["Scenario"](root, "refusals", [], [], raw, [])
        for phase, expected, group in [("new-phase", "unsupported_request", 3), ("first", "unsupported_version", 3),
                                      ("replay", "incompatible_downgrade", 3), ("replay", "operator_required", 4),
                                      ("rollback", "action_invalidated", 4), ("rollback", "stale_correlation", 5)]:
            with patch.dict(driver["Scenario"].refuse.__globals__, invoke=lambda *args: {"code": "unexpected", "executions": 0}):
                expect_error(group, phase + "_" + expected, "refusal_mismatch",
                             lambda: empty.refuse("control", phase, expected, 0))
        def changed(*args):
            empty.sink.invoke(bind)
            return {"code": "action_invalidated", "executions": 0}
        with patch.dict(driver["Scenario"].refuse.__globals__, invoke=changed):
            expect_error(4, "cancelled_reopen_with_effect", "refusal_changed_effects",
                         lambda: empty.refuse("reopen", "rollback", "action_invalidated", 0))

        with ZipFile(archive) as correct:
            for case, expected in [("member_removed", "archive_members"), ("wrong_manifest", "wrong_manifest"),
                                   ("member_changed", "member_digest_mismatch")]:
                altered = root / (case + ".pyz")
                with ZipFile(altered, "w") as out:
                    for entry in correct.namelist():
                        data = correct.read(entry)
                        if case == "member_removed" and entry == "request.json":
                            continue
                        if case == "wrong_manifest" and entry == "manifest.json":
                            data = json.dumps(dict(json.loads(data), supported_readers=[1])).encode()
                        if case == "member_changed" and entry == "request.json":
                            data += b" "
                        out.writestr(entry, data)
                expect_error(6, case, expected, lambda: driver["load_bundle"](altered, hashlib.sha256(altered.read_bytes()).hexdigest()))
        child = subprocess.run([sys.executable, "-I", str(archive), "--sha256", "0" * 64, "--observer", "[]", "--", "/unused"],
                               cwd=root, capture_output=True, check=False)
        if child.returncode != 1 or json.loads(child.stdout) != {"result":"failed", "code":"archive_digest_mismatch", "cases":[]}:
            raise AssertionError("wrong pin failed to refuse before dispatch")
        print(json.dumps(dict(group=6, case="wrong_pin", outcome="rejected", code="archive_digest_mismatch")))


if __name__ == "__main__":
    main()
