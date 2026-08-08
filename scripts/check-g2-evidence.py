#!/usr/bin/env python3
"""Verify that G2 source, tests, CI, and review evidence remain connected."""

from __future__ import annotations

import base64
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import urllib.parse
from collections.abc import Mapping


class EvidenceError(RuntimeError):
    """A G2 evidence relationship is absent or inconsistent."""


FIXED_SPEC_COMMIT = "42dcbc4334cb48ecaf63efb550345e3eea2fb7ad"
APPROVED_PLAN_COMMIT = "5f22ebef1e23d045a10f2ec0a3c87be029446cf6"
APPROVED_PLAN_SHA256 = "4fba002ad9f969cd01866ea08f270654f82b53c7d90b73d28643a9abb12cba68"
PLAN_PATH = "docs/superpowers/plans/2026-08-05-psyche-w2-g2-foundation.md"
SOURCE_ROWS = {
    "PLAN": (
        "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/PLAN.md",
        "01382f8a0d2bca95ddd535634dd6a9f09ac4a80d588ccbebd72f163eaf56bc1e",
    ),
    "RUNTIME_DESIGN": (
        "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/RUNTIME_DESIGN.md",
        "ab8c922214b8f1179ebf71fb8dfb55bd6d0ff2d6dfced4551bf90503767bb6b8",
    ),
    "TECH": (
        "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/TECH.md",
        "1d00fb2b725f384ca027db60d0afbd0a62a7ec6c7dcbb5637bf14d30d40e2e1c",
    ),
    "COVEN_PREREQUISITES": (
        "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/COVEN_PREREQUISITES.md",
        "33994a28921e70f824b0260ce08231b2117c50430c54e996ed47582d060e72f9",
    ),
    "COVEN_W1_AUDIT": (
        "https://github.com/OpenCoven/coven/blob/42dcbc43%334cb48ec%61f63efb5%350345e3e%65a2fb7ad/specs/psyche/COVEN_W1_AUDIT.md",
        "eab9028bf7ef9c8a96d4c6bed69e4ef0b3497b470ca26589cb3ffcd80677322d",
    ),
}

# The commands are plan-owned literals. Hashing keeps this checker readable
# while still rejecting any byte of drift in a command cell.
MATRIX_COMMAND_SHA256 = {
    "Canonical ID prefixes and execution-binding identity": "99555c557ce768a2691ea7a7e9e238d73596f0d6d302152fd128e7e2c1f55356",
    "Complete canonical error enum": "8f65d986781fb10422a0261e17138201e7b3379ef8c0a09defab7be815dd4bee",
    "Canonical delivery v1 shape": "832a79d4126a7cb77b8e0b0341674ca7348ef356091dab02257ccd390a69a384",
    "Surface and quarantine owned types": "c5915caab82a15c37e3af5ddc41ec948043daf158fe78c4decc8ed677a17f1f9",
    "Package-local nullable-binding fixtures": "2ac01fe93e9fb2580b65464cefaf2a1e942c2240048130d24597688e7d7b8d7a",
    "Exhaustive registered decode": "7f744575d04e26cdc6a798d0e315371fcf9bf1b598c4aca9e8052da9d34cd6aa",
    "Unknown kind/version/enum denial and quarantine": "ec7296a6b7fb26e7df19fb2c7147bb6225f45c279937b02046110773e468427e",
    "Quarantine resolution": "0529a41bff3a7533173055d8d015d1d8a46ae922b0772c8f19fd0bde752bb5d3",
    "Direct typed insert validation": "77f4e53c38d571775bf74280f50ea56ce8f802296d1e075425417e269210f5b4",
    "Append-only execution-binding revisions": "a3823e0c95245f67996706bc37171569e9369e72166a4b42643da4c90deef71b",
    "Transition contract and append-only rules": "65a4b8ac3d7e29f39d2d896ad222477576830b7a2829b41b5c147f0118dde3be",
    "Checkpoint-failure shutdown": "979c2261ae0b36b6a1ab5bd34c70a5ec253c5f3120f87876fe33835d18207a69",
    "Migrations": "f6d8857ebfc786480c20c2d693ed6cd4398bb5ad5979bb6454d15564d3532580",
    "State-machine/property": "ec8c6e799d722b7a382d019853a8405949b464f44f68fb9c6fdb097a789b73bd",
    "Crash/restart": "88e020450df496ffb6d915efacb4fc640a2bc325f023ac6332870ec847b71d16",
    "Fake boundaries and durable termination ordering": "71355a2738db952c35c0fc18734eb80287602dbf0c10e2aabc96293c05ed9b15",
    "Execution request RFC3339 golden bytes": "5315ed856e8dd813132a338a8024779707d8251efd0bdf71b92c2eae57c3a6e1",
    "Validated termination dispatch": "68d7ac817187f977be07ffa7668a340222eba600a116380e46ba2b2cd43a67c1",
    "G2 cancellation-state vocabulary": "b1822ccbdbd9cb0488227c16b94fd9ad75809c479bbfb01c3b9b8dba08b87caa",
    "Full execution-request digest binding": "1ae9e298ddfd97efbd0b673655fbc90fbd78ef650f75fb20a4aaa803d8e166ff",
    "C-S1 scripted contract negotiation": "0a0749461006ab5af3c1efd18cbff9b03b05e55045894153ce081160de337a83",
    "C-S2 scripted session lifecycle": "19ee8df21953e783e4fa7ac82cee53728deb944ee333a19646b0b4b52cda80b6",
    "C-S3 scripted snapshot/attempt binding": "02b6ecde06ab9bb935b81f7d1c003058ed7519ae272b139f1e82e10664510c3c",
    "C-S4 scripted stable adoption": "7fa24fdf7b9465a457d33aa1c61c00b6372de8509b4142dbe109b9e6bdc9eb5c",
    "C-S5 scripted non-adoption proof": "d25d4488d7d4d8832dca0ac72eec5fbaf09fbb870ff9ff035aeec21b6c039b55",
    "C-S6 scripted ambiguity reconciliation/fence": "228e9790d023509f64a3871dfd58c2fd9b9afb4c1c18efa8e827d4a07f93dd84",
    "C-S7 scripted ordered cursor": "220c990305d5ab9f080aafbab8b5224da1fc67e29f3a5f5a3d5cf141365675a8",
    "C-S8 scripted terminal authority": "5e3223be4f8d8280792509a78661ae892691c6a0513e71fe35ea6752bbb86dc9",
    "C-S9 scripted O5 cancellation acknowledgement": "fe4d01e7088f7376df97614522b4eebe3ee3cb7493d216c748d30bc2bf99b26a",
    "C-S10 scripted result/artifact binding": "6bcfe3d642acc1a0c258cbaa3ddefd2257bba8a8d8b6e2251d93ce8d07366af3",
    "C-S11 scripted restart persistence": "24866d81ddab9d0131cef455721cd2aee7f06fe61dd0320420bdf2617c128860",
    "C-S12 scripted structured denial": "3e38ff6947e6e91504727847c5adae7cd3471f998d90c3980b67f520ce955a4a",
}

