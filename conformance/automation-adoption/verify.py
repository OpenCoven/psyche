"""Executable design vectors for #18. Run with Python 3.9+ and the standard library."""

from concurrent.futures import ThreadPoolExecutor
from contextlib import closing, contextmanager
from datetime import datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import re
import sqlite3
import tempfile
import threading
import unittest

HERE = Path(__file__).resolve().parent
NOW = datetime(2026, 11, 20, 8, 1, tzinfo=timezone.utc)
PROFILE = "draft.psyche.automation-invocation.fixture.1"
ADOPTION_PROFILE = "draft.psyche.automation-adoption.fixture.1"
EVENT_PROFILE = "draft.psyche.automation-event.fixture.1"
RETENTION = ("fixture-only",)
TIMESTAMP = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z")
COUNTER_MAX = 2**53 - 1

# Closed shapes. A dict lists every permitted member, a list names its item
# shape, and a type must match exactly: a bool is not an int and 3.0 is not 3.
ANY = object()
DIGEST = {"algorithm": str, "canonicalization": str, "value": str}
SOURCE = {
    "automationId": str, "automationRevision": int, "definitionDigest": DIGEST,
    "occurrenceId": str, "occurrenceKey": str, "occurrenceFenceGeneration": int,
    "runId": str, "attemptId": str, "attemptNumber": int, "adoptionKey": str,
    "dispatchGeneration": int,
}
AUTHORITY = {
    "principalRef": str, "projectRef": str, "workspaceRef": str,
    "familiarBindingRef": str, "familiarBindingDigest": str,
    "decisionRef": str, "decisionDigest": str, "approvalRefs": [str],
    "runtimeRef": str, "riskClass": str, "scope": {"resource": str, "operation": str},
}
REQUEST = {
    "profile": str, "producer": str, "invocationKey": str, "source": SOURCE,
    "authority": AUTHORITY, "capabilities": [str], "brief": str, "payloadSha256": str,
    "deadline": str, "cancelChannel": str, "returnTo": str, "causeRef": str,
    "cursor": int, "retention": str,
}
VERIFIED = {
    "producer": str, "invocationKey": str, "source": SOURCE, "authority": AUTHORITY,
    "granted": [str], "validUntil": str, "revoked": bool, "payloadSha256": str,
}
EXECUTION = {"producer": str, "invocationKey": str, "executionId": str}
RESULT = {
    "profile": str, "producer": str, "invocationKey": str, "executionId": str,
    "occurrenceId": str, "runId": str, "attemptId": str, "requestSha256": str,
    "state": str,
}


class Refusal(Exception):
    """A structured refusal. `code` is the stable reason and `detail` locates it."""

    def __init__(self, code, detail=""):
        super().__init__(f"{code}: {detail}" if detail else code)
        self.code = code
        self.detail = detail


class Denied(Refusal):
    pass


class KeyConflict(Refusal):
    pass


def digest(raw):
    return sha256(raw).hexdigest()


