"""Regression vectors for independent, post-run evidence. No adapter process receives the sink database."""
from dataclasses import replace
from pathlib import Path
import tempfile
import unittest

from effect_sink import Binding, Effect, EffectSink
from evidence_checks import Observation, verify_refusal, verify_terminal


class EvidenceChecks(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.sink = EffectSink(Path(tmp.name) / "witness.sqlite")
        self.sink.initialize()
        self.binding = Binding("request-A", "digest-A", "attempt-A", "task-A", "action-A", "approval-A", "approval-digest-A")
        self.receipt = b'{"state":"succeeded","actionId":"action-A"}'
        self.reply = Observation(self.binding, "succeeded", self.receipt, 1)

    def verify(self, **changes):
        args = dict(binding=self.binding, journal_state="succeeded", journal_receipt=self.receipt,
                    reply=self.reply, effects=self.sink.observe(), expected_state="succeeded")
        args.update(changes)
        return verify_terminal(**args)

    def test_independent_count_detects_six_effects_reported_as_two(self):
        actions = (self.binding, replace(self.binding, task_id="task-B", action_id="action-B"))
        for binding in actions:
            for _ in range(3):
                self.sink.invoke(binding)
        rows = self.sink.observe()
        self.assertEqual(len(rows), 6)
        for binding in actions:
            with self.subTest(binding=binding), self.assertRaisesRegex(ValueError, "^effect_count_mismatch$"):
                self.verify(binding=binding, reply=replace(self.reply, binding=binding),
                            effects=tuple(row for row in rows if row.binding == binding))

    def test_persisted_request_attempt_and_approval_match_both_sources(self):
        self.sink.invoke(self.binding)
        for field in ("request_id", "request_digest", "attempt_id", "approval_id", "approval_digest", "task_id", "action_id"):
            other = replace(self.binding, **{field: "different"})
            with self.subTest(field=field):
                with self.assertRaisesRegex(ValueError, "^native_binding_mismatch$"):
                    self.verify(reply=replace(self.reply, binding=other))
                with self.assertRaisesRegex(ValueError, "^effect_binding_mismatch$"):
                    self.verify(effects=(Effect(self.sink.observe()[0].execution_id, other),))

    def test_unknown_inputs_require_expected_refusal_and_preserved_effects(self):
        for expected in ("unsupported_version", "incompatible_downgrade", "unsupported_request"):
            with self.subTest(expected=expected):
                verify_refusal(expected, expected, (), ())
                with self.assertRaisesRegex(ValueError, "^refusal_mismatch$"):
                    verify_refusal(expected, "succeeded", (), ())

    def test_reopen_of_denied_and_cancelled_work_requires_zero_effects(self):
        for expected in ("operator_required", "action_invalidated"):
            for phase in ("replay", "rollback"):
                with self.subTest(code=expected, phase=phase):
                    verify_refusal(expected, expected, (), ())
                    with self.assertRaisesRegex(ValueError, "^refusal_changed_effects$"):
                        verify_refusal(expected, expected, (), (Effect("unexpected", self.binding),))

    def test_restart_and_rollback_preserve_original_native_ids_and_exact_bytes(self):
        original = self.sink.invoke(self.binding)
        self.assertEqual(self.verify(), original)
        self.assertEqual(self.verify(previous_id=original), original)
        for field in ("task_id", "action_id"):
            wrong = replace(self.binding, **{field: "different"})
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "^native_binding_mismatch$"):
                self.verify(binding=wrong, previous_id=original)
        with self.assertRaisesRegex(ValueError, "^execution_identity_mismatch$"):
            self.verify(previous_id="different")
        with self.assertRaisesRegex(ValueError, "^native_receipt_mismatch$"):
            self.verify(journal_receipt=self.receipt + b" ")

    def test_missing_or_inconsistent_evidence_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "^effect_count_mismatch$"):
            self.verify()
        self.sink.invoke(self.binding)
        for receipt in (None, b""):
            with self.assertRaisesRegex(ValueError, "^native_receipt_mismatch$"):
                self.verify(journal_receipt=receipt)
        with self.assertRaisesRegex(ValueError, "^terminal_state_mismatch$"):
            self.verify(expected_state="unknown")
        lost = b'{"state":"unknown","actionId":"action-A"}'
        eid = self.verify(expected_state="unknown", journal_state="unknown", journal_receipt=lost,
                          reply=replace(self.reply, state="unknown", receipt=lost))
        self.assertEqual(eid, self.sink.observe()[0].execution_id)