CI_COMMANDS = (
    "cargo fmt --all -- --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo test --workspace --locked",
    "cargo test -p psyche-test-support --test state_machine",
    "cargo test -p psyche-test-support --test conformance",
    "cargo test -p psyche-store --test migrations",
    "cargo test -p psyche-store --features test-fault-injection --test crash",
    "cargo clippy -p psyche-store --all-targets --features test-fault-injection -- -D warnings",
    "cargo deny check licenses advisories bans sources",
    'gitleaks detect --no-banner --redact --log-opts="--all"',
    "python3 scripts/check-g2-evidence-test.py",
    "python3 scripts/check-g2-evidence.py",
)
NON_RUST_CI_COMMAND_JOBS = {
    "cargo deny check licenses advisories bans sources": "supply-chain",
    'gitleaks detect --no-banner --redact --log-opts="--all"': "secrets",
}
# Pins the complete reviewed workflow, including setup/actions and every writer
# that could poison GITHUB_ENV or GITHUB_PATH. Newlines are normalized first so
# the same reviewed content verifies on Windows checkouts.
REVIEWED_WORKFLOW_SHA256 = "1f908303c1a8940ce5ec8c81182ddaf5d82e5c6bddd5ece7fb33e1baf9a087f1"
CI_WORKFLOW_ID = 326408880


def fail(message: str) -> None:
    raise EvidenceError(message)


def read_text(root: pathlib.Path, path: str, overrides: Mapping[str, str]) -> str:
    if path in overrides:
        return overrides[path]
    candidate = root / path
    if not candidate.is_file():
        fail(f"required file is absent: {path}")
    return candidate.read_text(encoding="utf-8")


def parse_tables(markdown: str) -> tuple[list[list[str]], list[list[str]]]:
    source: list[list[str]] = []
    matrix: list[list[str]] = []
    active: list[list[str]] | None = None
    for line in markdown.splitlines():
        if line == "| Coven source | Immutable URL | SHA-256 |":
            active = source
            continue
        if line == "| Criterion | Command | Result | Artifact |":
            active = matrix
            continue
        if active is not None and line.startswith("|---"):
            continue
        if active is not None and line.startswith("|"):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if not all(cells):
                fail("evidence table contains an empty cell")
            active.append(cells)
        elif active is not None:
            active = None
    if any(len(row) != 3 for row in source) or any(len(row) != 4 for row in matrix):
        fail("evidence table has an invalid column count")
    return source, matrix


def field(markdown: str, label: str) -> str:
    matches = re.findall(rf"^\*\*{re.escape(label)}:\*\* (.+)$", markdown, re.MULTILINE)
    if not matches:
        fail(f"missing evidence field: {label}")
    if len(matches) != 1:
        fail(f"evidence field must occur exactly once: {label}")
    return matches[0].strip().strip("`")


def yaml_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def parse_workflow_steps(workflow: str) -> list[dict[str, object]]:
    """Extract active named/uses steps without treating YAML comments as data."""
    lines = workflow.splitlines()
    try:
        jobs_at = lines.index("jobs:")
    except ValueError:
        fail("workflow jobs mapping is absent")
    job_starts = [
        (index, match.group("job"))
        for index, line in enumerate(lines[jobs_at + 1:], start=jobs_at + 1)
        if (match := re.fullmatch(r"  (?P<job>[A-Za-z0-9_-]+):\s*", line))
    ]
    starts = [
        (index, len(match.group("indent")), match.group("key"))
        for index, line in enumerate(lines)
        if (match := re.match(r"^(?P<indent>\s*)-\s+(?P<key>name|uses):", line))
    ]
    steps: list[dict[str, object]] = []
    for position, (start, indent, header_key) in enumerate(starts):
        owners = [(index, job) for index, job in job_starts if index < start]
        if not owners:
            fail("workflow step is not contained by a job")
        owner_at, owner = owners[-1]
        end = next((index for index, _ in job_starts if index > owner_at), len(lines))
        for candidate, candidate_indent, _ in starts[position + 1:]:
            if candidate < end and candidate_indent == indent:
                end = candidate
                break
        child = " " * (indent + 2)
        grandchild = " " * (indent + 4)
        run_values: list[str] = []
        env: dict[str, str] = {}
        env_counts: dict[str, int] = {}
        direct_counts = {header_key: 1}
        invalid = False
        in_env = False
        for line in lines[start + 1:end]:
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            line_indent = len(line) - len(line.lstrip())
            if in_env and line_indent == indent + 4:
                match = re.fullmatch(r"(?P<key>[A-Za-z_][A-Za-z0-9_-]*):(?P<value>.*)", line[len(grandchild):])
                if not match:
                    invalid = True
                    continue
                key = match.group("key")
                env_counts[key] = env_counts.get(key, 0) + 1
                env[key] = yaml_scalar(match.group("value"))
                continue
            if line_indent == indent + 2:
                in_env = False
                match = re.fullmatch(r"(?P<key>[A-Za-z_][A-Za-z0-9_-]*):(?P<value>.*)", line[len(child):])
                if not match:
                    invalid = True
                    continue
                key = match.group("key")
                value = match.group("value")
                direct_counts[key] = direct_counts.get(key, 0) + 1
                if key == "env":
                    if value.strip():
                        invalid = True
                    else:
                        in_env = True
                elif key == "run":
                    run_values.append(yaml_scalar(value))
                continue
            if line_indent <= indent + 2:
                in_env = False
        if len(run_values) > 1:
            fail("workflow step contains more than one active run value")
        steps.append({
            "run": run_values[0] if run_values else None,
            "env": env,
            "env_counts": env_counts,
            "direct_counts": direct_counts,
            "invalid": invalid,
            "job": owner,
        })
    return steps


