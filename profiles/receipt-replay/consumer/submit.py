"""Consumer-owned intent and receipt for one synthetic action. The harness owns its effect."""

import hashlib
import json
import sqlite3
import sys
from pathlib import Path


class Refusal(Exception):
    pass


def submit():
    action = json.loads(Path("action.json").read_bytes())
    request = Path("request.json").read_bytes()
    request_id = json.loads(request)["request_id"]
    digest = "sha256:" + hashlib.sha256(request).hexdigest()
    binding = (action["taskId"], action["actionId"], action["idempotencyKey"], request_id, digest)
    key = action["idempotencyKey"]
    witness = json.loads(input())
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
            witness = json.loads(input())
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
            if json.loads(receipt)["effectNonce"] != witness["nonce"]:
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
