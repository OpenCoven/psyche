"""Process-restart and event-replay vectors. Run with Python 3.9+."""

import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

# Keep a clean checkout when this standalone runner imports the adoption fixture,
# and find that fixture when the interpreter omits the script directory (-P).
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify import (ADOPTION_PROFILE, ANY, EVENT_PROFILE, EXECUTION, HERE, RESULT, Denied, Refusal,
                    adopt, conform, connect, decode, digest, initialize, load, lookup_bytes,
                    transaction)

PAGE_LIMIT = 8
LANE_STATES = ("active", "returned")  # A lane may only move rightwards.
EVENT = {"profile": str, "execution": EXECUTION, "sequence": int, "kind": str, "data": ANY}
LANE = {"taskId": str, "laneId": str, "leaseId": str, "workerAttemptRef": str, "observedState": str}


class ReplayError(Refusal):
    pass


def strict(check, *args):
    try:
        return check(*args)
    except Denied as error:
        raise ReplayError(error.code, error.detail) from None


def stage(root):
    initialize(root / "psyche.db")
    verified = load("verified-input.json")
    with transaction(root / "coven.db", "IMMEDIATE") as db:
        db.execute("""CREATE TABLE pending (producer TEXT, key TEXT, request BLOB, response BLOB,
                                            PRIMARY KEY (producer, key))""")
        db.execute("CREATE TABLE outbound (command TEXT)")
        db.execute("""CREATE TABLE inbox (producer TEXT, key TEXT, execution TEXT, sequence INTEGER, body BLOB,
                                          PRIMARY KEY (producer, key, execution, sequence))""")
        db.execute("INSERT INTO pending VALUES (?, ?, ?, NULL)",
                   (verified["producer"], verified["invocationKey"], (HERE / "request.json").read_bytes()))


def read_pending(root):
    with connect(root / "coven.db") as db:
        return db.execute("SELECT producer, key, request, response FROM pending ORDER BY producer, key").fetchone()


def note(root, command):
    with transaction(root / "coven.db", "IMMEDIATE") as db:
        db.execute("INSERT INTO outbound VALUES (?)", (command,))


def confirmed(producer, key, request, response):
    """Return a result only when it proves that this exact request was adopted."""
    if response is None:
        raise ReplayError("no-confirmed-adoption")
    result = strict(decode, response)
    if type(result) is not dict or result.get("profile") != ADOPTION_PROFILE:
        raise ReplayError("unsupported-result-profile")
    strict(conform, result, RESULT, "result")
    if result["state"] != "adopted":
        raise ReplayError("result-not-adopted")
    if result["producer"] != producer:
        raise ReplayError("lookup-crossed", "producer")
    if result["invocationKey"] != key:
        raise ReplayError("lookup-crossed", "invocationKey")
    if result["requestSha256"] != digest(request):
        raise ReplayError("lookup-crossed", "requestSha256")
    source = strict(decode, request)["source"]
    for name in ("occurrenceId", "runId", "attemptId"):
        if result[name] != source[name]:
            raise ReplayError("lookup-crossed", name)
    return result


def recover_producer(root):
    producer, key, request, _ = read_pending(root)
    note(root, "lookup")
    response = lookup_bytes(root / "psyche.db", producer, key)
    if response is None:
        raise ReplayError("lookup-unresolved")
    result = confirmed(producer, key, request, response)
    with transaction(root / "coven.db", "IMMEDIATE") as db:
        db.execute("UPDATE pending SET response=? WHERE producer=? AND key=?", (response, producer, key))
    return result


def subscription(root):
    """The caller's stream route and the adoption it retained for that route."""
    producer, key, request, response = read_pending(root)
    result = confirmed(producer, key, request, response)
    return (producer, key, result["executionId"]), result


def subscriber_route(root):
    return subscription(root)[0]


def correlation(route):
    return dict(zip(("producer", "invocationKey", "executionId"), route))


def parse_event(body):
    if type(body) is not bytes:
        raise ReplayError("invalid-envelope")
    event = strict(decode, body)
    if type(event) is not dict or event.get("profile") != EVENT_PROFILE:
        raise ReplayError("unsupported-event-profile")
    strict(conform, event, EVENT, "event")
    if event["kind"] == "adopted":
        strict(conform, event["data"], RESULT, "event.data")
    elif event["kind"] == "lane-observed":
        strict(conform, event["data"], LANE, "event.data")
        if event["data"]["observedState"] not in LANE_STATES:
            raise ReplayError("unsupported-observed-state")
    else:
        raise ReplayError("unsupported-event-kind")
    return event