def workflow_job_lines(workflow: str, job: str) -> list[str]:
    lines = workflow.splitlines()
    job_pattern = re.compile(rf"^  {re.escape(job)}:\s*$")
    starts = [index for index, line in enumerate(lines) if job_pattern.fullmatch(line)]
    if len(starts) != 1:
        fail(f"workflow must contain exactly one {job} job")
    start = starts[0]
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if re.match(r"^  [A-Za-z0-9_-]+:\s*$", lines[index]):
            end = index
            break
    return lines[start + 1:end]


def mapping_values(lines: list[str], indent: int) -> dict[str, list[str]]:
    values: dict[str, list[str]] = {}
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if len(line) - len(line.lstrip()) == indent:
            match = re.fullmatch(r"(?P<key>[A-Za-z_][A-Za-z0-9_-]*):(?P<value>.*)", line[indent:])
            if not match:
                fail(f"workflow contains a noncanonical key at indentation {indent}: {line.strip()}")
            values.setdefault(match.group("key"), []).append(yaml_scalar(match.group("value")))
    return values


def nested_mapping_lines(lines: list[str], indent: int, key: str) -> list[str]:
    header = " " * indent + key + ":"
    starts = [index for index, line in enumerate(lines) if line == header]
    if len(starts) != 1:
        fail(f"workflow mapping must contain exactly one active {key}")
    start = starts[0]
    end = len(lines)
    for index in range(start + 1, len(lines)):
        line = lines[index]
        if line.strip() and not line.lstrip().startswith("#") and len(line) - len(line.lstrip()) <= indent:
            end = index
            break
    return lines[start + 1:end]


def validate_workflow_scope(workflow: str) -> None:
    lines = workflow.splitlines()
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if re.search(r"(?:^|[\s:\[\{,])(?:&|\*)[A-Za-z_][A-Za-z0-9_-]*", line) or re.match(r"^\s*<<\s*:", line):
            fail("workflow anchors, aliases, and merge keys are not allowed")
    root = mapping_values(lines, 0)
    expected_root = {"name", "on", "concurrency", "env", "jobs"}
    if set(root) != expected_root or any(len(values) != 1 for values in root.values()):
        fail("workflow root must use the exact canonical CI structure")
    if root["on"] != [""]:
        fail("workflow triggers must use the canonical block mapping")
    triggers = nested_mapping_lines(lines, 0, "on")
    trigger_values = mapping_values(triggers, 2)
    if trigger_values != {"push": [""], "pull_request": [""]}:
        fail("workflow must run only for main pushes and pull requests")
    push = mapping_values(nested_mapping_lines(triggers, 2, "push"), 4)
    if push != {"branches": ["[main]"]}:
        fail("workflow push trigger must target exactly main")
    if mapping_values(nested_mapping_lines(triggers, 2, "pull_request"), 4):
        fail("workflow pull_request trigger must be unqualified")
    global_env = mapping_values(nested_mapping_lines(lines, 0, "env"), 2)
    if global_env != {"CARGO_TERM_COLOR": ["always"], "RUSTFLAGS": ["-D warnings"]}:
        fail("workflow global env must contain only fixed non-overriding values")


def validate_required_job_shapes(workflow: str) -> None:
    expected = {
        "rust": {"name", "runs-on", "strategy", "steps"},
        "supply-chain": {"name", "runs-on", "steps"},
        "secrets": {"name", "runs-on", "steps"},
    }
    for job, keys in expected.items():
        values = mapping_values(workflow_job_lines(workflow, job), 4)
        if set(values) != keys or any(len(entries) != 1 for entries in values.values()):
            fail(f"CI required-command job {job} must use its exact canonical direct keys")


def validate_rust_matrix(workflow: str) -> None:
    rust = workflow_job_lines(workflow, "rust")
    direct = mapping_values(rust, 4)
    if direct.get("runs-on") != ["${{ matrix.os }}"]:
        fail("CI rust job must run on the active matrix.os value")
    strategy = nested_mapping_lines(rust, 4, "strategy")
    strategy_values = mapping_values(strategy, 6)
    if strategy_values.get("fail-fast") != ["false"]:
        fail("CI rust strategy must actively set fail-fast to false")
    matrix = nested_mapping_lines(strategy, 6, "matrix")
    matrix_values = mapping_values(matrix, 8)
    if set(matrix_values) != {"os"} or len(matrix_values["os"]) != 1:
        fail("CI rust matrix must contain only the supported os axis")
    os_value = matrix_values["os"][0]
    if not (os_value.startswith("[") and os_value.endswith("]")):
        fail("CI rust matrix os axis must be an inline list")
    systems = [yaml_scalar(item) for item in os_value[1:-1].split(",") if item.strip()]
    expected = {"ubuntu-latest", "macos-latest", "windows-latest"}
    if len(systems) != 3 or set(systems) != expected:
        fail("CI rust matrix must cover exactly ubuntu, macOS, and Windows")


