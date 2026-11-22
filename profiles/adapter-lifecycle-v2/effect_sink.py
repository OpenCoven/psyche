"""Parent-owned synthetic effect for lifecycle regression tests. Appending the row is the effect."""
import sqlite3
import uuid
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Binding:
    request_id: str
    request_digest: str
    attempt_id: str
    task_id: str
    action_id: str
    approval_id: str
    approval_digest: str


@dataclass(frozen=True)
class Effect:
    execution_id: str
    binding: Binding


class EffectSink:
    def __init__(self, path):
        self.path = Path(path).resolve()

    def initialize(self):
        # Only the scenario owner creates the database. Reopen and observation must
        # never turn loss of an existing ledger into a fresh zero-count ledger.
        with self.path.open("xb"):
            pass
        with closing(self.connect("rw")) as db, db:
            db.execute("""CREATE TABLE effects (
                seq INTEGER PRIMARY KEY, execution_id TEXT NOT NULL UNIQUE,
                request_id TEXT NOT NULL, request_digest TEXT NOT NULL,
                attempt_id TEXT NOT NULL, task_id TEXT NOT NULL, action_id TEXT NOT NULL,
                approval_id TEXT NOT NULL, approval_digest TEXT NOT NULL
            )""")

    def connect(self, mode):
        return sqlite3.connect(self.path.as_uri() + "?mode=" + mode, uri=True)

    def invoke(self, binding):
        execution_id = str(uuid.uuid4())
        with closing(self.connect("rw")) as db, db:
            db.execute("""INSERT INTO effects
                (execution_id, request_id, request_digest, attempt_id, task_id, action_id,
                 approval_id, approval_digest) VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                       (execution_id, binding.request_id, binding.request_digest, binding.attempt_id,
                        binding.task_id, binding.action_id, binding.approval_id, binding.approval_digest))
        return execution_id

    def observe(self):
        with closing(self.connect("ro")) as db:
            rows = db.execute("""SELECT execution_id, request_id, request_digest, attempt_id,
                task_id, action_id, approval_id, approval_digest FROM effects ORDER BY seq""")
            return tuple(Effect(row[0], Binding(*row[1:])) for row in rows)


class Witness:
    """Keep an independent parent-process record of every committed handler call."""
    def __init__(self, path):
        self.store = EffectSink(path)
        self.store.initialize()
        self.issued = []

    def invoke(self, binding):
        execution_id = self.store.invoke(binding)
        self.issued.append(Effect(execution_id, binding))
        return execution_id

    def observe(self):
        stored = self.store.observe()
        if tuple(self.issued) != stored:
            raise ValueError("independent_ledger_mismatch")
        return stored
