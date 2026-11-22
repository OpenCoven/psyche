"""Consumer-owned synthetic effect and exact-outcome replay across process restarts."""

import hashlib
import json
import sqlite3
from pathlib import Path

action = json.loads(Path("action.json").read_bytes())
request = Path("request.json").read_bytes()
request_id = json.loads(request)["request_id"]
request_digest = "sha256:" + hashlib.sha256(request).hexdigest()
binding = (action["taskId"], action["actionId"], action["idempotencyKey"], request_id, request_digest)
db = sqlite3.connect("fixture.sqlite")
with db:
    db.execute("BEGIN IMMEDIATE")
    db.execute("CREATE TABLE IF NOT EXISTS effects (id INTEGER PRIMARY KEY, action TEXT NOT NULL)")
    db.execute("CREATE TABLE IF NOT EXISTS outcomes (task TEXT, action TEXT, key TEXT PRIMARY KEY, "
               "request_id TEXT, digest TEXT, receipt BLOB NOT NULL)")
    row = db.execute("SELECT * FROM outcomes WHERE key = ?", (action["idempotencyKey"],)).fetchone()
    if row is None:
        # In this fixture the effect itself is an append. Count actual calls independently of receipts.
        seq = db.execute("INSERT INTO effects (action) VALUES (?)", (action["actionId"],)).lastrowid
        receipt = json.dumps({"taskId": action["taskId"], "actionId": action["actionId"],
                              "effectSequence": seq, "status": action["outcome"]},
                             sort_keys=True, separators=(",", ":")).encode()
        db.execute("INSERT INTO outcomes VALUES (?, ?, ?, ?, ?, ?)", (*binding, receipt))
    else:
        if row[:5] != binding:
            raise ValueError("stale consumer binding")
        receipt = row[5]
    executions = db.execute("SELECT count(*) FROM effects").fetchone()[0]
db.close()
print(json.dumps({"schema_version": "psyche.experimental.receipt-replay.v1", "request_id": request_id,
                  "request_digest": request_digest, "receipt_hex": receipt.hex(), "executions": executions}))
