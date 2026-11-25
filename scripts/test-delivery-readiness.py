"""Focused tests for the trusted decision and the two delivery events."""

import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("delivery", Path(__file__).with_name("delivery-readiness.py"))
delivery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(delivery)

HEAD = "a" * 40
PARENT = "b" * 40
REPO = {"full_name": delivery.REPO}
RUN = dict(head_sha=HEAD, head_branch="topic", workflow_id=delivery.g2.CI_WORKFLOW_ID,
           repository=REPO, head_repository=REPO, path=".github/workflows/ci.yml",
           event="pull_request", status="completed", conclusion="success")
PULL = {"base": {"ref": "main", "repo": REPO, "sha": PARENT}, "head": {"sha": HEAD, "repo": REPO}}


class ReadinessTests(unittest.TestCase):
    def test_pull_request_uses_exact_head(self):
        with patch.object(delivery, "api", side_effect=[[PULL], {"merge_base_commit": {"sha": PARENT}}]):
            self.assertEqual(delivery.review_head(RUN), (PARENT, HEAD))

    def test_unrelated_pull_cannot_qualify(self):
        with patch.object(delivery, "api", return_value=[dict(PULL, head=dict(PULL["head"], sha=PARENT))]):
            with self.assertRaises(delivery.g2.EvidenceError):
                delivery.review_head(RUN)

    def test_push_requires_reviewed_squash_tree(self):
        with patch.object(delivery, "api", return_value={"parents": [{"sha": PARENT}]}):
            with patch.object(delivery.g2, "squash_merge_terminal", return_value=PARENT) as squash:
                self.assertEqual(delivery.review_head(dict(RUN, event="push", head_branch="main")), (PARENT, PARENT))
                squash.assert_called_once_with(delivery.ROOT, dict(ref="refs/heads/main", before=PARENT, after=HEAD))

    def test_other_origins_and_unsuccessful_ci_fail(self):
        for change in (dict(workflow_id=0), dict(conclusion="failure"), dict(event="workflow_dispatch"),
                       dict(event="push"), dict(head_repository={"full_name": "another/repo"})):
            with self.subTest(change=change), self.assertRaises(delivery.g2.EvidenceError):
                delivery.review_head(dict(RUN, **change))

    def test_new_attestation_is_an_evidence_only_child(self):
        key = delivery.g2.SCOPE_EVIDENCE_PATH
        source, passed = {key: "candidate"}, {key: "passed"}
        with patch.object(delivery, "api", return_value={"parents": [{"sha": PARENT}]}):
            delivery.verify_transition({}, source, passed, PARENT, HEAD)
            with self.assertRaises(delivery.g2.EvidenceError):
                delivery.verify_transition({}, source, dict(passed, README="addition"), PARENT, HEAD)
            with self.assertRaises(delivery.g2.EvidenceError):
                delivery.verify_transition({}, source, passed, HEAD, PARENT)
        delivery.verify_transition(passed, source, dict(passed, README="addition"), PARENT, HEAD)

    def test_missing_jobs_are_refused(self):
        with patch.object(delivery, "api", return_value={"jobs": [], "total_count": 0}):
            with self.assertRaises(delivery.g2.EvidenceError):
                delivery.verify_job_coverage(1)

    def test_green_candidate_is_refused_before_remote_attestation(self):
        checked = (delivery.ROOT / delivery.g2.SCOPE_EVIDENCE_PATH).read_text()
        candidate = []
        for line in checked.splitlines():
            if line.startswith("**Status:"):
                line = "**Status:** candidate"
            elif line.startswith(("**Tested source commit:", "**CI attestation:")):
                line = line.partition("** ")[0] + "** not recorded before remote review"
            candidate.append(line)
        with patch.object(delivery, "review_head", return_value=(PARENT, HEAD)), \
             patch.object(delivery.g2, "_remote_tree_entries", return_value={}), \
             patch.object(delivery, "read_blob", return_value="\n".join(candidate)), \
             self.assertRaisesRegex(delivery.g2.EvidenceError, "Qualification candidate"):
            delivery.evaluate(RUN)


if __name__ == "__main__":
    unittest.main()
