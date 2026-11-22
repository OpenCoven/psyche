"""Qualify an experimental v2 adapter against an independent native reader and a controlled effect sink."""
import argparse
from contextlib import redirect_stdout
import io
from dataclasses import fields
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import BadZipFile, ZipFile

from effect_sink import Binding, Witness
from evidence_checks import Observation, verify_refusal, verify_terminal
from runtime import invoke, strict_object

PROFILE = "psyche.experimental.adapter-lifecycle.v2"
MODULES = {"__main__.py", "effect_sink.py", "evidence_checks.py", "runtime.py", "packaged_controls.py", "request.json", "receipt-replay.pyz"}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(value, code):
    if not value:
        raise ValueError(code)


def load_bundle(path, pin):
    require(sha(path.read_bytes()) == pin, "archive_digest_mismatch")
    with ZipFile(path) as bundle:
        names = bundle.namelist()
        require(len(names) == len(MODULES) + 1 and set(names) == MODULES | {"manifest.json"}, "archive_members")
        manifest = json.loads(bundle.read("manifest.json"), object_pairs_hook=strict_object)
        require(manifest["profile"] == PROFILE and manifest["supported_readers"] == [2], "wrong_manifest")
        require(manifest["files"] == {name: sha(bundle.read(name)) for name in MODULES}, "member_digest_mismatch")
        return bundle.read("request.json"), bundle.read("receipt-replay.pyz")


def snapshot(workspace):
    return {p.relative_to(workspace): sha(p.read_bytes()) for p in workspace.rglob("*") if p.is_file()}


class Scenario:
    def __init__(self, root, name, adapter, observer, raw, rows):
        self.name = name
        self.workspace = root / name
        self.workspace.mkdir()
        self.sink = Witness(root / (name + ".sqlite"))
        self.adapter = adapter
        self.observer = observer
        self.raw = raw
        self.launch = json.loads(raw)
        self.rows = rows

    def message(self, phase, **extra):
        envelope = dict(profile=PROFILE, reader_version=2, phase=phase)
        if phase != "rollback":
            envelope.update(request_hex=self.raw.hex(), request_id=self.launch["request_id"])
        return {**envelope, **extra}

    def refuse(self, label, phase, expected, count, readonly=True, **extra):
        before = self.sink.observe()
        require(len(before) == count, "unexpected_initial_effect_count")
        files = snapshot(self.workspace)
        reply = invoke(self.adapter, self.workspace, self.message(phase, **extra), self.sink)
        verify_refusal(expected, reply.get("code"), before, self.sink.observe())
        require(set(reply) == {"code", "executions"} and type(reply["executions"]) is int and reply["executions"] == count, "invalid_refusal")
        if readonly:
            require(snapshot(self.workspace) == files, "refusal_changed_workspace")
        self.rows.append(dict(case=self.name, probe=label, phase=phase, code=expected, executions=count))

    def read_native(self):
        before = snapshot(self.workspace)
        native = invoke(self.observer, self.workspace, {})
        require(snapshot(self.workspace) == before, "observer_changed_workspace")
        binding = Binding(**native["binding"])
        require(all(isinstance(getattr(binding, f.name), str) and 0 < len(getattr(binding, f.name)) <= 256
                    for f in fields(binding)), "invalid_native_binding")
        require(bytes.fromhex(native["request_hex"]) == self.raw and binding.request_id == self.launch["request_id"]
                and binding.request_digest == "sha256:" + sha(self.raw)
                and binding.attempt_id == self.launch["attempt_id"], "persisted_request_mismatch")
        return binding, native

    def terminal(self, phase, expected, original=None, **extra):
        reply = invoke(self.adapter, self.workspace, self.message(phase, **extra), self.sink)
        binding, native = self.read_native()
        keys = {f.name for f in fields(Binding)}
        require(set(reply) == keys | {"state", "receipt_hex", "executions"}, "invalid_fields")
        reported = Observation(Binding(**{k: reply[k] for k in keys}), reply["state"],
                               bytes.fromhex(reply["receipt_hex"]), reply["executions"])
        effect = verify_terminal(binding, native["state"], bytes.fromhex(native["receipt_hex"]), reported,
                                 self.sink.observe(), expected, original[0] if original else None)
        record = (effect, binding, reported.receipt)
        require(original is None or original == record, "reopened_observation_changed")
        self.rows.append(dict(case=self.name, phase=phase, execution_id=effect, task_id=binding.task_id,
                              action_id=binding.action_id, state=reported.state, receipt_sha256=sha(reported.receipt),
                              executions=reported.executions))
        return record