def validate_ci_structure(workflow: str) -> None:
    validate_workflow_scope(workflow)
    validate_required_job_shapes(workflow)
    steps = parse_workflow_steps(workflow)
    env_requirements = {
        "cargo test -p psyche-test-support --test state_machine": {
            "PROPTEST_CASES": "2048",
            "PROPTEST_RNG_SEED": "0" * 32,
        },
        "python3 scripts/check-g2-evidence.py": {"GH_TOKEN": "${{ github.token }}"},
    }
    for command in CI_COMMANDS:
        matching = [step for step in steps if step["run"] == command]
        if len(matching) != 1:
            fail(f"CI workflow must run exact G2 command once: {command}")
        expected_job = NON_RUST_CI_COMMAND_JOBS.get(command, "rust")
        if matching[0]["job"] != expected_job:
            fail(f"CI workflow runs required command outside {expected_job}: {command}")
        step = matching[0]
        required_env = env_requirements.get(command, {})
        required_direct = {"name": 1, "run": 1}
        if required_env:
            required_direct["env"] = 1
        if step["invalid"] or step["direct_counts"] != required_direct:
            fail(f"CI required command step has noncanonical direct keys: {command}")
        if step["env"] != required_env or step["env_counts"] != {key: 1 for key in required_env}:
            fail(f"CI required command step has noncanonical env: {command}")
    validate_rust_matrix(workflow)


def validate_ci_workflow(workflow: str) -> None:
    normalized = workflow.replace("\r\n", "\n").replace("\r", "\n")
    if hashlib.sha256(normalized.encode("utf-8")).hexdigest() != REVIEWED_WORKFLOW_SHA256:
        fail("CI workflow differs from the complete reviewed workflow")
    validate_ci_structure(normalized)


def normalize_grouped(value: str, prefix: str = "") -> str:
    value = value.removeprefix(prefix).replace("-", "")
    if not re.fullmatch(r"[0-9a-f]+", value):
        fail(f"invalid hexadecimal value: {value}")
    return value


def parse_blob_url(url: str) -> tuple[str, str]:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or parsed.netloc != "github.com":
        fail(f"Coven source URL is not immutable HTTPS: {url}")
    match = re.fullmatch(r"/OpenCoven/coven/blob/([^/]+)/(.+)", parsed.path)
    if not match:
        fail(f"Coven source URL is not an OpenCoven/coven blob URL: {url}")
    commit = urllib.parse.unquote(match.group(1)).replace("-", "")
    path = urllib.parse.unquote(match.group(2))
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or path.startswith("/") or ".." in pathlib.PurePosixPath(path).parts:
        fail(f"Coven source URL does not name a 40-hex commit and safe path: {url}")
    return commit, path


def validate_evidence(markdown: str) -> tuple[str, list[list[str]], list[list[str]]]:
    status = field(markdown, "Status")
    if status not in {"candidate", "passed"}:
        fail("evidence status must be candidate or passed")
    source, matrix = parse_tables(markdown)
    source_names = [row[0] for row in source]
    if len(source_names) != len(set(source_names)) or set(source_names) != set(SOURCE_ROWS):
        fail("Coven source table must contain each fixed source exactly once")
    for name, url_cell, digest_cell in source:
        url = url_cell.strip("`")
        digest = normalize_grouped(digest_cell.strip("`"), "sha256:")
        if (url, digest) != SOURCE_ROWS[name]:
            fail(f"immutable Coven source row drifted: {name}")
        commit, _ = parse_blob_url(url)
        if commit != FIXED_SPEC_COMMIT:
            fail(f"Coven specification source commit drifted: {name}")
    if normalize_grouped(field(markdown, "Coven specification source commit")) != FIXED_SPEC_COMMIT:
        fail("Coven specification source field drifted")

    criteria = [row[0] for row in matrix]
    if len(criteria) != len(set(criteria)) or set(criteria) != set(MATRIX_COMMAND_SHA256):
        fail("evidence matrix must contain every required criterion exactly once")
    for criterion, command_cell, result, artifact in matrix:
        if not (command_cell.startswith("`") and command_cell.endswith("`")):
            fail(f"matrix command is not a code literal: {criterion}")
        command = command_cell[1:-1]
        if hashlib.sha256(command.encode()).hexdigest() != MATRIX_COMMAND_SHA256[criterion]:
            fail(f"matrix command differs from the plan allowlist: {criterion}")
        for atomic in command.split(" && "):
            if not re.search(r" -- --exact [A-Za-z0-9_:]+$", atomic):
                fail(f"matrix command is not an exact filtered test: {atomic}")

    if status == "candidate":
        expected = {
            "Tested source commit": "not recorded before remote review",
            "CI attestation": "not recorded before remote review",
            "Coven plan source commit": "not recorded before plan approval",
            "Coven plan URL": "not recorded before plan approval",
            "Coven plan SHA-256": "not recorded before plan approval",
        }
        for label, value in expected.items():
            if field(markdown, label) != value:
                fail(f"candidate field must use its exact placeholder: {label}")
        if any(row[2:] != ["not run remotely", "none"] for row in matrix):
            fail("candidate matrix must use the exact result and artifact placeholders")
    else:
        tested = field(markdown, "Tested source commit")
        run_url = field(markdown, "CI attestation")
        plan_commit = normalize_grouped(field(markdown, "Coven plan source commit"))
        plan_url = field(markdown, "Coven plan URL")
        plan_digest = normalize_grouped(field(markdown, "Coven plan SHA-256"), "sha256:")
        if not re.fullmatch(r"[0-9a-f]{40}", tested):
            fail("passed evidence requires a 40-hex tested source")
        if plan_commit != APPROVED_PLAN_COMMIT or plan_digest != APPROVED_PLAN_SHA256:
            fail("passed evidence does not name the approved Coven plan provenance")
        url_commit, url_path = parse_blob_url(plan_url)
        if url_commit != plan_commit or url_path != PLAN_PATH:
            fail("passed evidence plan URL does not match the approved plan")
        if not re.fullmatch(r"https://github\.com/OpenCoven/psyche/actions/runs/[0-9]+", run_url):
            fail("passed evidence requires an immutable Actions run URL")
        if any(row[2] != "passed" or row[3] != run_url for row in matrix):
            fail("every passed matrix result and artifact must attest the same run")
        placeholders = re.compile(r"not run remotely|\bnone\b|not recorded|pending|placeholder|TBD|TODO", re.IGNORECASE)
        if placeholders.search(markdown):
            fail("passed evidence contains a candidate placeholder")
    return status, source, matrix


