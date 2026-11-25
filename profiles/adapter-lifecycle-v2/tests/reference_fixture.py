"""Disposable reference fixture for exercising the packaged v2 CLI in fresh processes."""
import hashlib
import json
import os
from pathlib import Path
import socket
import sys

PROFILE = "psyche.experimental.adapter-lifecycle.v2"
record = Path("original.json")
effects = Path("executions.jsonl")


def rows():
    return [json.loads(line) for line in effects.read_text().splitlines()] if effects.exists() else []


def submit(binding, audit):
    with socket.socket(fileno=os.dup(int(os.environ["PSYCHE_TEST_EFFECT_FD"]))) as channel:
        channel.sendall(json.dumps(binding).encode() + b"\n")
        execution_id = channel.makefile("rb").readline().decode().strip()
    event = dict(execution_id=execution_id, binding=binding)
    for path in (effects, audit):
        with path.open("a") as stream:
            print(json.dumps(event), file=stream)


def adapt(msg, raw, vector, audit):
    case = Path.cwd().name
    phase = msg["phase"]
    saved = json.loads(record.read_bytes()) if record.exists() else None
    code = None
    probe = None
    if msg["profile"] != PROFILE:
        probe, code = "profile", "unsupported_version"
    elif msg["reader_version"] != 2:
        probe, code = "version", "incompatible_downgrade"
    elif phase not in ("first", "replay", "rollback") or msg.get("fault") not in (None, "agent_approval", "cancel", "lost_ack"):
        probe, code = "enum", "unsupported_request"
    elif phase != "first":
        binding = saved["binding"]
        if any(msg.get(key) != binding[key] for key in ("action_id", "task_id")):
            probe, code = "ids", "stale_correlation"
        elif phase == "replay" and (msg["request_id"] != binding["request_id"] or msg["request_hex"] != raw.hex()):
            probe = "request_id" if msg["request_id"] != binding["request_id"] else "payload_digest"
            code = "stale_correlation"
        elif saved["refusal"]:
            probe, code = "reopen", saved["refusal"]
    else:
        request = json.loads(raw)
        if msg["request_hex"] != raw.hex() or msg["request_id"] != request["request_id"]:
            raise ValueError("unexpected reference launch")
        binding = dict(request_id=request["request_id"], request_digest="sha256:" + hashlib.sha256(raw).hexdigest(),
                       attempt_id=request["attempt_id"], task_id="task-" + case, action_id="action-" + case,
                       approval_id="approval-" + case, approval_digest=hashlib.sha256(case.encode()).hexdigest())
        code = {"cancel": "action_invalidated", "agent_approval": "operator_required"}.get(msg.get("fault"))
        state = "failed" if code else "unknown" if msg.get("fault") == "lost_ack" else "succeeded"
        receipt = json.dumps(dict(state=state, action=binding["action_id"])).encode()
        saved = dict(binding=binding, request_hex=raw.hex(), refusal=code, state=state, receipt_hex=receipt.hex())
        record.write_text(json.dumps(saved))
        if not code:
            for _ in range(3 if vector == "extra-" + case else 1):
                submit(binding, audit)
    if code:
        if vector == "invalid-refusal" and code == "unsupported_version":
            return dict(code=code, executions=False)
        if vector == "changed-workspace" and code == "unsupported_version":
            Path("unexpected").touch()
        if vector == f"{case}-{phase}-{probe}":
            return dict(code="fixture_data_error", executions=len(rows()))
        return dict(code=code, executions=len(rows()))
    reply = dict(**saved["binding"], state=saved["state"], receipt_hex=saved["receipt_hex"],
                 executions=1, execution_id=rows()[0]["execution_id"])
    if case == "lost_ack" and phase == "first" and vector == "lost-state":
        reply["state"] = "indeterminate_fixture_value"
    if case == "success" and phase == "rollback" and vector == "reopen-id":
        reply["execution_id"] = "unmatched"
    if case == "success" and phase == "first" and vector == "report-count":
        reply["executions"] = 0
    if vector == "extra-" + case:
        audit.with_suffix(".reported").write_text(str(reply["executions"]))
    if vector == "reply-field":
        reply["unrecognized"] = 1
    return reply


def observe(msg, vector):
    events = rows()
    if vector == "observer-write":
        Path("unexpected").touch()
    if msg == {"operation": "effects"}:
        return dict(effects=events)
    native = json.loads(record.read_bytes())
    if Path.cwd().name == "success":
        if vector == "unknown-field":
            native["schema_version"] = 99
        if vector == "native-id":
            events[0]["execution_id"] = "unmatched"
        if vector == "request-bytes":
            native["request_hex"] = native["request_hex"].upper()
        if vector == "missing-approval":
            native["binding"]["approval_id"] = "none"
        if vector == "receipt-bytes":
            native["receipt_hex"] += "20"
    return dict(**native, effects=events)


if __name__ == "__main__":
    role, vector, golden, audit = sys.argv[1:]
    message = json.load(sys.stdin)
    if role == "observer" and vector == "nonzero-child":
        sys.exit(7)
    if role == "observer" and vector == "duplicate-key":
        print('{"effects":[],"effects":[]}')
        sys.exit(0)
    if role == "observer" and vector == "oversize":
        print(" " * 65537 + "{}")
        sys.exit(0)
    result = adapt(message, Path(golden).read_bytes(), vector, Path(audit)) if role == "adapter" else observe(message, vector)
    print(json.dumps(result))
