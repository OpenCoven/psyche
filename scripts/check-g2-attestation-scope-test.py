#!/usr/bin/env python3
"""Focused regression tests for the retained G2 attestation scope."""

from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER_PATH = ROOT / "scripts/check-g2-evidence.py"


def load_checker():
    spec = importlib.util.spec_from_file_location("check_g2_evidence_scope", CHECKER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {CHECKER_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class G2AttestationScopeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.checker = load_checker()
        cls.scope = cls.checker._read_scope_manifest(ROOT)
        cls.blob_a = ("100644", "blob", "a" * 40)
        cls.blob_b = ("100644", "blob", "b" * 40)
        cls.exec_a = ("100755", "blob", "c" * 40)
        cls.exec_b = ("100755", "blob", "d" * 40)

    def assert_rejected(self, before, after, pattern="retained G2 attestation"):
        with self.assertRaisesRegex(self.checker.EvidenceError, pattern):
            self.checker._validate_attested_changes(before, after, self.scope)

    def compare_response(self, *, status="diverged", merge_base=None, files=None):
        tested = "1" * 40
        terminal = "2" * 40
        return {
            "status": status,
            "ahead_by": 1,
            "total_commits": 1,
            "base_commit": {"sha": tested},
            "merge_base_commit": {"sha": merge_base or ("3" * 40)},
            "commits": [{"sha": terminal}],
            "files": files or [
                {"filename": "docs/G2-SCOPE-EVIDENCE.md", "status": "modified"},
                {"filename": "README.md", "status": "added"},
            ],
        }, tested, terminal

    def test_repository_scope_manifest_is_exact_and_fail_closed(self):
        self.assertEqual(set(self.scope), set(self.checker.EXPECTED_UNATTESTED_PATHS))
        self.assertNotIn("crates/psyche-core/src/contracts/mod.rs", self.scope)
        self.assertNotIn(".github/workflows/ci.yml", self.scope)
        self.assertNotIn("scripts/check-g2-evidence.py", self.scope)
        self.assertNotIn("docs/G2-EVIDENCE.md", self.scope)

    def test_scope_manifest_rejects_duplicate_json_keys(self):
        with self.assertRaisesRegex(self.checker.EvidenceError, "repeats a key"):
            self.checker._json_object_without_duplicates([("default", "protected"), ("default", "open")])

    def candidate_evidence(self):
        markdown = (ROOT / self.checker.SCOPE_EVIDENCE_PATH).read_text(encoding="utf-8")
        for name, value in (
            ("Status", "candidate"),
            ("Tested source commit", "not recorded before remote review"),
            ("CI attestation", "not recorded before remote review"),
        ):
            pattern = rf"^\*\*{name}:\*\* .+$"
            markdown, count = re.subn(pattern, f"**{name}:** {value}", markdown, flags=re.MULTILINE)
            self.assertEqual(count, 1)
        return markdown

    def test_repository_scope_evidence_is_valid(self):
        self.checker._load_scope_state(ROOT)

    def test_candidate_scope_evidence_is_explicit(self):
        markdown = self.candidate_evidence()
        status, tested, run_url = self.checker._validate_scope_evidence(markdown)
        self.assertEqual((status, tested, run_url), ("candidate", None, None))

    def passed_evidence(self):
        markdown = self.candidate_evidence()
        passed = markdown.replace("**Status:** candidate", "**Status:** passed")
        passed = passed.replace(
            "**Tested source commit:** not recorded before remote review",
            f"**Tested source commit:** {'1' * 40}",
        )
        passed = passed.replace(
            "**CI attestation:** not recorded before remote review",
            "**CI attestation:** https://github.com/OpenCoven/psyche/actions/runs/123456",
        )
        return passed

    def test_passed_scope_evidence_requires_immutable_source_and_run(self):
        status, tested, run_url = self.checker._validate_scope_evidence(self.passed_evidence())
        self.assertEqual(status, "passed")
        self.assertEqual(tested, "1" * 40)
        self.assertEqual(run_url, "https://github.com/OpenCoven/psyche/actions/runs/123456")

    def test_repository_requires_immutable_passed_historic_evidence_in_both_scope_states(self):
        historic_path = self.checker.HISTORIC_EVIDENCE_PATH
        historic = (ROOT / historic_path).read_text(encoding="utf-8")
        for scope in (self.candidate_evidence(), self.passed_evidence()):
            for changed in (historic.replace("**Status:** passed", "**Status:** candidate"), historic + "\n"):
                with self.subTest(scope=self.checker.field(scope, "Status")), self.assertRaisesRegex(
                    self.checker.EvidenceError, "immutable passed attestation"
                ):
                    self.checker.validate_repository(
                        ROOT,
                        source_overrides={historic_path: changed, self.checker.SCOPE_EVIDENCE_PATH: scope},
                    )

    def test_attesting_jobs_check_out_the_source_revision(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        jobs = re.split(r"^  [\w-]+:\s*$", workflow.split("\njobs:\n", 1)[1], flags=re.MULTILINE)[1:]
        self.assertEqual(len(jobs), 4)
        for job in jobs:
            self.assertIn(
                "      - uses: actions/checkout@v4\n"
                "        with:\n"
                "          ref: ${{ github.event.pull_request.head.sha || github.sha }}\n",
                job,
            )

    def test_allowed_modifications_preserve_mode_and_object_type(self):
        for old in (self.exec_a, ("120000", "blob", "a" * 40), ("160000", "commit", "a" * 40)):
            with self.subTest(old=old):
                self.assert_rejected({"README.md": old}, {"README.md": self.blob_b})
        self.assert_rejected({"scripts/agent-check": self.blob_a}, {"scripts/agent-check": self.exec_b})

    def test_scope_evidence_only_change_is_allowed(self):
        path = self.checker.SCOPE_EVIDENCE_PATH
        changes = self.checker._validate_attested_changes(
            {path: self.blob_a},
            {path: self.blob_b},
            self.scope,
        )
        self.assertEqual(changes, [(path, "modified")])

    def test_explicit_readiness_additions_and_modifications_are_allowed(self):
        before = {
            "README.md": self.blob_a,
            "scripts/agent-check": self.exec_a,
        }
        after = {
            "README.md": self.blob_b,
            "AGENTS.md": self.blob_a,
            "scripts/agent-check": self.exec_b,
            "scripts/agent-bootstrap": self.exec_a,
        }
        changes = self.checker._validate_attested_changes(before, after, self.scope)
        self.assertEqual(
            changes,
            [
                ("AGENTS.md", "added"),
                ("README.md", "modified"),
                ("scripts/agent-bootstrap", "added"),
                ("scripts/agent-check", "modified"),
            ],
        )

    def test_unknown_and_g2_protected_paths_are_rejected(self):
        paths = (
            "new-top-level.md",
            "crates/psyche-core/src/contracts/mod.rs",
            "crates/psyche-store/tests/migrations.rs",
            ".github/workflows/ci.yml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "scripts/check-g2-evidence.py",
            "scripts/check-g2-evidence-test.py",
            "scripts/g2-test-manifest.json",
            "scripts/g2-attestation-scope.json",
            "docs/G2-EVIDENCE.md",
            "docs/ARCHITECTURE.md",
            "docs/SCHEMAS.md",
            "docs/TESTING.md",
        )
        for path in paths:
            with self.subTest(path=path):
                self.assert_rejected({path: self.blob_a}, {path: self.blob_b})

    def test_safe_path_deletion_mode_change_symlink_and_submodule_are_rejected(self):
        self.assert_rejected({"README.md": self.blob_a}, {})
        self.assert_rejected(
            {"README.md": self.blob_a},
            {"README.md": ("100755", "blob", "b" * 40)},
        )
        self.assert_rejected(
            {},
            {"README.md": ("120000", "blob", "b" * 40)},
        )
        self.assert_rejected(
            {},
            {"README.md": ("160000", "commit", "b" * 40)},
        )

    def test_rename_is_rejected_as_a_safe_path_deletion(self):
        self.assert_rejected(
            {"README.md": self.blob_a},
            {"AGENTS.md": self.blob_a},
        )

    def test_remote_tree_rejects_truncation_submodules_and_duplicate_paths(self):
        base = {
            "sha": "a" * 40,
            "truncated": True,
            "tree": [],
        }
        with self.assertRaisesRegex(self.checker.EvidenceError, "truncated"):
            self.checker._entry_map_from_remote_tree(base, "a" * 40)

        submodule = {
            "sha": "a" * 40,
            "truncated": False,
            "tree": [
                {"path": "README.md", "mode": "160000", "type": "commit", "sha": "b" * 40},
            ],
        }
        entries = self.checker._entry_map_from_remote_tree(submodule, "a" * 40)
        self.assert_rejected({}, entries)

        duplicate = {
            "sha": "a" * 40,
            "truncated": False,
            "tree": [
                {"path": "README.md", "mode": "100644", "type": "blob", "sha": "b" * 40},
                {"path": "README.md", "mode": "100644", "type": "blob", "sha": "c" * 40},
            ],
        }
        with self.assertRaisesRegex(self.checker.EvidenceError, "duplicate"):
            self.checker._entry_map_from_remote_tree(duplicate, "a" * 40)

    def test_compare_requires_tree_scope_for_nonancestor_safe_changes(self):
        compare, tested, terminal = self.compare_response()
        before = {self.checker.SCOPE_EVIDENCE_PATH: self.blob_a}
        after = {self.checker.SCOPE_EVIDENCE_PATH: self.blob_b, "README.md": self.blob_a}
        self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, before, after)

    def test_squashed_candidate_uses_both_endpoint_trees(self):
        path = "scripts/check-g2-evidence.py"
        compare, tested, terminal = self.compare_response(files=[
            {"filename": path, "status": "modified"},
            {"filename": self.checker.SCOPE_EVIDENCE_PATH, "status": "added"},
            {"filename": "README.md", "status": "added"},
        ])
        before = {path: self.blob_a, self.checker.SCOPE_EVIDENCE_PATH: self.blob_a}
        after = {**before, self.checker.SCOPE_EVIDENCE_PATH: self.blob_b, "README.md": self.blob_b}
        self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, before, after)
        after[path] = self.blob_b
        with self.assertRaises(self.checker.EvidenceError):
            self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, before, after)

    def test_post_squash_reviewed_file_removal_move_and_copy_are_inherited(self):
        removed, added = "docs/previous-name.md", "docs/current-name.md"
        for operation in ("removed", "renamed", "copied"):
            with self.subTest(operation=operation):
                entry = {"filename": removed, "status": operation}
                before = {}
                if operation in {"renamed", "copied"}:
                    entry.update(filename=added, previous_filename=removed)
                    before[added] = self.blob_a
                if operation == "copied":
                    before[removed] = self.blob_a
                compare, tested, terminal = self.compare_response(files=[
                    entry, {"filename": "README.md", "status": "added"},
                ])
                after = {**before, "README.md": self.blob_b}
                self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, before, after)
                # An actual endpoint change at either source or destination still invalidates the proof.
                after[removed] = self.blob_b
                with self.assertRaises(self.checker.EvidenceError):
                    self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, before, after)

    def test_compare_keeps_original_evidence_only_ancestor_contract(self):
        tested = "1" * 40
        terminal = "2" * 40
        compare = {
            "status": "ahead",
            "ahead_by": 1,
            "total_commits": 1,
            "base_commit": {"sha": tested},
            "merge_base_commit": {"sha": tested},
            "commits": [{"sha": terminal}],
            "files": [{"filename": "docs/G2-EVIDENCE.md", "status": "modified"}],
        }
        self.checker.validate_evidence_only_compare(compare, tested, terminal)
        with self.assertRaisesRegex(self.checker.EvidenceError, "does not cover"):
            self.checker._validate_scope_relationship(compare, tested, terminal, self.scope, {}, {})

    def test_nonancestor_legacy_evidence_only_compare_fails_closed(self):
        compare, tested, terminal = self.compare_response(
            files=[{"filename": "docs/G2-EVIDENCE.md", "status": "modified"}]
        )
        with self.assertRaisesRegex(self.checker.EvidenceError, "ancestor"):
            self.checker.validate_evidence_only_compare(compare, tested, terminal)

    def test_compare_rejects_copy_rename_truncation_and_protected_files(self):
        mutations = (
            [{"filename": "README.md", "status": "copied"}],
            [{"filename": "README.md", "status": "renamed"}],
            [{"filename": "src/main.rs", "status": "modified"}],
            [{"filename": f"safe-{index}.md", "status": "added"} for index in range(300)],
        )
        for files in mutations:
            compare, tested, terminal = self.compare_response(files=files)
            with self.subTest(status=files[0]["status"], count=len(files)):
                with self.assertRaises(self.checker.EvidenceError):
                    self.checker._validate_scope_relationship(
                        compare, tested, terminal, self.scope, {}, {}
                    )

    def test_path_parser_rejects_absolute_parent_backslash_and_directory_paths(self):
        for path in ("/README.md", "../README.md", "docs\\README.md", "docs/"):
            with self.subTest(path=path):
                with self.assertRaises(self.checker.EvidenceError):
                    self.checker._safe_repository_path(path)