def advance(lanes, event):
    """Refuse an observation that moves a lane back to an earlier state."""
    if event["kind"] != "lane-observed":
        return
    lane, rank = event["data"]["laneId"], LANE_STATES.index(event["data"]["observedState"])
    if rank < lanes.get(lane, 0):
        raise ReplayError("lane-regression")
    lanes[lane] = rank


def append(db, execution, route, event):
    """Psyche retains an observation only under the execution that it names."""
    body = json.dumps(event).encode()
    if parse_event(body)["execution"] != correlation(route):
        raise ReplayError("crossed-stream")
    db.execute("INSERT INTO observations VALUES (?, ?, ?)", (execution, event["sequence"], body))


def read_page(root, route, after, limit):
    if type(after) is not int or after < 0 or type(limit) is not int or not 1 <= limit <= PAGE_LIMIT:
        raise ReplayError("invalid-page-request")
    with transaction(root / "psyche.db", "DEFERRED") as db:
        execution = db.execute("SELECT id, result FROM executions WHERE producer=? AND invocation_key=?",
                               route[:2]).fetchone()
        if not execution or decode(execution[1])["executionId"] != route[2]:
            raise ReplayError("unknown-stream")
        high = db.execute("SELECT max(sequence) FROM observations WHERE execution=?", (execution[0],)).fetchone()[0]
        if after > high:
            raise ReplayError("cursor-beyond-retained")
        rows = db.execute("""SELECT sequence, body FROM observations WHERE execution=?
                             AND sequence>? ORDER BY sequence LIMIT ?""", (execution[0], after, limit)).fetchall()
    return [(route, sequence, body) for sequence, body in rows]


def consume(root, page):
    """Apply one delivered page atomically. Any refusal leaves the cursor unchanged."""
    route, adoption = subscription(root)
    if type(page) is not list:
        raise ReplayError("invalid-envelope")
    if len(page) > PAGE_LIMIT:
        raise ReplayError("page-too-large")
    applied = duplicates = 0
    with transaction(root / "coven.db", "IMMEDIATE") as db:
        retained = [body for _, body in db.execute(
            """SELECT sequence, body FROM inbox WHERE producer=? AND key=? AND execution=?
               ORDER BY sequence""", route)]
        lanes = {}
        for body in retained:
            advance(lanes, parse_event(body))
        for entry in page:
            if type(entry) is not tuple or len(entry) != 3:
                raise ReplayError("invalid-envelope")
            entry_route, sequence, body = entry
            if entry_route != route:
                raise ReplayError("wrong-stream")
            if type(sequence) is not int or sequence < 1:
                raise ReplayError("invalid-envelope")
            event = parse_event(body)
            if event["sequence"] != sequence:
                raise ReplayError("sequence-mismatch")
            if event["execution"] != correlation(route):
                raise ReplayError("crossed-stream")
            if (event["kind"] == "adopted") != (sequence == 1):
                raise ReplayError("adoption-out-of-place")
            if event["kind"] == "adopted" and event["data"] != adoption:
                raise ReplayError("crossed-adoption")
            if sequence <= len(retained):
                if retained[sequence - 1] != body:
                    raise ReplayError("retained-bytes-differ")
                duplicates += 1
            elif sequence == len(retained) + 1:
                advance(lanes, event)
                db.execute("INSERT INTO inbox VALUES (?, ?, ?, ?, ?)", (*route, sequence, body))
                retained.append(body)
                applied += 1
            else:
                raise ReplayError("missing-event")
    return {"cursor": len(retained), "applied": applied, "duplicates": duplicates}


