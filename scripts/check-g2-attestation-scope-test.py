#!/usr/bin/env python3
"""Focused regression tests for the retained G2 attestation scope."""

from __future__ import annotations

import importlib.util
import pathlib
import unittest


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

    def test_candidate_scope_evidence_is_explicit(self):
        markdown = (ROOT / self.checker.SCOPE_EVIDENCE_PATH).read_text()
        status, tested, run_url = self.checker._validate_scope_evidence(markdown)
        self.assertEqual((status, tested, run_url), ("candidate", None, None))

    def test_passed_scope_evidence_requires_immutable_source_and_run(self):
        markdown = (ROOT / self.checker.SCOPE_EVIDENCE_PATH).read_text()
        passed = markdown.replace("**Status:** candidate", "**Status:** passed")
        passed = passed.replace(
            "**Tested source commit:** not recorded before remote review",
            f"**Tested source commit:** {'1' * 40}",
        )
        passed = passed.replace(
            "**CI attestation:** not recorded before remote review",
            "**CI attestation:** https://github.com/OpenCoven/psyche/actions/runs/123456",
        )
        status, tested, run_url = self.checker._validate_scope_evidence(passed)
        self.assertEqual(status, "passed")
        self.assertEqual(tested, "1" * 40)
        self.assertEqual(run_url, "https://github.com/OpenCoven/psyche/actions/runs/123456")

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
        self.assertTrue(
            self.checker._compare_requires_tree_scope(compare, tested, terminal, self.scope)
        )

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
        self.assertFalse(
            self.checker._compare_requires_tree_scope(compare, tested, terminal, self.scope)
        )

    def test_nonancestor_legacy_evidence_only_compare_fails_closed(self):
        compare, tested, terminal = self.compare_response(
            files=[{"filename": "docs/G2-EVIDENCE.md", "status": "modified"}]
        )
        with self.assertRaisesRegex(self.checker.EvidenceError, "ancestor"):
            self.checker._compare_requires_tree_scope(compare, tested, terminal, self.scope)

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
                    self.checker._compare_requires_tree_scope(
                        compare, tested, terminal, self.scope
                    )

    def test_path_parser_rejects_absolute_parent_backslash_and_directory_paths(self):
        for path in ("/README.md", "../README.md", "docs\\README.md", "docs/"):
            with self.subTest(path=path):
                with self.assertRaises(self.checker.EvidenceError):
                    self.checker._safe_repository_path(path)


if __name__ == "__main__":
    unittest.main(verbosity=2)