def qualify(root, raw, comparator, adapter, observer, rows):
    def scenario(name):
        return Scenario(root, name, adapter, observer, raw, rows)
    invalid = scenario("unknown_input")
    for key, value, code in [("profile", PROFILE + ".next", "unsupported_version"),
                             ("reader_version", 0, "incompatible_downgrade"),
                             ("reader_version", 3, "incompatible_downgrade"),
                             ("fault", "unrecognized", "unsupported_request")]:
        invalid.refuse(key + "_" + str(value), "first", code, 0, **{key: value})
    invalid.refuse("unknown_phase", "unrecognized", "unsupported_request", 0)
    for fault, code in [("agent_approval", "operator_required"), ("cancel", "action_invalidated")]:
        denied = scenario(fault)
        denied.refuse("initial", "first", code, 0, readonly=False, fault=fault)
        binding, first_native = denied.read_native()
        require(first_native.get("refusal") == code, "missing_native_refusal")
        ids = dict(task_id=binding.task_id, action_id=binding.action_id)
        for phase in ("replay", "rollback"):
            denied.refuse("reopen", phase, code, 0, readonly=phase == "rollback", **ids)
            reopened_binding, reopened = denied.read_native()
            require(reopened_binding == binding and reopened.get("refusal") == code and
                    reopened.get("state") == "failed", "refused_native_state_changed")
            if phase == "rollback":
                require(reopened == first_native, "refused_native_state_changed")
            first_native = reopened
    request_path = root / "request.json"
    request_path.write_bytes(raw)
    for name, state in [("success", "succeeded"), ("lost_ack", "unknown")]:
        target = scenario(name)
        original = target.terminal("first", state, **({"fault": "lost_ack"} if state == "unknown" else {}))
        _, binding, receipt = original
        ids = dict(task_id=binding.task_id, action_id=binding.action_id)
        if name == "success":
            for field in ("request_id", "payload_digest"):
                mutated = dict(target.launch)
                mutated[field] = ("req_01J00000000000000000000001" if field == "request_id" else "sha256:" + "0" * 64)
                canonical = json.dumps(mutated, sort_keys=True, separators=(",", ":")).encode()
                target.refuse(field, "replay", "stale_correlation", 1, request_id=mutated["request_id"], request_hex=canonical.hex())
            for field in ids:
                target.refuse("wrong_" + field, "rollback", "stale_correlation", 1, **{**ids, field: "other-native-id"})
            target.refuse("downgrade", "replay", "incompatible_downgrade", 1, reader_version=1)
        first = dict(schema_version="psyche.experimental.receipt-replay.v1", request_id=binding.request_id,
                     request_digest=binding.request_digest, receipt_hex=receipt.hex(), executions=1)
        first_file = root / "first.json"
        first_file.write_text(json.dumps(first))
        for phase in ("replay", "rollback"):
            _, _, observed = target.terminal(phase, state, original, **ids)
            current = root / "current.json"
            current.write_text(json.dumps(dict(first, receipt_hex=observed.hex())))
            # Extract from immutable verified bytes immediately before each invocation.
            checker = root / "checker.pyz"
            checker.write_bytes(comparator)
            verified = invoke([sys.executable, "-I", str(checker), str(request_path), str(first_file), str(current)], root, {})
            require(verified.get("result") == "passed" and verified.get("additional_executions") == 0, "comparator_failed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--controls", action="store_true", help="also exercise the packaged negative evidence corpus")
    parser.add_argument("--observer", required=True, help="JSON array containing the independent native reader command")
    parser.add_argument("adapter", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    rows = []
    controls = []
    try:
        raw, comparator = load_bundle(Path(sys.argv[0]), args.sha256)
        if args.controls:
            from packaged_controls import run_controls
            output = io.StringIO()
            with redirect_stdout(output):
                run_controls(Path(sys.argv[0]).resolve(), args.sha256)
            controls = [json.loads(line) for line in output.getvalue().splitlines()]
        observer = json.loads(args.observer)
        adapter = args.adapter[1:] if args.adapter[:1] == ["--"] else args.adapter
        for cmd in (adapter, observer):
            require(isinstance(cmd, list) and cmd and all(isinstance(v, str) for v in cmd) and Path(cmd[0]).is_absolute(), "invalid_command")
        with TemporaryDirectory() as folder:
            qualify(Path(folder), raw, comparator, adapter, observer, rows)
    except (ValueError, KeyError, TypeError, OSError, sqlite3.Error, BadZipFile, subprocess.TimeoutExpired) as exc:
        code = str(exc) if type(exc) is ValueError and str(exc).replace("_", "").isalnum() else "invalid_run"
        print(json.dumps(dict(result="failed", code=code, cases=rows)))
        return 1
    print(json.dumps(dict(result="passed", profile=PROFILE, sha256=args.sha256, cases=rows, controls=controls), sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