def manifest_atomic_commands(manifest: Mapping[str, object]) -> tuple[dict[str, tuple[str, str]], dict[str, str]]:
    targets = manifest.get("targets")
    if not isinstance(targets, dict) or not targets:
        fail("manifest.targets must be a non-empty object")
    atomic: dict[str, tuple[str, str]] = {}
    lists: dict[str, str] = {}
    for target, raw in targets.items():
        if not isinstance(target, str) or not isinstance(raw, dict):
            fail("manifest target entries must be objects")
        list_command = raw.get("list_command")
        tests = raw.get("tests")
        if not isinstance(list_command, str) or not list_command.endswith(" -- --list --format terse"):
            fail(f"invalid list command for {target}")
        if not isinstance(tests, list) or not tests or any(not isinstance(name, str) or not name for name in tests):
            fail(f"manifest target has no exact tests: {target}")
        if len(tests) != len(set(tests)):
            fail(f"manifest target repeats a test: {target}")
        prefix = list_command.removesuffix(" -- --list --format terse")
        lists[target] = list_command
        for name in tests:
            command = f"{prefix} -- --exact {name}"
            if command in atomic:
                fail(f"manifest maps an atomic command more than once: {command}")
            atomic[command] = (target, name)
    return atomic, lists


def list_tests(root: pathlib.Path, commands: Mapping[str, str]) -> dict[str, str]:
    outputs: dict[str, str] = {}
    for target, command in commands.items():
        completed = subprocess.run(command.split(), cwd=root, text=True, capture_output=True, check=False)
        if completed.returncode:
            fail(f"test listing failed for {target}:\n{completed.stdout}{completed.stderr}")
        outputs[target] = completed.stdout
    return outputs


def validate_manifest(
    root: pathlib.Path,
    manifest: Mapping[str, object],
    matrix: list[list[str]],
    listed_tests: Mapping[str, str] | None,
) -> None:
    atomic_manifest, list_commands = manifest_atomic_commands(manifest)
    matrix_commands: list[str] = []
    for row in matrix:
        matrix_commands.extend(row[1][1:-1].split(" && "))
    if len(matrix_commands) != len(set(matrix_commands)):
        fail("an atomic matrix command is duplicated")
    if set(matrix_commands) != set(atomic_manifest):
        missing = sorted(set(matrix_commands) - set(atomic_manifest))
        unused = sorted(set(atomic_manifest) - set(matrix_commands))
        fail(f"matrix/manifest mismatch; missing={missing}, unused={unused}")
    outputs = dict(listed_tests) if listed_tests is not None else list_tests(root, list_commands)
    if set(outputs) != set(list_commands):
        fail("test listing output does not cover exactly the manifest targets")
    for target, output in outputs.items():
        names = {
            line.rsplit(": test", 1)[0]
            for line in output.splitlines()
            if line.endswith(": test")
        }
        if not names:
            fail(f"test target lists zero tests: {target}")
        for name in manifest["targets"][target]["tests"]:  # type: ignore[index]
            if name not in names:
                fail(f"exact manifest test is absent from {target}: {name}")


def require_terms(path: str, text: str, terms: tuple[str, ...]) -> None:
    missing = [term for term in terms if term not in text]
    if missing:
        fail(f"{path} is missing required relationships: {missing}")


def require_digest_mutation_matrix(path: str, text: str) -> None:
    common = (
        "schema_version", "request_id", "graph_id", "node_id", "attempt_id",
        "principal_id", "familiar_snapshot_id", "project_id",
        "context_manifest_digest", "required_artifact_bindings", "payload_digest",
        "created_at", "valid_until",
    )
    launch = common + ("project_root", "cwd", "harness", "delegation_digest", "budget_digest")
    input_request = common + ("session_id", "input_digest")
    artifact = ("artifact_id", "digest", "media_type", "size")
    for name in launch + input_request:
        if f'"/input/{name}"' not in text:
            fail(f"{path} does not stale-digest mutate request field: {name}")
    for name in artifact:
        if f'"/input/required_artifact_bindings/0/{name}"' not in text:
            fail(f"{path} does not stale-digest mutate artifact field: {name}")
    if 'mutations.push(("/input", other_input))' not in text:
        fail(f"{path} does not stale-digest mutate the request variant")


