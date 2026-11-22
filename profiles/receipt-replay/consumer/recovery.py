"""Qualify first delivery, restart and crash recovery against a harness-owned effect ledger."""

import hashlib
import json
import secrets
import shutil
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Timer


SUBMIT = Path(__file__).resolve().with_name("submit.py")
INDETERMINATE = {"result": "denied", "code": "indeterminate_effect"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def witness(effects):
    # This list exists only in the parent. A restarted fixture can check its
    # stored nonce against the digest but cannot reconstruct it from the snapshot.
    digest = hashlib.sha256(effects[-1].encode("ascii")).hexdigest() if effects else None
    return {"executions": len(effects), "nonce_digest": digest}


def deliver(directory, effects, stop=None, *, allow_effect=True):
    # The consumer receives observations and may request an append. Only this
    # parent process records an effect and issues its fresh execution nonce.
    with subprocess.Popen([sys.executable, "-I", str(SUBMIT)], cwd=directory,
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as child:
        def reply(value):
            child.stdin.write(json.dumps(value) + "\n")
            child.stdin.flush()

        deadline = Timer(15, child.kill)
        deadline.start()
        try:
            reply(witness(effects))
            result = json.loads(child.stdout.readline())
            if result == {"execute": "fixture-action"}:
                require(allow_effect, "unexpected effect request during read-only recovery")
                if stop != "before_effect":
                    effects.append(secrets.token_hex(16))
                if stop:
                    # Kill while the child is awaiting the effect acknowledgement,
                    # before any terminal receipt can have been saved.
                    child.kill()
                    require(child.wait() < 0, "fault injection did not stop the consumer")
                    return None
                reply({"executions": len(effects), "nonce": effects[-1]})
                result = json.loads(child.stdout.readline())
            child.stdin.close()
            remainder = child.stdout.read()
            code = child.wait()
            require(not remainder and stop is None, "unexpected consumer output or missing fault boundary")
            require(code == (1 if result.get("result") == "denied" else 0), "unexpected consumer exit")
            return result
        finally:
            deadline.cancel()
            if child.poll() is None:
                child.kill()


def qualify_recovery_controls(harness, first_nonce):
    oracle = harness / "read-only-control"
    oracle.mkdir()
    for name in ("action.json", "request.json"):
        shutil.copyfile(name, oracle / name)
    effects = []
    try:
        deliver(oracle, effects, allow_effect=False)
    except ValueError as refusal:
        require(str(refusal) == "unexpected effect request during read-only recovery", str(refusal))
    else:
        raise ValueError("read-only control accepted an effect request")
    require(not effects, "refused request changed the parent effect count")

    # Each case starts with a different store, effect ledger and process.
    for case in ("lost_store", "lost_receipt", "before_effect", "after_effect"):
        directory = harness / case
        directory.mkdir()
        for name in ("action.json", "request.json"):
            shutil.copyfile(name, directory / name)
        store = directory / "fixture.sqlite"
        effects = []
        if case in ("before_effect", "after_effect"):
            deliver(directory, effects, stop=case)
        else:
            first = deliver(directory, effects)
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
        before = witness(effects)
        expected = 0 if case == "before_effect" else 1
        require(before["executions"] == expected, "incorrect effect count at fault boundary")
        if expected:
            require(effects[-1] != first_nonce, "re-execution reused the receipt nonce")
        for _ in range(2):
            require(deliver(directory, effects, allow_effect=False) == INDETERMINATE, "unsafe recovery from " + case)
            require(witness(effects) == before, "recovery repeated an effect in " + case)


def exercise_recovery():
    """Write first.json/replay.json and return durable receipt bytes, total effects and restart delta."""
    store = Path("fixture.sqlite")
    require(not store.exists(), "consumer must start with fresh storage")
    with TemporaryDirectory() as scratch:
        harness = Path(scratch)
        effects = []
        first = deliver(Path.cwd(), effects)
        after_first = witness(effects)
        replay = deliver(Path.cwd(), effects, allow_effect=False)
        after_replay = witness(effects)
        require(after_first["executions"] == 1 and after_first == after_replay, "unexpected effect history")
        require(first.get("executions") == replay.get("executions") == 1, "consumer failed intact-store replay")
        for name, value in (("first.json", first), ("replay.json", replay)):
            with Path(name).open("x") as output:
                json.dump(value, output)
        with closing(sqlite3.connect("file:fixture.sqlite?mode=ro", uri=True)) as db:
            outcomes = db.execute("SELECT receipt FROM outcomes").fetchall()
        require(len(outcomes) == 1, "expected one durable outcome")
        receipt = outcomes[0][0]
        require(json.loads(receipt)["effectNonce"] == effects[0], "receipt does not bind effect")
        require(first["receipt_hex"] == replay["receipt_hex"] == receipt.hex(), "stored receipt differs")
        qualify_recovery_controls(harness, effects[0])
    return receipt, after_replay["executions"], after_replay["executions"] - after_first["executions"]
