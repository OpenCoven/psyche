"""Exercise the synthetic consumer's witness protocol across fresh processes."""

import hashlib
import json
import shutil
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory

SUBMIT = Path(__file__).resolve().with_name("submit.py")
NONCE = "0123456789abcdef" * 2
DIGEST = hashlib.sha256(NONCE.encode("ascii")).hexdigest()


def line(value):
    return json.dumps(value).encode("ascii") + b"\n"


def snapshot(count, digest):
    return line({"executions": count, "nonce_digest": digest})


def acknowledgement(count, nonce):
    return line({"executions": count, "nonce": nonce})


def fixture(root, name):
    directory = root / name
    directory.mkdir()
    for path in ("action.json", "request.json"):
        shutil.copyfile(path, directory / path)
    return directory


def submit(directory, state, reply=None, *, denial=None):
    # An acknowledgement is available only after the child requests an effect.
    with subprocess.Popen([sys.executable, "-I", str(SUBMIT)], cwd=directory, stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
        try:
            if reply is None:
                output, errors = child.communicate(state, timeout=20)
            else:
                child.stdin.write(state)
                child.stdin.flush()
                if json.loads(child.stdout.readline()) != {"execute": "fixture-action"}:
                    raise ValueError("consumer did not request the fixture effect")
                output, errors = child.communicate(reply, timeout=20)
        finally:
            if child.poll() is None:
                child.kill()
    if errors or len(output.splitlines()) != 1:
        raise ValueError("unexpected witness protocol transcript")
    message = json.loads(output)
    if denial is None:
        if child.returncode != 0 or message.get("executions") != 1:
            raise ValueError("consumer did not return the first outcome")
    elif child.returncode != 1 or message != {"result": "denied", "code": denial}:
        raise ValueError(f"expected {denial}, received {message} (exit {child.returncode})")
    return message


def check_snapshot(root):
    directory = fixture(root, "invalid-snapshot")
    empty = snapshot(0, None)
    for data in (b"", b"null\n", b"{\n", b"\xff\n", empty.rstrip(b"\n"), empty[:-1].ljust(1024) + b"\n",
                 b'{"executions":0,"executions":0,"nonce_digest":null}\n', snapshot(True, None), snapshot(-1, None),
                 snapshot(0, DIGEST), snapshot(1, None), snapshot(1, "0" * 63), snapshot(1, "G" * 64),
                 line({"executions": 0}), line({"executions": 0, "nonce_digest": None, "extra": True})):
        submit(directory, data, denial="invalid_witness")
    if (directory / "fixture.sqlite").exists():
        raise ValueError("invalid initial witness created a consumer store")


def check_acknowledgement(root):
    replies = [
        (b"", "invalid_witness"), (b"{\n", "invalid_witness"), (b"[]\n", "invalid_witness"),
        (acknowledgement(True, NONCE), "invalid_witness"), (acknowledgement(-1, NONCE), "invalid_witness"),
        (acknowledgement(1, False), "invalid_witness"), (acknowledgement(1, NONCE[:-1]), "invalid_witness"),
        (acknowledgement(1, NONCE + "0"), "invalid_witness"), (acknowledgement(1, NONCE.upper()), "invalid_witness"),
        (line({"executions": 1, "nonce_digest": DIGEST}), "invalid_witness"),
        (acknowledgement(0, NONCE), "indeterminate_effect"), (acknowledgement(2, NONCE), "indeterminate_effect"),
    ]
    for index, (data, denial) in enumerate(replies):
        directory = fixture(root, f"invalid-ack-{index}")
        submit(directory, snapshot(0, None), data, denial=denial)
        with closing(sqlite3.connect((directory / "fixture.sqlite").as_uri() + "?mode=ro", uri=True)) as db:
            if db.execute("SELECT count(*) FROM intents").fetchone() != (1,):
                raise ValueError("missing committed intent after acknowledgement refusal")
            if db.execute("SELECT count(*) FROM outcomes").fetchone() != (0,):
                raise ValueError("refused acknowledgement produced a terminal outcome")


def check_replay(root):
    directory = fixture(root, "replay")
    # The frame limit includes the newline on both protocol messages.
    state = snapshot(0, None)[:-1].ljust(1023) + b"\n"
    reply = acknowledgement(1, NONCE)[:-1].ljust(1023) + b"\n"
    first = submit(directory, state, reply)
    if submit(directory, snapshot(1, DIGEST)) != first:
        raise ValueError("first and replay observations differ")
    for data in (snapshot(0, None), snapshot(2, DIGEST), snapshot(1, "0" * 64)):
        submit(directory, data, denial="indeterminate_effect")

    action_file = directory / "action.json"
    original = action_file.read_bytes()
    action_file.write_bytes(line({**json.loads(original), "taskId": "other-task"}))
    submit(directory, snapshot(1, DIGEST), denial="stale_consumer_binding")
    action_file.write_bytes(original)
    if submit(directory, snapshot(1, DIGEST)) != first:
        raise ValueError("a refused replay changed the stored outcome")

    receipt = json.loads(bytes.fromhex(first["receipt_hex"]))
    if receipt["effectNonce"] != NONCE:
        raise ValueError("saved receipt has the wrong execution nonce")
    for bad in (b"{}", b"{", "not a blob", line({**receipt, "effectNonce": False}),
                line({**receipt, "effectNonce": "5" * 31}), line({**receipt, "effectNonce": "f" * 32})):
        with closing(sqlite3.connect(directory / "fixture.sqlite")) as db, db:
            if db.execute("UPDATE outcomes SET receipt = ?", (bad,)).rowcount != 1:
                raise ValueError("missing persisted outcome")
        submit(directory, snapshot(1, DIGEST), denial="indeterminate_effect")


def run_controls():
    with TemporaryDirectory() as temporary:
        root = Path(temporary).resolve()
        check_snapshot(root)
        check_acknowledgement(root)
        check_replay(root)
    print(json.dumps({"control": "consumer-witness-protocol", "result": "passed"}))