def validate_record_kinds(source: str) -> None:
    enum = re.search(r"pub enum RecordKind \{(.*?)\n\}", source, re.DOTALL)
    if not enum:
        fail("RecordKind declaration is absent")
    variants = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]+),$", enum.group(1), re.MULTILINE)
    if len(variants) != 15 or "Attempt" not in variants or "ExecutionBinding" in variants:
        fail(f"RecordKind must have exactly 15 variants with Attempt only: {variants}")
    all_block = re.search(r"pub const ALL: \[RecordKind; 15\] = \[(.*?)\];", source, re.DOTALL)
    prefixes = re.search(r"pub const fn prefix\(self\).*?match self \{(.*?)\n\s*\}", source, re.DOTALL)
    if not all_block or re.findall(r"RecordKind::([A-Za-z0-9]+)", all_block.group(1)) != variants:
        fail("RecordKind::ALL is not exhaustive and declaration-ordered")
    if not prefixes:
        fail("RecordKind prefix match is absent")
    pairs = re.findall(r'RecordKind::([A-Za-z0-9]+) => "([a-z]{3}_)"', prefixes.group(1))
    if [name for name, _ in pairs] != variants:
        fail("RecordKind prefix match does not use the exact variant set")
    if dict(pairs).get("Attempt") != "att_" or sum(prefix == "att_" for _, prefix in pairs) != 1:
        fail("Attempt must be the only att_ record kind")
    if source.count("SchemaKind::ExecutionBinding => Some(RecordKind::Attempt)") != 1:
        fail("ExecutionBinding must map exactly once to RecordKind::Attempt")


def validate_result_fixture(text: str) -> None:
    try:
        bundle = json.loads(text)
    except json.JSONDecodeError as error:
        fail(f"result-bundle fixture is invalid JSON: {error}")
    if set(bundle) != {"artifacts", "correlation", "result", "session_id"}:
        fail("result-bundle fixture is not strict and complete")
    if not isinstance(bundle["artifacts"], list) or not bundle["artifacts"]:
        fail("result-bundle fixture has no artifact")
    references = [bundle["result"]] + [artifact.get("content", {}) for artifact in bundle["artifacts"]]
    for reference in references:
        if set(reference) != {"digest", "expires_at", "media_type", "size_bytes"}:
            fail("every result/artifact content reference needs digest, media_type, size_bytes, expires_at")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", str(reference["digest"])):
            fail("content reference digest is not canonical")
        if not isinstance(reference["size_bytes"], int) or reference["size_bytes"] <= 0:
            fail("content reference size is invalid")
        if not re.fullmatch(r"[^/\s]+/[^/\s]+", str(reference["media_type"])):
            fail("content reference media type is invalid")
        if not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", str(reference["expires_at"])):
            fail("content reference expiry is not canonical RFC3339 UTC")
    correlation = bundle["correlation"]
    for artifact in bundle["artifacts"]:
        if artifact.get("correlation") != correlation or artifact.get("session_id") != bundle["session_id"]:
            fail("artifact lifetime/correlation does not match its result bundle")


def validate_golden(path: str, raw: bytes, expected_sha: str) -> None:
    if raw.endswith(b"\n"):
        fail(f"golden fixture has a trailing newline: {path}")
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        fail(f"golden fixture is invalid JSON: {path}: {error}")
    for key in ("created_at", "valid_until"):
        if not isinstance(value.get(key), str) or not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", value[key]):
            fail(f"golden fixture lacks an RFC3339 string {key}: {path}")
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    if canonical != raw:
        fail(f"golden fixture is not canonical JSON: {path}")
    if hashlib.sha256(raw).hexdigest() != expected_sha:
        fail(f"golden fixture SHA-256 drifted: {path}")


