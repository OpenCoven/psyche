"""Process-restart and event-replay vectors. Run with Python 3.9+."""

from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

# Keep a clean checkout when this standalone runner imports the adoption fixture.
sys.dont_write_bytecode = True
from verify import EVENT_PROFILE, HERE, adopt, digest, initialize, load, lookup


class ReplayError(Exception):
    pass


def connect(root, name):
    return closing(sqlite3.connect(root / name))


def stage(root):
    initialize(root / "psyche.db")
    verified = load("verified-input.json")
    with connect(root, "coven.db") as db, db:
        db.executescript("""
            CREATE TABLE pending (producer TEXT, key TEXT, request BLOB, response BLOB,
                                  PRIMARY KEY (producer, key));
            CREATE TABLE outbound (command TEXT);
            CREATE TABLE inbox (producer TEXT, key TEXT, execution TEXT, sequence INTEGER, body BLOB,
                                PRIMARY KEY (producer, key, execution, sequence));
        """)
        db.execute("INSERT INTO pending VALUES (?, ?, ?, NULL)",
                   (verified["producer"], verified["invocationKey"], (HERE / "request.json").read_bytes()))


def read_pending(root):
    with connect(root, "coven.db") as db:
        return db.execute("SELECT producer, key, request FROM pending").fetchone()


def recover_producer(root):
    producer, key, request = read_pending(root)
    with connect(root, "coven.db") as db, db:
        db.execute("INSERT INTO outbound VALUES ('lookup')")
    result = lookup(root / "psyche.db", producer, key)
    if result is None:
        raise ReplayError("producer lookup unresolved")
    expected_source = json.loads(request)["source"]
    if (result["producer"] != producer or result["invocationKey"] != key
            or result["requestSha256"] != digest(request)
            or any(result[k] != expected_source[k] for k in ("occurrenceId", "runId", "attemptId"))):
        raise ReplayError("producer lookup correlation mismatch")
    with connect(root, "coven.db") as db, db:
        db.execute("UPDATE pending SET response=? WHERE producer=? AND key=?",
                   (json.dumps(result).encode(), producer, key))
    return result


def read_page(root, route, after, limit):
    if type(after) is not int or after < 0 or type(limit) is not int or not 1 <= limit <= 8:
        raise ReplayError("invalid cursor or page size")
    with connect(root, "psyche.db") as db:
        db.execute("BEGIN")
        execution = db.execute("SELECT id, result FROM executions WHERE producer=? AND invocation_key=?",
                               route[:2]).fetchone()
        if not execution or json.loads(execution[1])["executionId"] != route[2]:
            raise ReplayError("unknown stream")
        high = db.execute("SELECT max(sequence) FROM observations WHERE execution=?", (execution[0],)).fetchone()[0]
        if after > high:
            raise ReplayError("cursor beyond retained stream")
        rows = db.execute("""SELECT sequence, body FROM observations WHERE execution=?
                             AND sequence>? ORDER BY sequence LIMIT ?""", (execution[0], after, limit))
        return [(route, seq, raw) for seq, raw in rows]


def consume(root, expected_route, page):
    with connect(root, "coven.db") as db, db:
        db.execute("BEGIN IMMEDIATE")
        high = db.execute("""SELECT coalesce(max(sequence), 0) FROM inbox
                             WHERE producer=? AND key=? AND execution=?""", expected_route).fetchone()[0]
        for route, sequence, body in page:
            if route != expected_route or type(sequence) is not int or sequence < 1:
                raise ReplayError("wrong stream or invalid sequence")
            event = json.loads(body)
            if event["profile"] != EVENT_PROFILE or event["kind"] not in ("adopted", "lane-observed"):
                raise ReplayError("unsupported event")
            if type(event["sequence"]) is not int or event["sequence"] != sequence:
                raise ReplayError("event sequence mismatch")
            if event["kind"] == "adopted":
                source = event["data"]
                if (source["producer"], source["invocationKey"], source["executionId"]) != route:
                    raise ReplayError("crossed adoption body")
            if sequence <= high:
                prior = db.execute("""SELECT body FROM inbox WHERE producer=? AND key=?
                                      AND execution=? AND sequence=?""", (*route, sequence)).fetchone()
                if not prior or prior[0] != body:
                    raise ReplayError("different bytes for retained event")
            elif sequence == high + 1:
                db.execute("INSERT INTO inbox VALUES (?, ?, ?, ?, ?)", (*route, sequence, body))
                high = sequence
            else:
                raise ReplayError("missing event")
    return high


def subscriber_route(root):
    with connect(root, "coven.db") as db:
        producer, key, raw = db.execute("SELECT producer, key, response FROM pending").fetchone()
    if not raw:
        raise ReplayError("caller has no confirmed adoption")
    return producer, key, json.loads(raw)["executionId"]


def child(mode, root):
    if mode == "adopt-and-exit":
        _, _, raw = read_pending(root)
        with connect(root, "coven.db") as db, db:
            db.execute("INSERT INTO outbound VALUES ('adopt')")
        adopt(root / "psyche.db", raw, load("verified-input.json"))
        os._exit(71)  # The adoption committed. Its response never reached the requester.
    if mode == "lookup":
        return recover_producer(root)
    route = subscriber_route(root)
    if mode == "observe-and-exit":
        # Retain an observation supplied by the fixture host. Do not start a worker.
        with connect(root, "psyche.db") as db:
            execution, = db.execute("SELECT id FROM executions WHERE producer=? AND invocation_key=?", route[:2]).fetchone()
            for event in load("replay-observations.json"):
                db.execute("INSERT INTO observations VALUES (?, ?, ?)",
                           (execution, event["sequence"], json.dumps(event).encode()))
                if event["sequence"] == 2:
                    db.commit()
            os._exit(72)  # The later observation is present only in the uncommitted transaction.
    if mode == "snapshot":
        return [json.loads(body) for _, _, body in read_page(root, route, 0, 8)]
    if mode == "consume-and-exit":
        consume(root, route, read_page(root, route, 0, 1))
        os._exit(73)  # Subscriber committed the event before its acknowledgement.
    if mode == "reconnect":
        return consume(root, route, read_page(root, route, 0, 8))
    if mode == "resume":
        cursor = consume(root, route, [])
        page = read_page(root, route, cursor, 8)
        return {"received": len(page), "cursor": consume(root, route, page)}
    raise ValueError(mode)


