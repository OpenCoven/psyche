"""Executable design vectors for #18. Run with Python 3.9+ and the standard library."""

from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from datetime import datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
NOW = datetime(2026, 11, 20, 8, 1, tzinfo=timezone.utc)
PROFILE = "draft.psyche.automation-invocation.fixture.1"


class Denied(Exception):
    pass


class KeyConflict(Exception):
    pass


def load(name):
    return json.loads((HERE / name).read_bytes())


def digest(raw):
    return sha256(raw).hexdigest()


def timestamp(value):
    return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def initialize(path):
    with closing(sqlite3.connect(path)) as db:
        db.execute("""CREATE TABLE executions (
            id INTEGER PRIMARY KEY, producer TEXT NOT NULL, invocation_key TEXT NOT NULL,
            run_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
            request BLOB NOT NULL, result BLOB NOT NULL, UNIQUE(producer, invocation_key),
            UNIQUE(producer, run_id, attempt_id)
        )""")


def lookup(path, producer, key):
    with closing(sqlite3.connect(path)) as db:
        row = db.execute("SELECT result FROM executions WHERE producer=? AND invocation_key=?",
                         (producer, key)).fetchone()
    return json.loads(row[0]) if row else None


def adopt(path, raw, verified, now=NOW, lose_reply=False):
    # verified models independently supplied producer records. There is no network
    # endpoint, credential verifier, schedule, worker or dispatch hook in this runner.
    request = json.loads(raw)
    if (request["profile"] != PROFILE or request["producer"] != verified["producer"]
            or request["invocationKey"] != verified["invocationKey"]
            or request["source"] != verified["source"] or request["authority"] != verified["authority"]
            or verified["revoked"] or now >= timestamp(verified["validUntil"])
            or now >= timestamp(request["deadline"])
            or timestamp(request["deadline"]) > timestamp(verified["validUntil"])
            or not set(request["capabilities"]).issubset(verified["granted"])
            or request["payloadSha256"] != verified["payloadSha256"]
            or digest(request["brief"].encode()) != request["payloadSha256"]):
        raise Denied
    key = (verified["producer"], request["invocationKey"])
    with closing(sqlite3.connect(path)) as db, db:
        db.execute("BEGIN IMMEDIATE")
        row = db.execute("SELECT request, result FROM executions WHERE producer=? AND invocation_key=?", key).fetchone()
        if row:
            if row[0] != raw:
                raise KeyConflict
            return json.loads(row[1])
        source = request["source"]
        prior = db.execute(
            "SELECT invocation_key FROM executions WHERE producer=? AND run_id=? AND attempt_id=?",
            (verified["producer"], source["runId"], source["attemptId"]),
        ).fetchone()
        if prior:
            raise KeyConflict
        inserted = db.execute(
            "INSERT INTO executions(producer,invocation_key,run_id,attempt_id,request,result) VALUES(?,?,?,?,?,?)",
            (*key, source["runId"], source["attemptId"], raw, b""),
        )
        result = {
            "profile": "draft.psyche.automation-adoption.fixture.1",
            "producer": key[0], "invocationKey": key[1], "executionId": f"fixture-execution-{inserted.lastrowid}",
            "occurrenceId": source["occurrenceId"], "runId": source["runId"],
            "attemptId": source["attemptId"], "requestSha256": digest(raw), "state": "adopted",
        }
        db.execute("UPDATE executions SET result=? WHERE id=?",
                   (json.dumps(result).encode(), inserted.lastrowid))
    if lose_reply:
        raise TimeoutError
    return result


class Vectors(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.path = Path(temp.name) / "journal"
        initialize(self.path)
        self.wire = (HERE / "request.json").read_bytes()
        self.expected = load("result.json")
        self.verified = load("verified-input.json")

    def adoption(self, raw=None, **kwargs):
        return adopt(self.path, self.wire if raw is None else raw, self.verified, **kwargs)

    def count(self):
        with closing(sqlite3.connect(self.path)) as db:
            return db.execute("SELECT count(*) FROM executions").fetchone()[0]

    def test_exact_concurrent_redelivery(self):
        with ThreadPoolExecutor(max_workers=4) as workers:
            results = list(workers.map(lambda _: self.adoption(), range(8)))
        self.assertEqual(results, [self.expected] * 8)
        self.assertEqual(self.count(), 1)

    def test_conflict_preserves_execution_and_result(self):
        self.adoption()
        changed = json.loads(self.wire)
        changed["capabilities"] = []
        for raw in (self.wire + b" ", json.dumps(changed).encode()):
            with self.assertRaises(KeyConflict):
                self.adoption(raw)
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-001"), self.expected)
        self.assertEqual(self.count(), 1)

    def test_one_adopted_attempt_cannot_fork_by_changing_keys(self):
        self.adoption()
        other = json.loads(self.wire)
        other["invocationKey"] = "psyche-adoption-002"
        raw = json.dumps(other).encode()
        with self.assertRaises(Denied):
            self.adoption(raw)
        # Even an inconsistent issuer replay cannot fork a retained attempt.
        self.verified["invocationKey"] = other["invocationKey"]
        with self.assertRaises(KeyConflict):
            self.adoption(raw)
        self.assertIsNone(lookup(self.path, "coven:fixture", other["invocationKey"]))
        self.assertEqual(self.count(), 1)

    def test_timeout_after_commit_uses_lookup_on_new_connection(self):
        with self.assertRaises(TimeoutError):
            self.adoption(lose_reply=True)
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-001"), self.expected)
        self.assertIsNone(lookup(self.path, "other:fixture", "psyche-adoption-001"))
        self.assertEqual(self.count(), 1)

    def test_authority_and_integrity_denials_precede_adoption(self):
        vectors = [
            ("authority", "principalRef", "other"), ("authority", "familiarBindingDigest", "stale"),
            ("authority", "decisionDigest", "stale"), ("authority", "projectRef", "other"),
            ("authority", "scope", {"resource": "artifact:other", "operation": "write"}),
            ("source", "occurrenceFenceGeneration", 2), ("source", "attemptId", "other"),
            (None, "capabilities", ["artifact.read", "artifact.write"]),
            (None, "brief", "uncommitted change"), (None, "payloadSha256", "changed"), (None, "profile", "future"),
            (None, "deadline", "2026-11-20T08:06:00Z"),
        ]
        for section, field, value in vectors:
            with self.subTest(field=field):
                data = json.loads(self.wire)
                (data[section] if section else data)[field] = value
                with self.assertRaises(Denied):
                    self.adoption(json.dumps(data).encode())
        with self.assertRaises(Denied):
            self.adoption(now=timestamp(self.verified["validUntil"]))
        self.verified["revoked"] = True
        with self.assertRaises(Denied):
            self.adoption()
        self.assertEqual(self.count(), 0)


if __name__ == "__main__":
    unittest.main()