def validate_sources(root: pathlib.Path, overrides: Mapping[str, str]) -> None:
    port_path = "crates/psyche-coven/src/port.rs"
    port = read_text(root, port_path, overrides)
    require_terms(port_path, port, (
        "pub fn new(input: ExecutionRequestInput)", "let request_digest = digest(&input)?;",
        "pub fn recompute_digest", "pub trait CovenPort", "async fn reconcile(",
        "pub struct ResultBundle", "CancellationAcknowledgementEvidence",
    ))
    if re.search(r"(?:struct|enum)\s+\w*Acknowledgement\w*", port):
        fail("psyche-coven owns a duplicate acknowledgement wire type")

    suite_path = "crates/psyche-test-support/src/suites/coven.rs"
    suite = read_text(root, suite_path, overrides)
    for number in range(1, 13):
        if suite.count(f"pub async fn assert_c_s{number}_") != 1:
            fail(f"reusable C-S{number} function is absent or duplicated")
    require_terms(suite_path, suite, (
        "stale_digest_mutations", "request.recompute_digest()", "RequestDigestMismatch",
        "RAW_LEDGER_STATES", '"killed"', '"orphaned"', "CancellationAcknowledgementEvidence",
        "assert_c_s6_ambiguity_fence", "ReconciliationDisposition::Returned",
        "ReconciliationDisposition::Fenced", "DurableDispositionKind::Returned",
        "DurableDispositionKind::Fenced", "fixture.restart().await", "require_fault(",
        "redispatch_eligibility", "EligibleAfterFence", "assert_c_s10_result_artifact_binding",
        "mutate_result_digest", "mutate_result_media_type", "mutate_result_size",
        "mutate_result_expiry", "mutate_artifact_digest", "mutate_artifact_media_type",
        "mutate_artifact_size", "mutate_artifact_expiry",
    ))
    require_digest_mutation_matrix(suite_path, suite)
    fake_path = "crates/psyche-test-support/src/coven.rs"
    fake = read_text(root, fake_path, overrides)
    require_terms(fake_path, fake, ("async fn adopt", "request.validate_digest()?;"))
    if suite.count("request.validate_digest()?;") != 1:
        fail("scripted Coven boundary must recompute the request digest exactly once")
    conformance_path = "crates/psyche-test-support/tests/conformance.rs"
    conformance = read_text(root, conformance_path, overrides)
    for number in range(1, 13):
        if len(re.findall(rf"async fn c_s{number}_[a-z0-9_]+\(\)", conformance)) != 1:
            fail(f"exact C-S{number} wrapper is absent or duplicated")

    state_path = "crates/psyche-test-support/tests/state_machine.rs"
    state = read_text(root, state_path, overrides)
    require_terms(state_path, state, (
        "c_s6_model_never_redispatches_without_fence", "request_digest_binds_every_typed_field",
        "stale_digest_requests", "MutateRequestFieldRetainDigest", "RequestDigestMismatch",
        "ReconciliationDisposition::Returned", "ReconciliationDisposition::Fenced",
        "fixture.restart().await", "select_fault(", "RedispatchEligibility::EligibleAfterFence",
    ))
    require_digest_mutation_matrix(state_path, state)

    core_path = "crates/psyche-core/src/contracts/mod.rs"
    core = read_text(root, core_path, overrides)
    validate_record_kinds(core)
    require_terms(core_path, core, (
        "const ALL: [SchemaKind; 16]", "CanonicalDocument::ExecutionBinding",
        "SchemaKind::Error => None", "UnknownSchema", "UnsupportedMajor", "UnknownEnumValue",
    ))
    error_path = "crates/psyche-core/src/contracts/error.rs"
    require_terms(error_path, read_text(root, error_path, overrides), ("pub const ALL: [Self; 36]",))
    contracts_path = "crates/psyche-core/tests/contracts.rs"
    require_terms(contracts_path, read_text(root, contracts_path, overrides), (
        "all_canonical_error_codes_decode", "delivery_v1_fixture_round_trips_canonically",
        "surface_event_and_effect_fixtures_round_trip", "cancellation_state_vocabulary_requires_matching_o5_evidence",
        "graph_and_node_accept_only_the_two_frozen_nullable_bindings",
    ))
    records_path = "crates/psyche-store/tests/records.rs"
    require_terms(records_path, read_text(root, records_path, overrides), (
        "direct_insert_rejects_acknowledged_cancellation_without_evidence",
        "CancellationAcknowledgementEvidence", "direct_insert_rejects_mismatched_cancellation_evidence",
    ))
    result_path = "crates/psyche-coven/tests/fixtures/result-bundle.json"
    validate_result_fixture(read_text(root, result_path, overrides))
    bindings_path = "crates/psyche-coven/tests/bindings.rs"
    require_terms(bindings_path, read_text(root, bindings_path, overrides), (
        "result_bundle_fixture_round_trips_complete_content_references",
        "result_bundle_fixture_uses_launch_request_correlation",
        "content_reference_rejects_digest_size_media_type_and_lifetime_mismatch",
        '"/artifacts/0/correlation/request_digest"', '"/artifacts/0/correlation/valid_until"',
    ))
    request_test_path = "crates/psyche-coven/tests/request_digest.rs"
    request_tests = read_text(root, request_test_path, overrides)
    require_terms(request_test_path, request_tests, (
        "execution_request_launch_matches_golden_bytes_and_digest",
        "execution_request_input_matches_golden_bytes_and_digest",
        "75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04",
        "c8c3d0cad99f65d0fdac7b2bb577cf1278412a7ea6255d443e45394109311c61",
    ))
    for path, digest in (
        ("crates/psyche-coven/tests/fixtures/execution-request-launch.json", "75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04"),
        ("crates/psyche-coven/tests/fixtures/execution-request-input.json", "c8c3d0cad99f65d0fdac7b2bb577cf1278412a7ea6255d443e45394109311c61"),
    ):
        raw = overrides[path].encode() if path in overrides else (root / path).read_bytes()
        validate_golden(path, raw, digest)


def validate_docs(root: pathlib.Path, overrides: Mapping[str, str]) -> None:
    architecture = read_text(root, "docs/ARCHITECTURE.md", overrides)
    for line in (
        "psyche-core <- psyche-config", "psyche-core <- psyche-store", "psyche-core <- psyche-coven",
        "psyche-core <- psyche-surfaces",
        "psyche-core + psyche-coven + psyche-surfaces + psyche-store <- psyche-test-support",
        "psyche-config + psyche-store <- psyche-runtime <- psyche-cli",
        "psyche-config <- psyche-cli", "psyche-store <- psyche-cli",
    ):
        if architecture.count(line) != 1:
            fail(f"architecture dependency direction is absent or duplicated: {line}")
    schemas = read_text(root, "docs/SCHEMAS.md", overrides)
    require_terms("docs/SCHEMAS.md", schemas, (
        "psyche.identity_snapshot.v1", "psyche.intent.v1", "psyche.surface_event.v1", "psyche.graph.v1",
        "psyche.graph_node.v1", "psyche.delegation.v1", "psyche.budget.v1", "psyche.approval.v1",
        "psyche.execution_binding.v1", "psyche.evidence.v1", "psyche.verdict.v1", "psyche.recovery.v1",
        "psyche.addon.v1", "psyche.surface_effect.v1", "psyche.delivery.v1", "psyche.error.v1",
        "unknown kind", "unknown major", "unknown enum", "Transition", "del_", "dlg_", "QuarantineId",
        "qua_", "CancellationState", "CancellationAcknowledgementEvidence", "killed", "orphaned",
        "ResultBundle", "digest", "media_type", "size_bytes", "expires_at", "Attempt", "att_",
        "ExecutionBinding", "retention", "Deferred",
    ))
    testing = read_text(root, "docs/TESTING.md", overrides)
    require_terms("docs/TESTING.md", testing, (
        "scripts", "PROPTEST_CASES", "PROPTEST_RNG_SEED", "crash", "fault", "observation",
        "full-request digest", "RFC3339", "g2-test-manifest.json", "killed", "orphaned",
        "Immutable Coven", "return", "fence", "restart", "no-redispatch", "ExpectedUnsupported",
    ) + tuple(f"C-S{number}" for number in range(1, 13)))