class ReadinessDeliveryTests(unittest.TestCase):
    """Exercise the public relationship checks across a complete readiness delivery."""

    @classmethod
    def setUpClass(cls):
        cls.checker = load_checker()
        cls.scope = cls.checker._read_scope_manifest(ROOT)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = pathlib.Path(temporary.name) / "checkout"
        self.root.mkdir()
        self.event_path = self.root.parent / "event.json"
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Scope fixture")
        self.git("config", "user.email", "scope@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        evidence = self.root / self.checker.SCOPE_EVIDENCE_PATH
        evidence.parent.mkdir()
        evidence.write_text("candidate\n", encoding="utf-8")
        self.pinned = self.commit("Candidate without readiness files")
        evidence.write_text("passed\n", encoding="utf-8")
        self.commit("Attest candidate")

    def git(self, *arguments):
        return subprocess.check_output(
            ["git", *arguments], cwd=self.root, encoding="utf-8", stderr=subprocess.PIPE
        ).strip()

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-qm", message)
        return self.git("rev-parse", "HEAD")

    def api(self, command, root):
        self.assertEqual(root, self.root)
        endpoint = command[-1].removeprefix("repos/OpenCoven/psyche/")
        if endpoint == f"compare/{self.pinned}...{self.terminal}":
            commits = self.git("rev-list", "--reverse", f"{self.pinned}..{self.terminal}").splitlines()
            changes = self.git("diff", "--name-status", "--no-renames", self.pinned, self.terminal)
            statuses = {"A": "added", "M": "modified", "D": "removed"}
            return {
                "status": "ahead",
                "ahead_by": len(commits),
                "total_commits": len(commits),
                "base_commit": {"sha": self.pinned},
                "merge_base_commit": {"sha": self.pinned},
                "commits": [{"sha": commit} for commit in commits],
                "files": [
                    {"filename": path, "status": statuses[status]}
                    for status, path in (line.split("\t") for line in changes.splitlines())
                ],
            }
        if endpoint == f"commits/{self.squash}/pulls":
            return [dict(self.pull, state="closed", merged_at="2026-01-01T00:00:00Z",
                         merge_commit_sha=self.squash)]
        if endpoint.startswith("git/commits/"):
            commit = endpoint.removeprefix("git/commits/")
            tree, *parents = self.git("show", "-s", "--format=%T %P", commit).split()
            return {"sha": commit, "tree": {"sha": tree}, "parents": [{"sha": p} for p in parents]}
        self.fail(f"Unexpected GitHub request: {command}")

    def assert_delivery(self, operation):
        readme = self.root / "README.md"
        if operation != "first addition":
            readme.write_text(
                "Install the pinned Rust toolchain.\n"
                "Run the repository bootstrap script.\n"
                "Check formatting before submitting.\n"
                "Verify the retained source evidence.\n",
                encoding="utf-8",
            )
            self.commit("Publish initial readiness guide")
        delivery_base = self.git("rev-parse", "HEAD")
        self.git("switch", "-qc", "readiness")
        if operation in {"first addition", "edit"}:
            readme.write_text("Updated guide\n", encoding="utf-8")
            self.commit("Write guide")
        elif operation == "delete":
            readme.unlink()
            self.commit("Remove guide")
        elif operation in {"copy", "copy with edits"}:
            contents = readme.read_bytes()
            if operation == "copy with edits":
                contents = contents.replace(b"Check formatting before submitting.",
                                            b"Include the affected crate in the review.")
            (self.root / "CONTRIBUTING.md").write_bytes(contents)
            self.commit("Publish contributor guide")
        # A second commit changes a different path from the operation under test.
        (self.root / "AGENTS.md").write_text("Contributor guide\n", encoding="utf-8")
        self.terminal = self.commit("Publish contributor notes")
        # Check the Git classification independently of the verifier. The source
        # exists only in the delivered baseline, never in the pinned source.
        delivery_changes = self.git(
            "diff", "--name-status", "--find-copies-harder", "--find-copies=50%",
            "--find-renames=50%", delivery_base, self.terminal,
        ).splitlines()
        if operation == "copy":
            self.assertIn("C100\tREADME.md\tCONTRIBUTING.md", delivery_changes)
        elif operation == "copy with edits":
            self.assertRegex("\n".join(delivery_changes),
                             r"(?m)^C0[5-9][0-9]\tREADME\.md\tCONTRIBUTING\.md$")
        elif operation == "first addition":
            self.assertIn("A\tREADME.md", delivery_changes)
        self.assertIn("A\tAGENTS.md", delivery_changes)
        self.git("switch", "-q", "main")
        (self.root / "LICENSE").write_text("License on a parallel main change\n", encoding="utf-8")
        main_tip = self.commit("Main advanced after the topic branched")
        self.git("update-ref", "refs/remotes/origin/main", main_tip)
        self.git("switch", "-q", "readiness")
        repository = {"full_name": "OpenCoven/psyche"}
        self.pull = {
            "base": {"ref": "main", "sha": main_tip, "repo": repository},
            "head": {"sha": self.terminal, "repo": repository},
        }
        self.squash = self.git(
            "commit-tree", self.git("rev-parse", f"{self.terminal}^{{tree}}"),
            "-p", delivery_base, "-m", "Squash readiness PR",
        )
        # Only the retained state and the remote API are fixture data. The
        # checkout, merge bases, trees and per-delivery changes use real Git.
        with mock.patch.object(
            self.checker, "_scope_relationship_for_actual_evidence",
            return_value=(self.scope, "passed", self.pinned),
        ), mock.patch.object(
            self.checker, "_remote_tree_entries", side_effect=self.checker._local_tree_entries,
        ), mock.patch.object(self.checker, "run_json", side_effect=self.api), mock.patch.dict(
            os.environ, {"GITHUB_EVENT_PATH": str(self.event_path)},
        ):
            events = (
                ("pull_request", {"repository": repository, "pull_request": self.pull}),
                ("push", {"repository": repository, "ref": "refs/heads/main",
                          "before": delivery_base, "after": self.squash}),
            )
            for event_name, event in events:
                if event_name == "push":
                    # Model a squash onto the actual previous main. The parallel
                    # tip above is specific to the open-PR merge-base case.
                    self.pull["base"] = {"ref": "main", "sha": delivery_base, "repo": repository}
                    self.git("branch", "-f", "main", self.squash)
                    self.git("switch", "-q", "main")
                    self.git("update-ref", "refs/remotes/origin/main", self.squash)
                self.event_path.write_text(json.dumps(event), encoding="utf-8")
                for verify in (self.checker.verify_local_source_relationship,
                               self.checker.verify_actions_source_relationship):
                    with self.subTest(event=event_name, entrypoint=verify.__name__):
                        denied_path = {
                            "delete": r"README\.md",
                            "copy": r"CONTRIBUTING\.md",
                            "copy with edits": r"CONTRIBUTING\.md",
                        }.get(operation)
                        if denied_path:
                            with self.assertRaisesRegex(self.checker.EvidenceError, denied_path):
                                verify(self.root, self.pinned)
                        else:
                            verify(self.root, self.pinned)

    def test_initial_readiness_addition(self):
        self.assert_delivery("first addition")

    def test_existing_readiness_content_edit(self):
        self.assert_delivery("edit")

    def test_existing_readiness_file_unchanged(self):
        self.assert_delivery("unchanged")

    def test_prior_readiness_file_removed_earlier_in_the_delivery(self):
        self.assert_delivery("delete")

    def test_unchanged_readiness_file_copied_to_another_allowed_path(self):
        self.assert_delivery("copy")

    def test_git_reports_edited_copy_from_unchanged_readiness_file(self):
        self.assert_delivery("copy with edits")


if __name__ == "__main__":
    unittest.main(verbosity=2)