def decode(raw):
    """Strict JSON: UTF-8 without a byte-order mark, no repeated member, finite numbers."""
    if type(raw) is not bytes:
        raise Denied("not-bytes")
    if raw.startswith(b"\xef\xbb\xbf"):
        raise Denied("byte-order-mark")
    try:
        text = raw.decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        raise Denied("not-utf8") from None

    def members(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise Denied("duplicate-key", key)
            value[key] = item
        return value

    def constant(name):
        raise Denied("non-finite-number", name)

    try:
        return json.loads(text, object_pairs_hook=members, parse_constant=constant)
    except (ValueError, RecursionError):
        raise Denied("malformed-json") from None


def conform(value, shape, where):
    if shape is ANY:
        return
    if isinstance(shape, dict):
        if type(value) is not dict:
            raise Denied("wrong-type", where)
        for key in value:
            if key not in shape:
                raise Denied("unknown-field", f"{where}.{key}")
        for key, inner in shape.items():
            if key not in value:
                raise Denied("missing-field", f"{where}.{key}")
            conform(value[key], inner, f"{where}.{key}")
    elif isinstance(shape, list):
        if type(value) is not list:
            raise Denied("wrong-type", where)
        for item in value:
            conform(item, shape[0], f"{where}[]")
    elif type(value) is not shape:
        raise Denied("wrong-type", where)
    elif shape is int and not 1 <= value <= COUNTER_MAX:
        raise Denied("out-of-range", where)
    elif shape is str:
        try:
            value.encode("utf-8")
        except UnicodeEncodeError:
            raise Denied("unpaired-surrogate", where) from None


def timestamp(value, where="timestamp"):
    """RFC 3339 in UTC with whole seconds and a literal Z, as the vectors spell it."""
    if type(value) is not str or not TIMESTAMP.fullmatch(value):
        raise Denied("malformed-timestamp", where)
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError:
        raise Denied("malformed-timestamp", where) from None


def load(name):
    return decode((HERE / name).read_bytes())


def connect(path):
    # Driver autocommit: every transaction below is an explicit statement, so the
    # behaviour is the same on each supported Python release.
    return closing(sqlite3.connect(str(path), isolation_level=None, timeout=30))


@contextmanager
def transaction(path, mode):
    with connect(path) as db:
        db.execute(f"BEGIN {mode}")
        try:
            yield db
        except BaseException:
            if db.in_transaction:
                db.execute("ROLLBACK")
            raise
        db.execute("COMMIT")


def initialize(path):
    with transaction(path, "IMMEDIATE") as db:
        db.execute("""CREATE TABLE executions (
            id INTEGER PRIMARY KEY, producer TEXT NOT NULL, invocation_key TEXT NOT NULL,
            run_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
            request BLOB NOT NULL, result BLOB NOT NULL, UNIQUE(producer, invocation_key),
            UNIQUE(producer, run_id, attempt_id)
        )""")
        db.execute("""CREATE TABLE observations (
            execution INTEGER NOT NULL REFERENCES executions(id), sequence INTEGER NOT NULL,
            body BLOB NOT NULL, PRIMARY KEY(execution, sequence)
        )""")


def lookup_bytes(path, producer, key):
    with connect(path) as db:
        row = db.execute("SELECT result FROM executions WHERE producer=? AND invocation_key=?",
                         (producer, key)).fetchone()
    return row[0] if row else None


def lookup(path, producer, key):
    raw = lookup_bytes(path, producer, key)
    return None if raw is None else decode(raw)


def difference(left, right):
    return next(key for key in left if left[key] != right[key])


def admit(raw, verified, now):
    """Deny before any state exists unless the bytes match the trusted projection."""
    request = decode(raw)
    if type(request) is not dict:
        raise Denied("wrong-type", "request")
    if "profile" not in request:
        raise Denied("missing-field", "request.profile")
    if request["profile"] != PROFILE:
        raise Denied("unsupported-profile")
    conform(request, REQUEST, "request")
    try:
        conform(verified, VERIFIED, "verified")
        valid_until = timestamp(verified["validUntil"], "verified.validUntil")
    except Denied as error:
        raise Denied("verified-input-invalid", f"{error.code} {error.detail}") from None
    deadline = timestamp(request["deadline"], "request.deadline")
    if request["retention"] not in RETENTION:
        raise Denied("unsupported-retention")
    if request["producer"] != verified["producer"]:
        raise Denied("producer-mismatch")
    if request["invocationKey"] != verified["invocationKey"]:
        raise Denied("invocation-key-mismatch")
    if request["source"] != verified["source"]:
        raise Denied("source-mismatch", difference(request["source"], verified["source"]))
    if request["authority"] != verified["authority"]:
        raise Denied("authority-mismatch", difference(request["authority"], verified["authority"]))
    if not set(request["capabilities"]).issubset(verified["granted"]):
        raise Denied("capability-widened")
    if verified["revoked"]:
        raise Denied("grant-revoked")
    if now >= valid_until:
        raise Denied("grant-expired")
    if now >= deadline:
        raise Denied("deadline-passed")
    if deadline > valid_until:
        raise Denied("deadline-exceeds-grant")
    if request["payloadSha256"] != verified["payloadSha256"]:
        raise Denied("payload-mismatch")
    if digest(request["brief"].encode("utf-8")) != request["payloadSha256"]:
        raise Denied("brief-digest-mismatch")
    return request


def adopt(path, raw, verified, now=NOW, lose_reply=False, hold=None):
    # verified models independently supplied producer records. There is no network
    # endpoint, credential verifier, schedule, worker or dispatch hook in this runner.
    # hold is a test seam: it runs inside the transaction, after the idempotency reads.
    request = admit(raw, verified, now)
    source = request["source"]
    key = (request["producer"], request["invocationKey"])
    with transaction(path, "IMMEDIATE") as db:
        row = db.execute("SELECT request, result FROM executions WHERE producer=? AND invocation_key=?", key).fetchone()
        if row:
            if row[0] != raw:
                raise KeyConflict("request-bytes-conflict")
            return decode(row[1])
        prior = db.execute(
            "SELECT invocation_key FROM executions WHERE producer=? AND run_id=? AND attempt_id=?",
            (key[0], source["runId"], source["attemptId"]),
        ).fetchone()
        if prior:
            raise KeyConflict("attempt-already-adopted")
        if hold:
            hold()
        inserted = db.execute(
            "INSERT INTO executions(producer,invocation_key,run_id,attempt_id,request,result) VALUES(?,?,?,?,?,?)",
            (*key, source["runId"], source["attemptId"], raw, b""),
        )
        result = {
            "profile": ADOPTION_PROFILE,
            "producer": key[0], "invocationKey": key[1], "executionId": f"fixture-execution-{inserted.lastrowid}",
            "occurrenceId": source["occurrenceId"], "runId": source["runId"],
            "attemptId": source["attemptId"], "requestSha256": digest(raw), "state": "adopted",
        }
        db.execute("UPDATE executions SET result=? WHERE id=?",
                   (json.dumps(result).encode(), inserted.lastrowid))
        execution = {name: result[name] for name in EXECUTION}
        first_event = {"profile": EVENT_PROFILE, "execution": execution, "sequence": 1,
                       "kind": "adopted", "data": result}
        db.execute("INSERT INTO observations VALUES (?, 1, ?)",
                   (inserted.lastrowid, json.dumps(first_event).encode()))
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

    def adoption(self, raw=None, verified=None, **kwargs):
        return adopt(self.path, self.wire if raw is None else raw,
                     self.verified if verified is None else verified, **kwargs)

    def refused(self, kind, code, detail, call, *args, **kwargs):
        with self.assertRaises(kind) as caught:
            call(*args, **kwargs)
        self.assertIs(type(caught.exception), kind)
        self.assertEqual((caught.exception.code, caught.exception.detail), (code, detail))

    def changed(self, edit):
        data = json.loads(self.wire)
        edit(data)
        return json.dumps(data).encode()

    def spliced(self, old, new):
        self.assertEqual(self.wire.count(old), 1, old)
        return self.wire.replace(old, new)

    def rows(self, query):
        with connect(self.path) as db:
            return db.execute(query).fetchall()

    def count(self):
        return self.rows("SELECT count(*) FROM executions")[0][0]

    def another(self, **edits):
        """A second well-formed invocation and its own trusted projection."""
        request, verified = json.loads(self.wire), json.loads(json.dumps(self.verified))
        for name, value in edits.items():
            for holder in (request, verified):
                (holder["source"] if name in SOURCE else holder)[name] = value
        return json.dumps(request).encode(), verified

    def test_first_adoption_records_one_execution_and_its_initial_event(self):
        self.assertEqual(self.adoption(), self.expected)
        (*stored, result), = self.rows(
            "SELECT producer, invocation_key, run_id, attempt_id, request, result FROM executions")
        self.assertEqual(stored, ["coven:fixture", "psyche-adoption-001", "run-001", "attempt-002", self.wire])
        self.assertEqual(decode(result), self.expected)
        events = self.rows("SELECT execution, sequence, body FROM observations")
        self.assertEqual([entry[:2] for entry in events], [(1, 1)])
        self.assertEqual(decode(events[0][2]), {
            "profile": EVENT_PROFILE, "sequence": 1, "kind": "adopted", "data": self.expected,
            "execution": {"producer": "coven:fixture", "invocationKey": "psyche-adoption-001",
                          "executionId": "fixture-execution-1"}})

    def test_exact_concurrent_redelivery(self):
        with ThreadPoolExecutor(max_workers=4) as workers:
            results = list(workers.map(lambda _: self.adoption(), range(8)))
        self.assertEqual(results, [self.expected] * 8)
        self.assertEqual(self.count(), 1)

    def test_contending_first_deliveries_are_serialized(self):
        # Both deliveries start together. The first writer then waits inside its
        # transaction for the other to pass the same idempotency reads. A writer
        # that holds the write lock from BEGIN can never be joined there.
        start, inside = threading.Barrier(2), threading.Barrier(2)

        def hold():
            try:
                inside.wait(timeout=2)
            except threading.BrokenBarrierError:
                pass

        def deliver(_):
            start.wait(timeout=30)
            return self.adoption(hold=hold)

        with ThreadPoolExecutor(max_workers=2) as workers:
            results = list(workers.map(deliver, range(2)))
        self.assertEqual(results, [self.expected] * 2)
        self.assertTrue(inside.broken)
        self.assertEqual(self.count(), 1)

    def test_conflict_preserves_execution_and_result(self):
        self.adoption()
        for raw in (self.wire + b" ", self.changed(lambda data: data.update(capabilities=[]))):
            self.refused(KeyConflict, "request-bytes-conflict", "", self.adoption, raw)
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-001"), self.expected)
        self.assertEqual(self.count(), 1)

    def test_one_adopted_attempt_cannot_fork_by_changing_keys(self):
        self.adoption()
        raw = self.changed(lambda data: data.update(invocationKey="psyche-adoption-002"))
        self.refused(Denied, "invocation-key-mismatch", "", self.adoption, raw)
        # Even an inconsistent issuer replay cannot fork a retained attempt.
        self.verified["invocationKey"] = "psyche-adoption-002"
        self.refused(KeyConflict, "attempt-already-adopted", "", self.adoption, raw)
        self.assertIsNone(lookup(self.path, "coven:fixture", "psyche-adoption-002"))
        self.assertEqual(self.count(), 1)

    def test_distinct_attempts_and_producers_get_distinct_executions(self):
        first = self.adoption()
        next_attempt = self.another(invocationKey="psyche-adoption-002", attemptId="attempt-003",
                                    attemptNumber=3, adoptionKey="dispatch-003", dispatchGeneration=3)
        # A different authenticated producer may reuse the same key and source IDs.
        other_producer = self.another(producer="coven:other")
        second, third = self.adoption(*next_attempt), self.adoption(*other_producer)
        self.assertEqual(self.count(), 3)
        self.assertEqual(len({first["executionId"], second["executionId"], third["executionId"]}), 3)
        self.assertEqual((second["attemptId"], second["requestSha256"]), ("attempt-003", digest(next_attempt[0])))
        self.assertEqual((third["producer"], third["requestSha256"]), ("coven:other", digest(other_producer[0])))
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-001"), first)
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-002"), second)
        self.assertEqual(lookup(self.path, "coven:other", "psyche-adoption-001"), third)
        self.assertEqual(self.adoption(*other_producer), third)
        self.assertEqual(self.adoption(), first)

    def test_timeout_after_commit_uses_lookup_on_new_connection(self):
        with self.assertRaises(TimeoutError):
            self.adoption(lose_reply=True)
        self.assertEqual(lookup(self.path, "coven:fixture", "psyche-adoption-001"), self.expected)
        self.assertIsNone(lookup(self.path, "other:fixture", "psyche-adoption-001"))
        self.assertEqual(self.count(), 1)

    def test_authority_and_integrity_denials_precede_adoption(self):
        other_brief = "A different brief."
        vectors = [
            ("authority-mismatch", "principalRef", lambda d: d["authority"].update(principalRef="other")),
            ("authority-mismatch", "familiarBindingDigest", lambda d: d["authority"].update(familiarBindingDigest="stale")),
            ("authority-mismatch", "decisionDigest", lambda d: d["authority"].update(decisionDigest="stale")),
            ("authority-mismatch", "projectRef", lambda d: d["authority"].update(projectRef="other")),
            ("authority-mismatch", "riskClass", lambda d: d["authority"].update(riskClass="R9")),
            ("authority-mismatch", "scope", lambda d: d["authority"].update(
                scope={"resource": "artifact:other", "operation": "write"})),
            ("source-mismatch", "occurrenceFenceGeneration", lambda d: d["source"].update(occurrenceFenceGeneration=2)),
            ("source-mismatch", "attemptId", lambda d: d["source"].update(attemptId="other")),
            ("source-mismatch", "attemptNumber", lambda d: d["source"].update(attemptNumber=3)),
            ("producer-mismatch", "", lambda d: d.update(producer="coven:other")),
            ("capability-widened", "", lambda d: d.update(capabilities=["artifact.read", "artifact.write"])),
            ("brief-digest-mismatch", "", lambda d: d.update(brief="uncommitted change")),
            ("payload-mismatch", "", lambda d: d.update(payloadSha256="changed")),
            ("payload-mismatch", "", lambda d: d.update(
                brief=other_brief, payloadSha256=digest(other_brief.encode()))),
            ("unsupported-profile", "", lambda d: d.update(profile="future")),
            ("unsupported-profile", "", lambda d: d.update(profile=PROFILE[:-1] + "2")),
            ("unsupported-profile", "", lambda d: d.update(profile=1)),
            ("unsupported-retention", "", lambda d: d.update(retention="forever")),
        ]
        for code, detail, edit in vectors:
            with self.subTest(code=code, detail=detail):
                self.refused(Denied, code, detail, self.adoption, self.changed(edit))
        self.verified["revoked"] = True
        self.refused(Denied, "grant-revoked", "", self.adoption)
        self.assertEqual(self.count(), 0)

    def test_each_time_bound_is_enforced_at_its_own_boundary(self):
        def at(now, deadline, valid_until):
            raw = self.changed(lambda data: data.update(deadline=f"2026-11-20T08:{deadline:02}:00Z"))
            verified = dict(self.verified, validUntil=f"2026-11-20T08:{valid_until:02}:00Z")
            return raw, verified, datetime(2026, 11, 20, 8, now, tzinfo=timezone.utc)

        # The three instants are distinct unless a row tests an exact boundary.
        vectors = [
            ("grant-expired", at(now=4, deadline=2, valid_until=3)),
            ("grant-expired", at(now=3, deadline=2, valid_until=3)),
            ("deadline-passed", at(now=3, deadline=2, valid_until=5)),
            ("deadline-passed", at(now=2, deadline=2, valid_until=5)),
            ("deadline-exceeds-grant", at(now=1, deadline=6, valid_until=5)),
        ]
        for code, (raw, verified, now) in vectors:
            with self.subTest(code=code, now=now.minute):
                self.refused(Denied, code, "", self.adoption, raw, verified, now=now)
        self.assertEqual(self.count(), 0)
        # One second inside every bound, and a deadline equal to the grant, adopt.
        raw, verified, _ = at(now=1, deadline=5, valid_until=5)
        latest = datetime(2026, 11, 20, 8, 4, 59, tzinfo=timezone.utc)
        self.assertEqual(self.adoption(raw, verified, now=latest)["state"], "adopted")

    def test_malformed_timestamps_are_denied(self):
        for value in ("2026-11-20T08:05:00+00:00", "2026-11-20T08:05:00.000Z", "2026-11-20t08:05:00z",
                      "2026-11-20 08:05:00Z", "2026-11-20T8:05:00Z", "2026-13-20T08:05:00Z",
                      "2026-11-20T08:05:60Z", "2026-11-20T08:05:00", "٢026-11-20T08:05:00Z", ""):
            with self.subTest(value=value):
                self.refused(Denied, "malformed-timestamp", "request.deadline", self.adoption,
                             self.changed(lambda data: data.update(deadline=value)))
                self.refused(Denied, "verified-input-invalid", "malformed-timestamp verified.validUntil",
                             self.adoption, verified=dict(self.verified, validUntil=value))
        self.assertEqual(self.count(), 0)

    def test_bytes_must_be_strict_utf8_json(self):
        text = self.wire.decode()
        vectors = [
            ("byte-order-mark", "", b"\xef\xbb\xbf" + self.wire),
            ("not-utf8", "", text.encode("utf-16")),
            ("malformed-json", "", text.encode("utf-16-le")),
            ("malformed-json", "", text.encode("utf-16-be")),
            ("not-utf8", "", text.encode("utf-32")),
            ("not-utf8", "", self.spliced(b"daily-report@", b"daily-report\xff@")),
            ("malformed-json", "", b""),
            ("malformed-json", "", self.wire[:-3]),
            ("malformed-json", "", self.wire + b"{}"),
            ("malformed-json", "", self.spliced(b'"cursor": 1,', b'"cursor": 01,')),
            ("non-finite-number", "NaN", self.spliced(b'"cursor": 1,', b'"cursor": NaN,')),
            ("non-finite-number", "Infinity", self.spliced(b'"cursor": 1,', b'"cursor": Infinity,')),
            ("non-finite-number", "-Infinity", self.spliced(b'"cursor": 1,', b'"cursor": -Infinity,')),
            ("duplicate-key", "capabilities",
             self.spliced(b'"capabilities": [', b'"capabilities": ["artifact.write"], "capabilities": [')),
            ("duplicate-key", "profile", self.spliced(b'"producer":', b'"profile": "future", "producer":')),
            ("duplicate-key", "invocationKey",
             self.spliced(b'"source":', b'"invocationKey": "psyche-adoption-002", "source":')),
            ("duplicate-key", "operation",
             self.spliced(b'"operation": "read"', b'"operation": "write", "operation": "read"')),
            ("duplicate-key", "value", self.spliced(b'"canonicalization":', b'"value": "0", "canonicalization":')),
            ("wrong-type", "request", b"[]"),
            ("wrong-type", "request", b'"request"'),
            ("wrong-type", "request", b"null"),
            ("unpaired-surrogate", "request.brief",
             self.spliced(b"Summarize the provided fixture artifact.", b"\\ud800")),
        ]
        for code, detail, raw in vectors:
            with self.subTest(code=code, detail=detail, raw=raw[:24]):
                self.refused(Denied, code, detail, self.adoption, raw)
        self.assertEqual(self.count(), 0)

    def test_request_shape_is_closed_and_exactly_typed(self):
        for name in REQUEST:
            with self.subTest(missing=name):
                self.refused(Denied, "missing-field", f"request.{name}", self.adoption,
                             self.changed(lambda data: data.pop(name)))
        vectors = [
            ("unknown-field", "request.lanes",
             lambda d: d.update(lanes=[{"capabilities": ["artifact.write"]}])),
            ("unknown-field", "request.source.schedule", lambda d: d["source"].update(schedule="hourly")),
            ("unknown-field", "request.source.definitionDigest.extra",
             lambda d: d["source"]["definitionDigest"].update(extra="x")),
            ("unknown-field", "request.authority.grantedCapabilities",
             lambda d: d["authority"].update(grantedCapabilities=["artifact.write"])),
            ("unknown-field", "request.authority.scope.also",
             lambda d: d["authority"]["scope"].update(also="artifact:other")),
            ("missing-field", "request.source.attemptNumber", lambda d: d["source"].pop("attemptNumber")),
            ("missing-field", "request.authority.scope.operation", lambda d: d["authority"]["scope"].pop("operation")),
            ("wrong-type", "request.capabilities", lambda d: d.update(capabilities={})),
            ("wrong-type", "request.capabilities", lambda d: d.update(capabilities="")),
            ("wrong-type", "request.capabilities", lambda d: d.update(capabilities="artifact.read")),
            ("wrong-type", "request.capabilities", lambda d: d.update(capabilities={"artifact.read": True})),
            ("wrong-type", "request.capabilities", lambda d: d.update(capabilities=None)),
            ("wrong-type", "request.capabilities[]", lambda d: d.update(capabilities=[["artifact.read"]])),
            ("wrong-type", "request.capabilities[]", lambda d: d.update(capabilities=[1])),
            ("wrong-type", "request.source", lambda d: d.update(source=[])),
            ("wrong-type", "request.authority.scope", lambda d: d["authority"].update(scope="artifact:input read")),
            ("wrong-type", "request.authority.approvalRefs", lambda d: d["authority"].update(approvalRefs={})),
            ("wrong-type", "request.invocationKey", lambda d: d.update(invocationKey=1)),
            ("wrong-type", "request.brief", lambda d: d.update(brief=None)),
            ("wrong-type", "request.deadline", lambda d: d.update(deadline=1795161900)),
            ("wrong-type", "request.retention", lambda d: d.update(retention=["fixture-only"])),
            ("wrong-type", "request.cursor", lambda d: d.update(cursor="1")),
            ("wrong-type", "request.cursor", lambda d: d.update(cursor=True)),
            ("wrong-type", "request.cursor", lambda d: d.update(cursor=1.0)),
            ("wrong-type", "request.cursor", lambda d: d.update(cursor=None)),
            ("wrong-type", "request.source.attemptNumber", lambda d: d["source"].update(attemptNumber=True)),
            ("out-of-range", "request.cursor", lambda d: d.update(cursor=0)),
            ("out-of-range", "request.cursor", lambda d: d.update(cursor=-1)),
            ("out-of-range", "request.source.automationRevision", lambda d: d["source"].update(automationRevision=2**53)),
        ]
        for code, detail, edit in vectors:
            with self.subTest(code=code, detail=detail):
                self.refused(Denied, code, detail, self.adoption, self.changed(edit))
        for detail, old, new in (
            ("request.source.occurrenceFenceGeneration",
             b'"occurrenceFenceGeneration": 3,', b'"occurrenceFenceGeneration": 3.0,'),
            ("request.source.automationRevision", b'"automationRevision": 7,', b'"automationRevision": 7e0,'),
            ("request.cursor", b'"cursor": 1,', b'"cursor": 1e400,'),
        ):
            with self.subTest(detail=detail, new=new):
                self.refused(Denied, "wrong-type", detail, self.adoption, self.spliced(old, new))
        self.assertEqual(self.count(), 0)

    def test_trusted_projection_must_be_well_formed(self):
        vectors = [
            ("wrong-type verified.revoked", dict(revoked=0)),
            ("wrong-type verified.revoked", dict(revoked=1)),
            ("wrong-type verified.revoked", dict(revoked="false")),
            ("wrong-type verified.revoked", dict(revoked=None)),
            ("wrong-type verified.granted", dict(granted="artifact.read")),
            ("wrong-type verified.granted", dict(granted={"artifact.read": True})),
            ("wrong-type verified.granted[]", dict(granted=[1])),
            ("wrong-type verified.validUntil", dict(validUntil=None)),
            ("unknown-field verified.note", dict(note="x")),
            ("wrong-type verified", None),
        ]
        for detail, edits in vectors:
            with self.subTest(detail=detail):
                verified = [] if edits is None else dict(self.verified, **edits)
                self.refused(Denied, "verified-input-invalid", detail, self.adoption, verified=verified)
        for name in VERIFIED:
            with self.subTest(missing=name):
                verified = {key: value for key, value in self.verified.items() if key != name}
                self.refused(Denied, "verified-input-invalid", f"missing-field verified.{name}",
                             self.adoption, verified=verified)
        self.assertEqual(self.count(), 0)


if __name__ == "__main__":
    unittest.main()