def run_json(command: list[str], root: pathlib.Path) -> object:
    completed = subprocess.run(command, cwd=root, text=True, capture_output=True, check=False)
    if completed.returncode:
        fail(f"command failed: {' '.join(command)}\n{completed.stdout}{completed.stderr}")
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        fail(f"command did not return JSON: {' '.join(command)}: {error}")


def verify_coven_blob(root: pathlib.Path, url: str, expected_digest: str) -> None:
    commit, path = parse_blob_url(url)
    response = run_json(["gh", "api", f"repos/OpenCoven/coven/contents/{path}?ref={commit}"], root)
    if not isinstance(response, dict) or response.get("type") != "file" or not response.get("sha"):
        fail(f"Coven content API did not return a commit-owned blob: {path}")
    try:
        content = base64.b64decode(str(response["content"]), validate=False)
    except (ValueError, TypeError) as error:
        fail(f"Coven content API returned invalid base64: {path}: {error}")
    if hashlib.sha256(content).hexdigest() != expected_digest:
        fail(f"Coven content SHA-256 disagrees with evidence: {path}")


def verify_passed(root: pathlib.Path, markdown: str, source_rows: list[list[str]]) -> None:
    tested = field(markdown, "Tested source commit")
    run_url = field(markdown, "CI attestation")
    subprocess.run(["git", "merge-base", "--is-ancestor", tested, "HEAD"], cwd=root, check=True)
    changed = subprocess.run(
        ["git", "diff", "--name-only", f"{tested}..HEAD"], cwd=root, text=True, capture_output=True, check=True
    ).stdout.splitlines()
    if changed != ["docs/G2-EVIDENCE.md"]:
        fail(f"passed source-to-HEAD diff is not evidence-only: {changed}")
    match = re.fullmatch(r"https://github\.com/(OpenCoven)/(psyche)/actions/runs/([0-9]+)", run_url)
    if not match:
        fail("CI attestation URL is malformed")
    owner, repo, run_id = match.groups()
    run = run_json(
        ["gh", "run", "view", run_id, "--repo", f"{owner}/{repo}", "--json", "conclusion,event,headSha,url,workflowName"],
        root,
    )
    expected_run = {
        "conclusion": "success",
        "event": "pull_request",
        "headSha": tested,
        "url": run_url,
        "workflowName": "CI",
    }
    if not isinstance(run, dict) or run != expected_run:
        fail(f"CI attestation does not match the tested source: {run}")
    rest = run_json(["gh", "api", f"repos/OpenCoven/psyche/actions/runs/{run_id}"], root)
    expected_rest = {
        "conclusion": "success",
        "event": "pull_request",
        "head_sha": tested,
        "html_url": run_url,
        "id": int(run_id),
        "path": ".github/workflows/ci.yml",
        "status": "completed",
        "workflow_id": CI_WORKFLOW_ID,
    }
    if (
        not isinstance(rest, dict)
        or any(rest.get(key) != value for key, value in expected_rest.items())
        or not isinstance(rest.get("repository"), dict)
        or rest["repository"].get("full_name") != "OpenCoven/psyche"
        or not isinstance(rest.get("head_repository"), dict)
        or rest["head_repository"].get("full_name") != "OpenCoven/psyche"
    ):
        fail(f"CI REST attestation does not match the tested workflow run: {rest}")
    workflow = run_json(
        ["gh", "api", f"repos/OpenCoven/psyche/actions/workflows/{CI_WORKFLOW_ID}"],
        root,
    )
    expected_workflow = {
        "id": CI_WORKFLOW_ID,
        "name": "CI",
        "path": ".github/workflows/ci.yml",
        "state": "active",
    }
    if not isinstance(workflow, dict) or any(workflow.get(key) != value for key, value in expected_workflow.items()):
        fail(f"CI workflow metadata is not the active reviewed workflow: {workflow}")
    for _, url, digest in source_rows:
        verify_coven_blob(root, url.strip("`"), normalize_grouped(digest.strip("`"), "sha256:"))
    verify_coven_blob(root, field(markdown, "Coven plan URL"), APPROVED_PLAN_SHA256)


def validate_repository(
    root: pathlib.Path,
    *,
    manifest: Mapping[str, object] | None = None,
    evidence: str | None = None,
    listed_tests: Mapping[str, str] | None = None,
    source_overrides: Mapping[str, str] | None = None,
    verify_remote: bool = True,
) -> None:
    root = root.resolve()
    overrides = source_overrides or {}
    workflow = read_text(root, ".github/workflows/ci.yml", overrides)
    validate_ci_workflow(workflow)
    for path in ("crates/psyche-store/tests/migrations.rs", "crates/psyche-store/tests/crash.rs"):
        if not (root / path).is_file():
            fail(f"store evidence target is absent: {path}")

    manifest_data = manifest
    if manifest_data is None:
        try:
            manifest_data = json.loads(read_text(root, "scripts/g2-test-manifest.json", overrides))
        except json.JSONDecodeError as error:
            fail(f"G2 manifest is invalid JSON: {error}")
    evidence_text = evidence if evidence is not None else read_text(root, "docs/G2-EVIDENCE.md", overrides)
    status, source_rows, matrix = validate_evidence(evidence_text)
    validate_manifest(root, manifest_data, matrix, listed_tests)
    validate_sources(root, overrides)
    validate_docs(root, overrides)
    if status == "passed" and verify_remote:
        verify_passed(root, evidence_text, source_rows)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    try:
        validate_repository(root)
    except (EvidenceError, subprocess.CalledProcessError, OSError) as error:
        print(f"G2 evidence check failed: {error}", file=sys.stderr)
        return 1
    print("G2 evidence relationships verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
