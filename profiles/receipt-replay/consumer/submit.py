"""Consumer-owned intent and receipt for one synthetic action. The harness owns its effect."""

import hashlib
import json
import re
import sqlite3
import sys
from pathlib import Path


class Refusal(Exception):
    pass


def unique_fields(pairs):
    fields = dict(pairs)
    if len(fields) != len(pairs):
        raise Refusal("invalid_witness")
    return fields


def read_witness(nonce_field):
    # A witness frame is one UTF-8 JSON line including its terminating LF.
    limit = 1024
    frame = sys.stdin.buffer.readline(limit + 1)
    if len(frame) > limit or not frame.endswith(b"\n"):
        raise Refusal("invalid_witness")
    try:
        value = json.loads(frame.decode("utf-8"), object_pairs_hook=unique_fields)
    except (ValueError, RecursionError):
        raise Refusal("invalid_witness") from None
    if not isinstance(value, dict) or set(value) != {"executions", nonce_field}:
        raise Refusal("invalid_witness")
    executions = value["executions"]
    if type(executions) is not int or executions < 0:
        raise Refusal("invalid_witness")
    nonce = value[nonce_field]
    if nonce_field == "nonce_digest" and executions == 0:
        if nonce is not None:
            raise Refusal("invalid_witness")
    else:
        pattern = r"[0-9a-f]{64}" if nonce_field == "nonce_digest" else r"[0-9a-f]{32}"
        if not isinstance(nonce, str) or re.fullmatch(pattern, nonce) is None:
            raise Refusal("invalid_witness")
    return value


def stored_nonce_digest(receipt):
    if not isinstance(receipt, bytes):
        raise Refusal("indeterminate_effect")
    try:
        saved = json.loads(receipt)
    except (ValueError, RecursionError):
        raise Refusal("indeterminate_effect") from None
    nonce = saved.get("effectNonce") if isinstance(saved, dict) else None
    if not isinstance(nonce, str) or re.fullmatch(r"[0-9a-f]{32}", nonce) is None:
        raise Refusal("indeterminate_effect")
    return hashlib.sha256(nonce.encode("utf-8")).hexdigest()


def submit():
    action = json.loads(Path("action.json").read_bytes())
    request = Path("request.json").read_bytes()
    request_id = json.loads(request)["request_id"]
    digest = "sha256:" + hashlib.sha256(request).hexdigest()
    binding = (action["taskId"], action["actionId"], action["idempotencyKey"], request_id, digest)
    key = action["idempotencyKey"]
    witness = read_witness("nonce_digest")
    store = Path("fixture.sqlite")
    if not store.exists() and witness["executions"]:
        raise Refusal("indeterminate_effect")

    db = sqlite3.connect(store)
    try:
        with db:
            db.execute("BEGIN IMMEDIATE")
            db.execute("CREATE TABLE IF NOT EXISTS intents (task TEXT, action TEXT, key TEXT PRIMARY KEY, "
                       "request_id TEXT, digest TEXT)")
            db.execute("CREATE TABLE IF NOT EXISTS outcomes (key TEXT PRIMARY KEY, receipt BLOB NOT NULL)")
            intent = db.execute("SELECT * FROM intents WHERE key = ?", (key,)).fetchone()
            outcome = db.execute("SELECT receipt FROM outcomes WHERE key = ?", (key,)).fetchone()
            if intent is None:
                if outcome is not None or witness["executions"]:
                    raise Refusal("indeterminate_effect")
                db.execute("INSERT INTO intents VALUES (?, ?, ?, ?, ?)", binding)
            elif intent != binding:
                raise Refusal("stale_consumer_binding")
            elif outcome is None or witness["executions"] != 1:
                raise Refusal("indeterminate_effect")

        if intent is None:
            # The intent survives a crash on either side of this external append.
            print(json.dumps({"execute": action["actionId"]}), flush=True)
            witness = read_witness("nonce")
            if witness["executions"] != 1:
                raise Refusal("indeterminate_effect")
            receipt = json.dumps({"taskId": action["taskId"], "actionId": action["actionId"],
                                  "effectNonce": witness["nonce"], "effectSequence": witness["executions"],
                                  "status": action["outcome"]},
                                 sort_keys=True, separators=(",", ":")).encode()
            with db:
                db.execute("INSERT INTO outcomes VALUES (?, ?)", (key, receipt))
        else:
            receipt = outcome[0]
            if stored_nonce_digest(receipt) != witness["nonce_digest"]:
                raise Refusal("indeterminate_effect")
    finally:
        db.close()
    return {"schema_version": "psyche.experimental.receipt-replay.v1", "request_id": request_id,
            "request_digest": digest, "receipt_hex": receipt.hex(), "executions": witness["executions"]}


if __name__ == "__main__":
    try:
        print(json.dumps(submit()))
    except Refusal as refusal:
        print(json.dumps({"result": "denied", "code": str(refusal)}))
        sys.exit(1)
