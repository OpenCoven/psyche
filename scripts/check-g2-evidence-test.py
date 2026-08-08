#!/usr/bin/env python3
"""Mutation tests for the G2 evidence relationship checker."""

from __future__ import annotations

import copy
import importlib.util
import json
import pathlib
import subprocess
import sys
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER_PATH = ROOT / "scripts/check-g2-evidence.py"
sys.dont_write_bytecode = True


def load_checker():
    spec = importlib.util.spec_from_file_location("check_g2_evidence", CHECKER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {CHECKER_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class G2EvidenceCheckerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.checker = load_checker()
        cls.manifest = json.loads((ROOT / "scripts/g2-test-manifest.json").read_text())
        cls.evidence = (ROOT / "docs/G2-EVIDENCE.md").read_text()
        cls.listed = {
            target: "".join(f"{name}: test\n" for name in definition["tests"])
            for target, definition in cls.manifest["targets"].items()
        }

    def assert_valid(
        self,
        *,
        manifest=None,
        evidence=None,
        listed=None,
        overrides=None,
    ) -> None:
        self.checker.validate_repository(
            ROOT,
            manifest=manifest or self.manifest,
            evidence=evidence or self.evidence,
            listed_tests=listed or self.listed,
            source_overrides=overrides or {},
            verify_remote=False,
        )

    def assert_rejected(self, *, hash_only: bool = False, **kwargs) -> None:
        overrides = kwargs.get("overrides")
        if not hash_only and isinstance(overrides, dict) and ".github/workflows/ci.yml" in overrides:
            self.assert_structure_rejected(overrides[".github/workflows/ci.yml"])
        with self.assertRaises(self.checker.EvidenceError):
            self.assert_valid(**kwargs)

    def assert_structure_rejected(self, workflow: str) -> None:
        with self.assertRaises(self.checker.EvidenceError):
            self.checker.validate_ci_structure(workflow)

    def passed_evidence(self) -> str:
        run_url = "https://github.com/OpenCoven/psyche/actions/runs/123456"
        passed = self.evidence.replace("**Status:** candidate", "**Status:** passed")
        passed = passed.replace(
            "**Tested source commit:** not recorded before remote review",
            "**Tested source commit:** 0123456789abcdef0123456789abcdef01234567",
        )
        passed = passed.replace(
            "**CI attestation:** not recorded before remote review",
            f"**CI attestation:** {run_url}",
        )
        passed = passed.replace(
            "**Coven plan source commit:** not recorded before plan approval",
            f"**Coven plan source commit:** {self.checker.APPROVED_PLAN_COMMIT}",
        )
        passed = passed.replace(
            "**Coven plan URL:** not recorded before plan approval",
            "**Coven plan URL:** "
            f"https://github.com/OpenCoven/coven/blob/{self.checker.APPROVED_PLAN_COMMIT}/"
            f"{self.checker.PLAN_PATH}",
        )
        passed = passed.replace(
            "**Coven plan SHA-256:** not recorded before plan approval",
            f"**Coven plan SHA-256:** sha256:{self.checker.APPROVED_PLAN_SHA256}",
        )
        return passed.replace("not run remotely | none", f"passed | {run_url}")

    def test_valid_exact_manifest_and_candidate_evidence(self) -> None:
        self.assert_valid()

    def test_delivery_v1_documentation_rejects_field_order_mutation(self) -> None:
        path = "docs/SCHEMAS.md"
        schemas = (ROOT / path).read_text()
        mutated = schemas.replace(
            "`effect`, `effect_digest`, `surface_decision`",
            "`effect_digest`, `effect`, `surface_decision`",
            1,
        )
        self.assertNotEqual(mutated, schemas)
        self.assert_rejected(overrides={path: mutated})

    def test_zero_listed_tests_is_rejected(self) -> None:
        listed = dict(self.listed)
        listed["psyche-core/contracts"] = ""
        self.assert_rejected(listed=listed)

    def test_missing_manifest_name_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["targets"]["psyche-core/contracts"]["tests"][0] = "not_a_real_test"
        self.assert_rejected(manifest=manifest)

    def test_substring_filter_without_exact_is_rejected(self) -> None:
        evidence = self.evidence.replace(
            "-- --exact delivery_keeps_the_canonical_del_prefix",
            "-- delivery_keeps_the_canonical_del_prefix",
            1,
        )
        self.assert_rejected(evidence=evidence)

    def test_unused_manifest_entry_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["targets"]["psyche-core/contracts"]["tests"].append("unused_test")
        listed = dict(self.listed)
        listed["psyche-core/contracts"] += "unused_test: test\n"
        self.assert_rejected(manifest=manifest, listed=listed)

    def test_duplicate_matrix_row_is_rejected(self) -> None:
        row = next(line for line in self.evidence.splitlines() if line.startswith("| Complete canonical error enum |"))
        self.assert_rejected(evidence=self.evidence.replace(row + "\n", row + "\n" + row + "\n"))

    def test_relative_or_mutable_coven_url_is_rejected(self) -> None:
        immutable = "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/PLAN.md"
        for replacement in ("../specs/psyche/PLAN.md", immutable.replace("42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad", "main")):
            with self.subTest(replacement=replacement):
                self.assert_rejected(evidence=self.evidence.replace(immutable, replacement))

    def test_coven_source_sha256_mismatch_is_rejected(self) -> None:
        self.assert_rejected(
            evidence=self.evidence.replace(
                "sha256:01382f8a-0d2bca95-ddd53563-4dd6a9f0-9ac4a80d-588ccbeb-d72f163e-af56bc1e",
                "sha256:11382f8a-0d2bca95-ddd53563-4dd6a9f0-9ac4a80d-588ccbeb-d72f163e-af56bc1e",
            )
        )

    def test_every_c_s_wrapper_and_evidence_row_is_required(self) -> None:
        conformance_path = "crates/psyche-test-support/tests/conformance.rs"
        source = (ROOT / conformance_path).read_text()
        for number in range(1, 13):
            wrapper = f"c_s{number}_"
            with self.subTest(wrapper=wrapper):
                self.assert_rejected(overrides={conformance_path: source.replace(wrapper, f"removed_c_s{number}_")})
            row = next(
                line
                for line in self.evidence.splitlines()
                if line.startswith(f"| C-S{number} scripted ")
            )
            with self.subTest(row=number):
                self.assert_rejected(evidence=self.evidence.replace(row + "\n", ""))

    def test_expected_unsupported_never_counts_as_passed(self) -> None:
        passed = self.passed_evidence().replace(
            "passed | https://github.com/OpenCoven/psyche/actions/runs/123456",
            "ExpectedUnsupported | https://github.com/OpenCoven/psyche/actions/runs/123456",
            1,
        )
        with self.assertRaisesRegex(self.checker.EvidenceError, "every passed matrix result"):
            self.assert_valid(evidence=passed)

    def test_every_scalar_evidence_field_must_occur_exactly_once(self) -> None:
        labels = (
            "Status",
            "Tested source commit",
            "CI attestation",
            "Coven plan source commit",
            "Coven plan URL",
            "Coven plan SHA-256",
            "Coven specification source commit",
        )
        for label in labels:
            line = next(line for line in self.evidence.splitlines() if line.startswith(f"**{label}:**"))
            for suffix in (line, f"**{label}:** conflicting-value"):
                with self.subTest(label=label, duplicate=suffix == line):
                    self.assert_rejected(evidence=self.evidence.replace(line + "\n", line + "\n" + suffix + "\n", 1))

    def test_commented_ci_command_does_not_count_as_an_active_run(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        command = "cargo test -p psyche-test-support --test conformance"
        mutated = workflow.replace(f"        run: {command}", f"        # run: {command}", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_fixed_seed_ci_step_requires_portable_env_mapping(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        self.assertIn('        env:\n          PROPTEST_CASES: "2048"\n', workflow)
        self.assertIn('          PROPTEST_RNG_SEED: "00000000000000000000000000000000"\n', workflow)
        self.assertIn("        run: cargo test -p psyche-test-support --test state_machine\n", workflow)
        mutated = workflow.replace('          PROPTEST_CASES: "2048"\n', "", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_ci_step_rejects_if_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: G2 reusable conformance\n"
        mutated = workflow.replace(anchor, anchor + "        if: ${{ false }}\n", 1)
        self.assert_rejected(overrides={path: mutated})
        commented = workflow.replace(anchor, anchor + "        # if: ${{ false }}\n", 1)
        self.checker.validate_ci_structure(commented)

    def test_required_ci_step_rejects_continue_on_error_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: G2 reusable conformance\n"
        mutated = workflow.replace(anchor, anchor + "        continue-on-error: true\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_rust_ci_job_rejects_if_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  rust:\n", "  rust:\n    if: ${{ false }}\n", 1)
        self.assert_rejected(overrides={path: mutated})
        other_job = workflow.replace("  npm:\n", "  npm:\n    if: ${{ always() }}\n", 1)
        self.checker.validate_ci_structure(other_job)

    def test_rust_ci_job_rejects_continue_on_error_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  rust:\n", "  rust:\n    continue-on-error: true\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_g2_step_must_remain_in_rust_matrix_job(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        step = (
            "      - name: G2 reusable conformance\n"
            "        run: cargo test -p psyche-test-support --test conformance\n"
        )
        mutated = workflow.replace(step, "", 1).replace("      - name: Wrapper tests\n", step + "      - name: Wrapper tests\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_supply_chain_ci_job_rejects_if_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  supply-chain:\n", "  supply-chain:\n    if: ${{ false }}\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_supply_chain_ci_job_rejects_continue_on_error_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  supply-chain:\n", "  supply-chain:\n    continue-on-error: true\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_secrets_ci_job_rejects_if_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  secrets:\n", "  secrets:\n    if: ${{ false }}\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_secrets_ci_job_rejects_continue_on_error_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("  secrets:\n", "  secrets:\n    continue-on-error: true\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_rust_ci_job_requires_active_matrix_runs_on(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("    runs-on: ${{ matrix.os }}\n", "    runs-on: ubuntu-latest\n", 1)
        self.assert_rejected(overrides={path: mutated})
        commented = workflow.replace("    runs-on: ${{ matrix.os }}\n", "    # runs-on: ubuntu-latest\n    runs-on: ${{ matrix.os }}\n", 1)
        self.checker.validate_ci_structure(commented)

    def test_rust_ci_matrix_requires_every_supported_os(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        matrix = "        os: [ubuntu-latest, macos-latest, windows-latest]\n"
        for removed in ("macos-latest", "windows-latest"):
            remaining = [os for os in ("ubuntu-latest", "macos-latest", "windows-latest") if os != removed]
            replacement = f"        os: [{', '.join(remaining)}]\n        # os: [{removed}]\n"
            with self.subTest(removed=removed):
                self.assert_rejected(overrides={path: workflow.replace(matrix, replacement, 1)})

    def test_rust_ci_matrix_rejects_include_and_exclude(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      matrix:\n"
        for key in ("include", "exclude"):
            mutation = f"        {key}:\n          - os: ubuntu-latest\n"
            with self.subTest(key=key):
                self.assert_rejected(overrides={path: workflow.replace(anchor, anchor + mutation, 1)})

    def test_rust_ci_strategy_requires_active_fail_fast_false(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("      fail-fast: false\n", "      fail-fast: true\n      # fail-fast: false\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_ci_jobs_reject_needs_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        for job in ("rust", "supply-chain", "secrets"):
            with self.subTest(job=job):
                mutated = workflow.replace(f"  {job}:\n", f"  {job}:\n    needs: npm\n", 1)
                self.assert_rejected(overrides={path: mutated})

    def test_required_ci_step_rejects_quoted_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: G2 reusable conformance\n"
        mutated = workflow.replace(anchor, anchor + '        "if": ${{ false }}\n', 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_ci_step_rejects_spaced_key(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: G2 reusable conformance\n"
        mutated = workflow.replace(anchor, anchor + "        if : ${{ false }}\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_ci_step_rejects_shell_override(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: G2 reusable conformance\n"
        mutated = workflow.replace(anchor, anchor + "        shell: echo {0}\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_ci_job_rejects_defaults_shell_override(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutations = (
            workflow.replace("  rust:\n", "  rust:\n    defaults: { run: { shell: echo {0} } }\n", 1),
            workflow.replace(
                "  rust:\n",
                "  rust:\n    defaults:\n      run:\n        shell: echo {0}\n",
                1,
            ),
        )
        for nested, mutated in enumerate(mutations):
            with self.subTest(nested=bool(nested)):
                self.assert_rejected(overrides={path: mutated})

    def test_workflow_rejects_defaults(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("jobs:\n", "defaults: { run: { shell: echo {0} } }\njobs:\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_workflow_env_rejects_command_override(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("env:\n", "env:\n  CARGO: /bin/true\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_required_workflow_regions_reject_yaml_anchors_aliases_and_merges(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutations = (
            workflow.replace("env:\n", "env: &global_env\n", 1),
            workflow.replace("  rust:\n", "  rust:\n    <<: *required_job\n", 1),
        )
        for index, mutated in enumerate(mutations):
            with self.subTest(index=index):
                self.assert_rejected(overrides={path: mutated})

    def test_workflow_rejects_noncanonical_inline_triggers(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        for trigger in ("workflow_dispatch", "push"):
            with self.subTest(trigger=trigger):
                self.assert_rejected(overrides={path: workflow.replace("on:\n", f"on: {trigger}\n", 1)})

    def test_workflow_requires_active_pull_request_trigger(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        for replacement in ("", "  pull-requests:\n"):
            with self.subTest(replacement=replacement):
                self.assert_rejected(overrides={path: workflow.replace("  pull_request:\n", replacement, 1)})

    def test_workflow_push_trigger_requires_main_branch(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("    branches: [main]\n", "    branches: [develop]\n", 1)
        self.assert_rejected(overrides={path: mutated})

    def test_workflow_hash_rejects_setup_action_drift(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        mutated = workflow.replace("dtolnay/rust-toolchain@master", "dtolnay/rust-toolchain@stable", 1)
        self.assert_rejected(hash_only=True, overrides={path: mutated})

    def test_workflow_hash_rejects_github_path_poison_step(self) -> None:
        path = ".github/workflows/ci.yml"
        workflow = (ROOT / path).read_text()
        anchor = "      - name: Format\n"
        poison = "      - name: Poison PATH\n        run: echo /tmp/poison >> $GITHUB_PATH\n"
        self.assert_rejected(hash_only=True, overrides={path: workflow.replace(anchor, poison + anchor, 1)})

    def test_passed_remote_verifier_accepts_exact_ci_attestation(self) -> None:
        passed = self.passed_evidence()
        completed = (
            subprocess.CompletedProcess([], 0, "", ""),
            subprocess.CompletedProcess([], 0, "docs/G2-EVIDENCE.md\n", ""),
        )
        run = {
            "conclusion": "success",
            "event": "pull_request",
            "headSha": "0123456789abcdef0123456789abcdef01234567",
            "url": "https://github.com/OpenCoven/psyche/actions/runs/123456",
            "workflowName": "CI",
        }
        rest = {
            "conclusion": "success",
            "event": "pull_request",
            "head_repository": {"full_name": "OpenCoven/psyche"},
            "head_sha": "0123456789abcdef0123456789abcdef01234567",
            "html_url": "https://github.com/OpenCoven/psyche/actions/runs/123456",
            "id": 123456,
            "path": ".github/workflows/ci.yml",
            "repository": {"full_name": "OpenCoven/psyche"},
            "status": "completed",
            "workflow_id": 326408880,
        }
        workflow = {
            "id": 326408880,
            "name": "CI",
            "path": ".github/workflows/ci.yml",
            "state": "active",
        }
        with mock.patch.object(self.checker.subprocess, "run", side_effect=completed), mock.patch.object(
            self.checker, "run_json", side_effect=(run, rest, workflow)
        ) as run_json, mock.patch.object(self.checker, "verify_coven_blob") as verify_blob:
            self.checker.verify_passed(ROOT, passed, self.checker.validate_evidence(passed)[1])
        self.assertEqual(run_json.call_count, 3)
        self.assertEqual(verify_blob.call_count, 6)

    def test_remote_verifier_rejects_wrong_workflow_or_event(self) -> None:
        passed = self.passed_evidence()
        baseline = {
            "conclusion": "success",
            "event": "pull_request",
            "headSha": "0123456789abcdef0123456789abcdef01234567",
            "url": "https://github.com/OpenCoven/psyche/actions/runs/123456",
            "workflowName": "CI",
        }
        for field, value in (("workflowName", "Other"), ("event", "push")):
            completed = (
                subprocess.CompletedProcess([], 0, "", ""),
                subprocess.CompletedProcess([], 0, "docs/G2-EVIDENCE.md\n", ""),
            )
            with self.subTest(field=field), mock.patch.object(
                self.checker.subprocess, "run", side_effect=completed
            ), mock.patch.object(self.checker, "run_json", return_value={**baseline, field: value}), mock.patch.object(
                self.checker, "verify_coven_blob"
            ):
                with self.assertRaisesRegex(self.checker.EvidenceError, "CI attestation"):
                    self.checker.verify_passed(ROOT, passed, self.checker.validate_evidence(passed)[1])

    def test_remote_verifier_rejects_wrong_rest_workflow_path_or_repository(self) -> None:
        passed = self.passed_evidence()
        view = {
            "conclusion": "success",
            "event": "pull_request",
            "headSha": "0123456789abcdef0123456789abcdef01234567",
            "url": "https://github.com/OpenCoven/psyche/actions/runs/123456",
            "workflowName": "CI",
        }
        rest = {
            "conclusion": "success",
            "event": "pull_request",
            "head_repository": {"full_name": "OpenCoven/psyche"},
            "head_sha": "0123456789abcdef0123456789abcdef01234567",
            "html_url": "https://github.com/OpenCoven/psyche/actions/runs/123456",
            "id": 123456,
            "path": ".github/workflows/ci.yml",
            "repository": {"full_name": "OpenCoven/psyche"},
            "status": "completed",
            "workflow_id": 326408880,
        }
        mutations = (
            {**rest, "workflow_id": 1},
            {**rest, "path": ".github/workflows/other.yml"},
            {**rest, "repository": {"full_name": "fork/psyche"}},
        )
        for index, mutated in enumerate(mutations):
            completed = (
                subprocess.CompletedProcess([], 0, "", ""),
                subprocess.CompletedProcess([], 0, "docs/G2-EVIDENCE.md\n", ""),
            )
            with self.subTest(index=index), mock.patch.object(
                self.checker.subprocess, "run", side_effect=completed
            ), mock.patch.object(self.checker, "run_json", side_effect=(view, mutated)), mock.patch.object(
                self.checker, "verify_coven_blob"
            ):
                with self.assertRaisesRegex(self.checker.EvidenceError, "REST attestation"):
                    self.checker.verify_passed(ROOT, passed, self.checker.validate_evidence(passed)[1])

    def test_remote_verifier_rejects_inactive_workflow_metadata(self) -> None:
        passed = self.passed_evidence()
        view = {
            "conclusion": "success", "event": "pull_request",
            "headSha": "0123456789abcdef0123456789abcdef01234567",
            "url": "https://github.com/OpenCoven/psyche/actions/runs/123456", "workflowName": "CI",
        }
        rest = {
            "conclusion": "success", "event": "pull_request",
            "head_repository": {"full_name": "OpenCoven/psyche"},
            "head_sha": "0123456789abcdef0123456789abcdef01234567",
            "html_url": "https://github.com/OpenCoven/psyche/actions/runs/123456", "id": 123456,
            "path": ".github/workflows/ci.yml", "repository": {"full_name": "OpenCoven/psyche"},
            "status": "completed", "workflow_id": 326408880,
        }
        workflow = {"id": 326408880, "name": "CI", "path": ".github/workflows/ci.yml", "state": "disabled_manually"}
        completed = (
            subprocess.CompletedProcess([], 0, "", ""),
            subprocess.CompletedProcess([], 0, "docs/G2-EVIDENCE.md\n", ""),
        )
        with mock.patch.object(self.checker.subprocess, "run", side_effect=completed), mock.patch.object(
            self.checker, "run_json", side_effect=(view, rest, workflow)
        ), mock.patch.object(self.checker, "verify_coven_blob"):
            with self.assertRaisesRegex(self.checker.EvidenceError, "workflow metadata"):
                self.checker.verify_passed(ROOT, passed, self.checker.validate_evidence(passed)[1])

    def test_architecture_lists_cli_direct_dependencies(self) -> None:
        architecture = (ROOT / "docs/ARCHITECTURE.md").read_text()
        self.assertIn("psyche-config <- psyche-cli", architecture)
        self.assertIn("psyche-store <- psyche-cli", architecture)
        self.assertIn("Runtime owns opening the store during startup.", architecture)
        self.assertIn("The CLI uses its direct store dependency only for doctor data-directory preparation.", architecture)
        self.assertNotIn("CLI is its outer process boundary and also reads\nconfiguration and opens the store", architecture)

    def test_c_s10_requires_every_content_reference_field(self) -> None:
        path = "crates/psyche-coven/tests/fixtures/result-bundle.json"
        fixture = json.loads((ROOT / path).read_text())
        for section in ("result", "artifacts"):
            for field in ("digest", "media_type", "size_bytes", "expires_at"):
                mutated = copy.deepcopy(fixture)
                target = mutated[section] if section == "result" else mutated[section][0]["content"]
                target.pop(field)
                with self.subTest(section=section, field=field):
                    self.assert_rejected(overrides={path: json.dumps(mutated, separators=(",", ":"))})

    def test_record_kind_identity_mutations_are_rejected(self) -> None:
        path = "crates/psyche-core/src/contracts/mod.rs"
        source = (ROOT / path).read_text()
        mutations = (
            source.replace("    Attempt,\n", "    Attempt,\n    ExecutionBinding,\n", 1),
            source.replace("SchemaKind::ExecutionBinding => Some(RecordKind::Attempt)", "SchemaKind::ExecutionBinding => Some(RecordKind::Session)", 1),
            source.replace('RecordKind::Intent => "int_"', 'RecordKind::Intent => "att_"', 1),
        )
        for index, mutation in enumerate(mutations):
            with self.subTest(index=index):
                self.assert_rejected(overrides={path: mutation})


if __name__ == "__main__":
    unittest.main(verbosity=2)