class ReplayVectors(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        stage(self.root)

    def worker(self, mode, code=0):
        result = subprocess.run([sys.executable, "-B", __file__, "--child", mode, str(self.root)],
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, code, result.stderr)
        return json.loads(result.stdout) if result.stdout else None

    def test_requester_restart_recovers_original_attempt_by_lookup(self):
        self.worker("adopt-and-exit", 71)
        with connect(self.root, "coven.db") as db:
            self.assertIsNone(db.execute("SELECT response FROM pending").fetchone()[0])
        self.assertEqual(self.worker("lookup"), load("result.json"))
        with connect(self.root, "coven.db") as db:
            self.assertEqual(db.execute("SELECT command FROM outbound ORDER BY rowid").fetchall(),
                             [("adopt",), ("lookup",)])
            self.assertEqual(json.loads(db.execute("SELECT response FROM pending").fetchone()[0]), load("result.json"))
        with connect(self.root, "psyche.db") as db:
            self.assertEqual(db.execute("SELECT count(*) FROM executions").fetchone()[0], 1)
            self.assertEqual(db.execute("SELECT count(*) FROM observations").fetchone()[0], 1)

    def test_missing_or_crossed_lookup_keeps_request_pending(self):
        with self.assertRaisesRegex(ReplayError, "unresolved"):
            recover_producer(self.root)
        self.worker("adopt-and-exit", 71)
        wrong = dict(load("result.json"), attemptId="different-attempt")
        with connect(self.root, "psyche.db") as db, db:
            db.execute("UPDATE executions SET result=?", (json.dumps(wrong).encode(),))
        with self.assertRaisesRegex(ReplayError, "correlation mismatch"):
            recover_producer(self.root)
        with connect(self.root, "coven.db") as db:
            self.assertIsNone(db.execute("SELECT response FROM pending").fetchone()[0])
            self.assertEqual(db.execute("SELECT command FROM outbound ORDER BY rowid").fetchall(),
                             [("lookup",), ("adopt",), ("lookup",)])

    def test_psyche_restart_preserves_last_committed_active_lane(self):
        self.worker("adopt-and-exit", 71)
        self.worker("lookup")
        self.worker("observe-and-exit", 72)
        snapshot = self.worker("snapshot")
        self.assertEqual(snapshot, [{"profile": EVENT_PROFILE, "sequence": 1, "kind": "adopted", "data": load("result.json")},
                                    load("replay-observations.json")[0]])
        self.assertEqual(self.worker("snapshot"), snapshot)

    def test_subscriber_reconnect_replays_committed_event_once(self):
        self.worker("adopt-and-exit", 71)
        self.worker("lookup")
        self.worker("observe-and-exit", 72)
        self.worker("consume-and-exit", 73)
        self.assertEqual(self.worker("reconnect"), 2)
        self.assertEqual(self.worker("resume"), {"received": 0, "cursor": 2})
        route = subscriber_route(self.root)
        second = read_page(self.root, route, 1, 1)
        self.assertEqual([entry[1] for entry in second], [2])
        self.assertEqual(read_page(self.root, route, 2, 1), [])
        with connect(self.root, "coven.db") as db:
            rows = db.execute("SELECT sequence, body FROM inbox ORDER BY sequence").fetchall()
        self.assertEqual([seq for seq, _ in rows], [1, 2])
        self.assertEqual([json.loads(raw) for _, raw in rows], self.worker("snapshot"))

    def test_gap_conflict_and_cross_stream_batch_leave_cursor_unchanged(self):
        self.worker("adopt-and-exit", 71)
        self.worker("lookup")
        self.worker("observe-and-exit", 72)
        route = subscriber_route(self.root)
        first, second = read_page(self.root, route, 0, 2)
        wrong_route = (route[0], "other-invocation", route[2])
        unknown = dict(json.loads(second[2]), kind="future-event")
        crossed = json.loads(first[2])
        crossed["data"]["invocationKey"] = "different-invocation"
        for page in ([second], [first, (wrong_route, second[1], second[2])],
                     [first, (route, 2, json.dumps(unknown).encode())],
                     [(route, 1, json.dumps(crossed).encode())],
                     [first, (route, 1, first[2] + b" ")]):
            with self.assertRaises(ReplayError):
                consume(self.root, route, page)
            self.assertEqual(consume(self.root, route, []), 0)
        self.assertEqual(consume(self.root, route, [first, second]), 2)
        with self.assertRaises(ReplayError):
            consume(self.root, route, [(route, 1, first[2] + b" ")])
        self.assertEqual(consume(self.root, route, []), 2)
        for candidate, after, limit in ((wrong_route, 0, 1), (route, 3, 1), (route, 0, 9)):
            with self.assertRaises(ReplayError):
                read_page(self.root, candidate, after, limit)


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--child":
        print(json.dumps(child(sys.argv[2], Path(sys.argv[3]))))
    else:
        unittest.main()