def child(mode, root):
    if mode == "adopt-and-exit":
        note(root, "adopt")
        adopt(root / "psyche.db", read_pending(root)[2], load("verified-input.json"))
        os._exit(71)  # The adoption committed. Its response never reached the requester.
    if mode == "lookup":
        return recover_producer(root)
    route = subscriber_route(root)
    if mode == "observe-and-exit":
        # Retain an observation supplied by the fixture host. Do not start a worker.
        active, returned = load("replay-observations.json")
        db = sqlite3.connect(str(root / "psyche.db"), isolation_level=None)
        execution, = db.execute("SELECT id FROM executions WHERE producer=? AND invocation_key=?", route[:2]).fetchone()
        db.execute("BEGIN IMMEDIATE")
        append(db, execution, route, active)
        db.execute("COMMIT")
        db.execute("BEGIN IMMEDIATE")
        append(db, execution, route, returned)
        os._exit(72)  # The later observation is present only in the uncommitted transaction.
    if mode == "snapshot":
        return [decode(body) for _, _, body in read_page(root, route, 0, PAGE_LIMIT)]
    if mode == "consume-and-exit":
        consume(root, read_page(root, route, 0, 1))
        os._exit(73)  # Subscriber committed the event before its acknowledgement.
    if mode == "reconnect":
        page = read_page(root, route, 0, PAGE_LIMIT)
        return dict(consume(root, page), delivered=[sequence for _, sequence, _ in page])
    if mode == "resume":
        page = read_page(root, route, consume(root, [])["cursor"], PAGE_LIMIT)
        return dict(consume(root, page), delivered=[sequence for _, sequence, _ in page])
    raise ValueError(mode)


