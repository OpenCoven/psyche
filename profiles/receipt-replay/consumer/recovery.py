"""Qualify first delivery, restart and crash recovery against a harness-owned effect ledger."""

import json
import secrets
import shutil
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory


SUBMIT = Path(__file__).resolve().with_name("submit.py")
INDETERMINATE = {"result": "denied", "code": "indeterminate_effect"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def witness(ledger):
    rows = ledger.execute("SELECT nonce FROM effects ORDER BY sequence").fetchall()
    return {"executions": len(rows), "nonce": rows[-1][0] if rows else None}


def deliver(directory, ledger, stop=None):
    # The consumer receives observations and may request an append. Only this
    # parent process records an effect and issues its fresh execution nonce.
    with subprocess.Popen([sys.executable, "-I", str(SUBMIT)], cwd=directory,
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as child:
        def reply(value):
            child.stdin.write(json.dumps(value) + "\n")
            child.stdin.flush()

        try:
            reply(witness(ledger))
            result = json.loads(child.stdout.readline())
            if result == {"execute": "fixture-action"}:
                if stop != "before_effect":
                    with ledger:
                        ledger.execute("INSERT INTO effects (nonce) VALUES (?)", (secrets.token_hex(16),))
                if stop:
                    # Kill while the child is awaiting the effect acknowledgement,
                    # before any terminal receipt can have been saved.
                    child.kill()
                    require(child.wait() < 0, "fault injection did not stop the consumer")
                    return None
                reply(witness(ledger))
                result = json.loads(child.stdout.readline())
            child.stdin.close()
            remainder = child.stdout.read()
            code = child.wait()
            require(not remainder and stop is None, "unexpected consumer output or missing fault boundary")
            require(code == (1 if result.get("result") == "denied" else 0), "unexpected consumer exit")
            return result
        finally:
            if child.poll() is None:
                child.kill()


def new_ledger(path):
    db = sqlite3.connect(path)
    with db:
        db.execute("CREATE TABLE effects (sequence INTEGER PRIMARY KEY, nonce TEXT NOT NULL UNIQUE)")
    return db


def qualify_recovery_controls(harness, first_nonce):
    # Each case starts with a different store, effect ledger and process.
    for case in ("lost_store", "lost_receipt", "before_effect", "after_effect"):
        directory = harness / case
        directory.mkdir()
        for name in ("action.json", "request.json"):
            shutil.copyfile(name, directory / name)
        store = directory / "fixture.sqlite"
        with closing(new_ledger(harness / (case + ".sqlite"))) as ledger:
            if case in ("before_effect", "after_effect"):
                deliver(directory, ledger, stop=case)
            else:
                first = deliver(directory, ledger)
                require(first.get("executions") == 1, "failed to prepare recovery control")
                if case == "lost_store":
                    store.unlink()
                else:
                    with closing(sqlite3.connect(store)) as db, db:
                        require(db.execute("DELETE FROM outcomes").rowcount == 1, "missing outcome")
            if case != "lost_store":
                with closing(sqlite3.connect(store.as_uri() + "?mode=ro", uri=True)) as db:
                    require(db.execute("SELECT count(*) FROM intents").fetchone() == (1,), "missing durable intent")
                    require(db.execute("SELECT count(*) FROM outcomes").fetchone() == (0,), "unexpected receipt")
            before = witness(ledger)
            expected = 0 if case == "before_effect" else 1
            require(before["executions"] == expected, "incorrect effect count at fault boundary")
            if expected:
                require(before["nonce"] != first_nonce, "re-execution reused the receipt nonce")
            require(deliver(directory, ledger) == INDETERMINATE, "unsafe recovery from " + case)
            require(witness(ledger) == before, "recovery repeated an effect in " + case)


def exercise_recovery():
    """Write first.json/replay.json and return durable receipt bytes, total effects and restart delta."""
    store = Path("fixture.sqlite")
    require(not store.exists(), "consumer must start with fresh storage")
    with TemporaryDirectory() as scratch:
        harness = Path(scratch)
        with closing(new_ledger(harness / "positive.sqlite")) as ledger:
            first = deliver(Path.cwd(), ledger)
            after_first = witness(ledger)
            replay = deliver(Path.cwd(), ledger)
            after_replay = witness(ledger)
            require(after_first["executions"] == 1 and after_first == after_replay, "unexpected effect history")
            require(first.get("executions") == replay.get("executions") == 1, "consumer failed intact-store replay")
            for name, value in (("first.json", first), ("replay.json", replay)):
                with Path(name).open("x") as output:
                    json.dump(value, output)
            with closing(sqlite3.connect("file:fixture.sqlite?mode=ro", uri=True)) as db:
                outcomes = db.execute("SELECT receipt FROM outcomes").fetchall()
            require(len(outcomes) == 1, "expected one durable outcome")
            receipt = outcomes[0][0]
            require(json.loads(receipt)["effectNonce"] == after_first["nonce"], "receipt does not bind effect")
            require(first["receipt_hex"] == replay["receipt_hex"] == receipt.hex(), "stored receipt differs")
        qualify_recovery_controls(harness, after_first["nonce"])
    return receipt, after_replay["executions"], after_replay["executions"] - after_first["executions"]
