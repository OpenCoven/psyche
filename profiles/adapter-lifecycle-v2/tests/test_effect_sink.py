"""Exercise independent evidence using synthetic local records, without selecting an adapter wire format."""
import hashlib
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from dataclasses import replace
from pathlib import Path

from effect_sink import Binding, Effect, EffectSink


class SinkTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.path = Path(directory.name) / "fixture.sqlite"
        self.sink = EffectSink(self.path)
        self.sink.initialize()
        self.binding = Binding("request-A", hashlib.sha256(b"request-A").hexdigest(), "attempt-A",
                               "task-A", "action-A", "approval-A", hashlib.sha256(b"approval-A").hexdigest())

    def test_every_handler_invocation_has_a_separate_durable_id(self):
        actions = (self.binding, replace(self.binding, task_id="task-B", action_id="action-B"))
        issued = [self.sink.invoke(binding) for binding in actions for _ in range(3)]
        rows = EffectSink(self.path).observe()
        self.assertEqual(len(set(issued)), 6)
        self.assertEqual([row.execution_id for row in rows], issued)
        for binding in actions:
            self.assertEqual(sum(row.binding == binding for row in rows), 3)

    def test_concurrent_duplicate_invocations_are_both_visible(self):
        with ThreadPoolExecutor(max_workers=2) as pool:
            ids = list(pool.map(self.sink.invoke, (self.binding, self.binding)))
        self.assertEqual(len(set(ids)), 2)
        self.assertCountEqual((row.execution_id for row in self.sink.observe()), ids)

    def test_reopen_reads_original_execution_and_binding(self):
        issued = self.sink.invoke(self.binding)
        expected = (Effect(issued, self.binding),)
        for _ in range(2):
            self.assertEqual(EffectSink(self.path).observe(), expected)
        # Use a new interpreter with a read-only SQLite connection as an independent reader.
        query = ("import sqlite3, sys; db = sqlite3.connect(sys.argv[1], uri=True); "
                 "print(db.execute('SELECT execution_id FROM effects').fetchone()[0])")
        out = subprocess.check_output([sys.executable, "-I", "-c", query, self.path.as_uri() + "?mode=ro"], text=True)
        self.assertEqual(out.strip(), issued)
        self.assertEqual(self.sink.observe(), expected)

    def test_empty_and_missing_ledgers_are_distinct(self):
        self.assertEqual(self.sink.observe(), ())
        with self.assertRaises(FileExistsError):
            self.sink.initialize()
        self.path.unlink()
        with self.assertRaises(sqlite3.OperationalError):
            self.sink.observe()
        with self.assertRaises(sqlite3.OperationalError):
            self.sink.invoke(self.binding)
        self.assertFalse(self.path.exists())


if __name__ == "__main__":
    unittest.main()