class ReplayVectors(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        stage(self.root)
        self.request = (HERE / "request.json").read_bytes()
        self.expected = load("result.json")
        self.observations = load("replay-observations.json")

    def worker(self, mode, code=0):
        result = subprocess.run([sys.executable, "-B", __file__, "--child", mode, str(self.root)],
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, code, result.stderr)
        return json.loads(result.stdout) if result.stdout else None

    def refused(self, code, detail, call, *args):
        with self.assertRaises(ReplayError) as caught:
            call(*args)
        self.assertEqual((caught.exception.code, caught.exception.detail), (code, detail))

    def rows(self, name, query):
        with connect(self.root / name) as db:
            return db.execute(query).fetchall()

    def response(self):
        return self.rows("coven.db", "SELECT response FROM pending")[0][0]

    def inbox(self):
        return [sequence for sequence, in self.rows("coven.db", "SELECT sequence FROM inbox ORDER BY sequence")]

    def adopted(self):
        """The committed adoption whose reply was lost, without a process exit."""
        with self.assertRaises(TimeoutError):
            adopt(self.root / "psyche.db", self.request, load("verified-input.json"), lose_reply=True)

    def store_result(self, raw):
        with transaction(self.root / "psyche.db", "IMMEDIATE") as db:
            db.execute("UPDATE executions SET result=?", (raw,))

    def subscribed(self, *observed):
        """A confirmed adoption plus retained observations; returns route and stream."""
        self.adopted()
        recover_producer(self.root)
        route = subscriber_route(self.root)
        with transaction(self.root / "psyche.db", "IMMEDIATE") as db:
            for event in observed:
                append(db, 1, route, event)
        return route, read_page(self.root, route, 0, PAGE_LIMIT)

    def event(self, route, sequence, **edits):
        """The retained lane observation renumbered, with optional member edits."""
        event = json.loads(json.dumps(self.observations[0]))
        event["sequence"] = sequence
        for name, value in edits.items():
            (event["data"] if name in LANE else event)[name] = value
        return route, sequence, json.dumps(event).encode()

    def test_requester_restart_recovers_original_attempt_by_lookup(self):
        self.worker("adopt-and-exit", 71)
        self.assertIsNone(self.response())
        self.assertEqual(self.worker("lookup"), self.expected)
        self.assertEqual(self.rows("coven.db", "SELECT command FROM outbound ORDER BY rowid"),
                         [("adopt",), ("lookup",)])
        self.assertEqual(decode(self.response()), self.expected)
        self.assertEqual(self.rows("psyche.db", "SELECT count(*) FROM executions"), [(1,)])
        self.assertEqual(self.rows("psyche.db", "SELECT count(*) FROM observations"), [(1,)])

    def test_missing_or_crossed_lookup_keeps_request_pending(self):
        self.refused("lookup-unresolved", "", recover_producer, self.root)
        self.refused("no-confirmed-adoption", "", subscriber_route, self.root)
        self.adopted()
        for name, value in (("producer", "coven:other"), ("invocationKey", "psyche-adoption-002"),
                            ("requestSha256", "0" * 64), ("occurrenceId", "occ-002"),
                            ("runId", "run-002"), ("attemptId", "different-attempt")):
            with self.subTest(crossed=name):
                self.store_result(json.dumps(dict(self.expected, **{name: value})).encode())
                self.refused("lookup-crossed", name, recover_producer, self.root)
                self.assertIsNone(self.response())
                self.refused("no-confirmed-adoption", "", subscriber_route, self.root)
        self.assertEqual(self.rows("coven.db", "SELECT command FROM outbound ORDER BY rowid"),
                         [("lookup",)] * 7)
        self.store_result(json.dumps(self.expected).encode())
        self.assertEqual(recover_producer(self.root), self.expected)

    def test_lookup_result_that_is_not_a_known_adoption_stays_pending(self):
        self.adopted()
        without = lambda name: {key: value for key, value in self.expected.items() if key != name}
        text = json.dumps(self.expected)
        vectors = [
            ("result-not-adopted", "", dict(self.expected, state="failed")),
            ("result-not-adopted", "", dict(self.expected, state="recovery-required")),
            ("result-not-adopted", "", dict(self.expected, state="Adopted")),
            ("missing-field", "result.state", without("state")),
            ("wrong-type", "result.state", dict(self.expected, state=None)),
            ("unsupported-result-profile", "", dict(self.expected, profile=ADOPTION_PROFILE[:-1] + "2")),
            ("unsupported-result-profile", "", without("profile")),
            ("unsupported-result-profile", "", [self.expected]),
            ("wrong-type", "result.executionId", dict(self.expected, executionId=1)),
            ("wrong-type", "result.executionId", dict(self.expected, executionId=None)),
            ("missing-field", "result.executionId", without("executionId")),
            ("unknown-field", "result.outcome", dict(self.expected, outcome="succeeded")),
            ("duplicate-key", "state", text[:-1] + ', "state": "adopted"}'),
            ("non-finite-number", "NaN", text[:-1] + ', "extra": NaN}'),
            ("malformed-json", "", text[:-1]),
            ("byte-order-mark", "", "\ufeff" + text),
        ]
        for code, detail, stored in vectors:
            with self.subTest(code=code, detail=detail):
                raw = stored if type(stored) is str else json.dumps(stored)
                self.store_result(raw.encode())
                self.refused(code, detail, recover_producer, self.root)
                self.assertIsNone(self.response())
                self.refused("no-confirmed-adoption", "", subscriber_route, self.root)

    def test_retained_response_is_rechecked_before_it_names_a_stream(self):
        route, stream = self.subscribed()
        with transaction(self.root / "coven.db", "IMMEDIATE") as db:
            db.execute("UPDATE pending SET response=?", (json.dumps(dict(self.expected, state="failed")).encode(),))
        self.refused("result-not-adopted", "", subscriber_route, self.root)
        self.refused("result-not-adopted", "", consume, self.root, stream)
        self.assertEqual(self.inbox(), [])

    def test_psyche_restart_preserves_last_committed_active_lane(self):
        self.worker("adopt-and-exit", 71)
        self.worker("lookup")
        self.worker("observe-and-exit", 72)
        snapshot = self.worker("snapshot")
        first = {"profile": EVENT_PROFILE, "execution": self.observations[0]["execution"],
                 "sequence": 1, "kind": "adopted", "data": self.expected}
        self.assertEqual(snapshot, [first, self.observations[0]])
        self.assertEqual(self.worker("snapshot"), snapshot)

    def test_subscriber_reconnect_replays_committed_event_once(self):
        self.worker("adopt-and-exit", 71)
        self.worker("lookup")
        self.worker("observe-and-exit", 72)
        self.worker("consume-and-exit", 73)
        # The event was committed before the exit, so the next delivery repeats it.
        self.assertEqual(self.inbox(), [1])
        self.assertEqual(self.worker("reconnect"),
                         {"delivered": [1, 2], "duplicates": 1, "applied": 1, "cursor": 2})
        self.assertEqual(self.worker("resume"), {"delivered": [], "duplicates": 0, "applied": 0, "cursor": 2})
        route = subscriber_route(self.root)
        self.assertEqual([entry[1] for entry in read_page(self.root, route, 1, 1)], [2])
        self.assertEqual(read_page(self.root, route, 2, 1), [])
        rows = self.rows("coven.db", "SELECT sequence, body FROM inbox ORDER BY sequence")
        self.assertEqual([sequence for sequence, _ in rows], [1, 2])
        self.assertEqual([decode(body) for _, body in rows], self.worker("snapshot"))

    def test_gap_conflict_and_cross_stream_batch_leave_cursor_unchanged(self):
        route, (first, second) = self.subscribed(self.observations[0])
        wrong_route = (route[0], "other-invocation", route[2])
        for code, page in (
            ("missing-event", [second]),
            ("wrong-stream", [first, (wrong_route, second[1], second[2])]),
            ("retained-bytes-differ", [first, (route, 1, first[2] + b" ")]),
            ("page-too-large", [first] * (PAGE_LIMIT + 1)),
            ("invalid-envelope", (first, second)),
            ("invalid-envelope", [first, list(second)]),
            ("invalid-envelope", [first, (route, True, second[2])]),
            ("invalid-envelope", [first, (route, 2, second[2].decode())]),
        ):
            with self.subTest(code=code):
                self.refused(code, "", consume, self.root, page)
                self.assertEqual(self.inbox(), [])
        self.assertEqual(consume(self.root, [first] * PAGE_LIMIT), {"cursor": 1, "applied": 1, "duplicates": 7})
        self.assertEqual(consume(self.root, [first, second]), {"cursor": 2, "applied": 1, "duplicates": 1})
        self.refused("retained-bytes-differ", "", consume, self.root, [(route, 1, first[2] + b" ")])
        self.assertEqual(consume(self.root, []), {"cursor": 2, "applied": 0, "duplicates": 0})

    def test_stream_requests_must_name_the_adopted_execution_and_a_retained_cursor(self):
        route, stream = self.subscribed(self.observations[0])
        for code, candidate, after, limit in (
            ("unknown-stream", ("coven:other", route[1], route[2]), 0, 1),
            ("unknown-stream", (route[0], "other-invocation", route[2]), 0, 1),
            ("unknown-stream", (route[0], route[1], "fixture-execution-2"), 0, 1),
            ("cursor-beyond-retained", route, 3, 1),
            ("invalid-page-request", route, 0, PAGE_LIMIT + 1),
            ("invalid-page-request", route, 0, 0),
            ("invalid-page-request", route, -1, 1),
            ("invalid-page-request", route, False, 1),
            ("invalid-page-request", route, 0, True),
        ):
            with self.subTest(code=code, candidate=candidate, after=after, limit=limit):
                self.refused(code, "", read_page, self.root, candidate, after, limit)
        self.assertEqual(read_page(self.root, route, 2, 1), [])
        self.assertEqual(len(stream), 2)

    def test_event_bodies_are_bound_to_the_adopted_execution(self):
        route, (first, second) = self.subscribed(self.observations[0])
        adoption = json.loads(first[2])

        def body(event):
            return json.dumps(event).encode()

        def adopted(**edits):
            return route, 1, body(dict(adoption, data=dict(self.expected, **edits)))

        def elsewhere(name, value, sequence=2, base=None):
            event = json.loads((base or second)[2])
            event["execution"][name] = value
            event["sequence"] = sequence
            return route, sequence, body(event)

        def without(*path):
            event = json.loads(second[2])
            holder = event
            for name in path[:-1]:
                holder = holder[name]
            del holder[path[-1]]
            return route, 2, body(event)

        vectors = [
            ("crossed-stream", "", [first, elsewhere("executionId", "fixture-execution-2")]),
            ("crossed-stream", "", [first, elsewhere("invocationKey", "psyche-adoption-002")]),
            ("crossed-stream", "", [first, elsewhere("producer", "coven:other")]),
            ("crossed-stream", "", [elsewhere("executionId", "fixture-execution-2", 1, first)]),
            ("crossed-adoption", "", [adopted(attemptId="attempt-003")]),
            ("crossed-adoption", "", [adopted(runId="run-002")]),
            ("crossed-adoption", "", [adopted(occurrenceId="occ-002")]),
            ("crossed-adoption", "", [adopted(requestSha256="0" * 64)]),
            ("crossed-adoption", "", [adopted(state="failed")]),
            ("adoption-out-of-place", "", [first, (route, 2, body(dict(adoption, sequence=2)))]),
            ("adoption-out-of-place", "", [self.event(route, 1)]),
            ("sequence-mismatch", "", [first, (route, 2, self.event(route, 3)[2])]),
            ("unsupported-event-profile", "", [first, self.event(route, 2, profile=EVENT_PROFILE[:-1] + "2")]),
            ("unsupported-event-profile", "", [first, (route, 2, b"[]")]),
            ("unsupported-event-kind", "", [first, self.event(route, 2, kind="future-event")]),
            ("unsupported-observed-state", "", [first, self.event(route, 2, observedState="completed")]),
            ("wrong-type", "event.data", [(route, 1, body(dict(adoption, data=None)))]),
            ("missing-field", "event.data.state",
             [(route, 1, body(dict(adoption, data={k: v for k, v in self.expected.items() if k != "state"})))]),
            ("wrong-type", "event.data", [first, self.event(route, 2, data=None)]),
            ("wrong-type", "event.data", [first, self.event(route, 2, data=[])]),
            ("wrong-type", "event.data.observedState", [first, self.event(route, 2, observedState=1)]),
            ("wrong-type", "event.sequence", [first, (route, 2, self.event(route, 2.0)[2])]),
            ("wrong-type", "event.execution", [first, self.event(route, 2, execution="fixture-execution-1")]),
            ("unknown-field", "event.outcome", [first, self.event(route, 2, outcome="succeeded")]),
            ("unknown-field", "event.data.capabilities",
             [first, (route, 2, second[2].replace(b'"taskId"', b'"capabilities": ["artifact.write"], "taskId"'))]),
            ("unknown-field", "event.execution.stream",
             [first, (route, 2, second[2].replace(b'"producer"', b'"stream": 1, "producer"'))]),
            ("missing-field", "event.data", [first, without("data")]),
            ("missing-field", "event.execution", [first, without("execution")]),
            ("missing-field", "event.data.laneId", [first, without("data", "laneId")]),
            ("missing-field", "event.execution.executionId", [first, without("execution", "executionId")]),
            ("duplicate-key", "kind",
             [first, (route, 2, second[2].replace(b'"kind"', b'"kind": "adopted", "kind"'))]),
            ("byte-order-mark", "", [first, (route, 2, b"\xef\xbb\xbf" + second[2])]),
            ("not-utf8", "", [first, (route, 2, second[2].decode().encode("utf-16"))]),
            ("lane-regression", "", [first, self.event(route, 2, observedState="returned"), self.event(route, 3)]),
        ]
        for code, detail, page in vectors:
            with self.subTest(code=code, detail=detail):
                self.refused(code, detail, consume, self.root, page)
                self.assertEqual(self.inbox(), [])
        # A regression is also refused against a state committed by an earlier page.
        returned = self.event(route, 2, observedState="returned")
        self.assertEqual(consume(self.root, [first, returned])["cursor"], 2)
        self.refused("lane-regression", "", consume, self.root, [self.event(route, 3)])
        other_lane = self.event(route, 3, laneId="lane-fixture-2")
        self.assertEqual(consume(self.root, [other_lane, self.event(route, 4, observedState="returned")]),
                         {"cursor": 4, "applied": 2, "duplicates": 0})

    def test_psyche_retains_an_observation_only_under_its_own_execution(self):
        route, _ = self.subscribed()
        foreign = dict(self.observations[0], execution=dict(self.observations[0]["execution"],
                                                            executionId="fixture-execution-2"))
        with transaction(self.root / "psyche.db", "IMMEDIATE") as db:
            self.refused("crossed-stream", "", append, db, 1, route, foreign)
            self.refused("unsupported-observed-state", "", append, db, 1, route,
                         dict(foreign, data=dict(foreign["data"], observedState="completed")))
        self.assertEqual(len(read_page(self.root, route, 0, PAGE_LIMIT)), 1)


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--child":
        print(json.dumps(child(sys.argv[2], Path(sys.argv[3]))))
    else:
        unittest.main()
