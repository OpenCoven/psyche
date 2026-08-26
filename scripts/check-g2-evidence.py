#!/usr/bin/env python3
"""Verify G2 evidence and its fail-closed retained-attestation scope."""

from __future__ import annotations

# Keep the original reviewed checker byte-for-byte and layer the scope policy
# around it. This makes the trust-root change small, reviewable, and reversible
# while retaining every existing source, workflow, matrix, and remote-run check.
import pathlib as _bootstrap_pathlib

_LEGACY_PATH = _bootstrap_pathlib.Path(__file__).with_name("check-g2-evidence-v1.py")
try:
    _legacy_source = _LEGACY_PATH.read_text(encoding="utf-8")
except OSError as error:
    raise RuntimeError(f"cannot load immutable G2 checker core: {error}") from error
_main_marker = '\nif __name__ == "__main__":\n'
if _legacy_source.count(_main_marker) != 1:
    raise RuntimeError("immutable G2 checker core has an unexpected main boundary")
exec(
    compile(_legacy_source.split(_main_marker, 1)[0], str(_LEGACY_PATH), "exec"),
    globals(),
)
del _legacy_source, _main_marker

_LEGACY_VERIFY_LOCAL_SOURCE_RELATIONSHIP = verify_local_source_relationship
_LEGACY_VALIDATE_EVIDENCE_ONLY_COMPARE = validate_evidence_only_compare
_LEGACY_VERIFY_ACTIONS_SOURCE_RELATIONSHIP = verify_actions_source_relationship
_LEGACY_VERIFY_PASSED = verify_passed
_LEGACY_VALIDATE_REPOSITORY = validate_repository

SCOPE_SCHEMA = "psyche.g2-attestation-scope/v1"
SCOPE_MANIFEST_PATH = "scripts/g2-attestation-scope.json"
SCOPE_EVIDENCE_PATH = "docs/G2-SCOPE-EVIDENCE.md"
HISTORIC_EVIDENCE_PATH = "docs/G2-EVIDENCE.md"
SCOPE_MANIFEST_SHA256 = "4e535973d4ab49de80e78cd35177bde658d5c31f6d0fd3a5d91edb579a118fa4"
_SCOPE_TEST_COMMAND = "python3 scripts/check-g2-attestation-scope-test.py"

if CI_COMMANDS[-1] != "python3 scripts/check-g2-evidence.py" or _SCOPE_TEST_COMMAND in CI_COMMANDS:
    fail("immutable G2 checker core has an unexpected CI command boundary")
CI_COMMANDS = CI_COMMANDS[:-1] + (_SCOPE_TEST_COMMAND, CI_COMMANDS[-1])
REVIEWED_WORKFLOW_SHA256 = "bce224826ebea5f2edfe9a63a45ccb332d1a20189462a5895f403c08dca3088d"

EXPECTED_UNATTESTED_PATHS = {
    "AGENTS.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "CONTRIBUTING.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "LICENSE": {"operations": ("added", "modified"), "modes": ("100644",)},
    "README.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "SECURITY.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "agent/manifest.yaml": {"operations": ("added", "modified"), "modes": ("100644",)},
    "docs/PROTOCOL-OWNERSHIP.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "docs/ROADMAP.md": {"operations": ("added", "modified"), "modes": ("100644",)},
    "scripts/agent-bootstrap": {"operations": ("added", "modified"), "modes": ("100755",)},
    "scripts/agent-check": {"operations": ("added", "modified"), "modes": ("100755",)},
}


def _json_object_without_duplicates(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            fail(f"attestation scope JSON repeats a key: {key}")
        value[key] = item
    return value


def _safe_repository_path(path: object) -> str:
    if not isinstance(path, str) or not path or "\\" in path or path.startswith("/") or path.endswith("/"):
        fail(f"attestation scope contains an unsafe path: {path}")
    pure = pathlib.PurePosixPath(path)
    if any(part in {"", ".", ".."} for part in pure.parts):
        fail(f"attestation scope contains an unsafe path: {path}")
    return path


def _read_scope_manifest(
    root: pathlib.Path,
    overrides: Mapping[str, str] | None = None,
) -> dict[str, dict[str, tuple[str, ...]]]:
    text = read_text(root, SCOPE_MANIFEST_PATH, overrides or {})
    if hashlib.sha256(text.encode("utf-8")).hexdigest() != SCOPE_MANIFEST_SHA256:
        fail("G2 attestation scope manifest SHA-256 drifted")
    try:
        raw = json.loads(text, object_pairs_hook=_json_object_without_duplicates)
    except json.JSONDecodeError as error:
        fail(f"G2 attestation scope manifest is invalid JSON: {error}")
    if not isinstance(raw, dict) or set(raw) != {
        "default", "schema_version", "scope_evidence_path", "unattested_paths"
    }:
        fail("G2 attestation scope manifest has an unexpected top-level shape")
    if raw.get("schema_version") != SCOPE_SCHEMA:
        fail("G2 attestation scope schema drifted")
    if raw.get("default") != "protected":
        fail("G2 attestation scope must default to protected")
    if raw.get("scope_evidence_path") != SCOPE_EVIDENCE_PATH:
        fail("G2 attestation scope evidence path drifted")
    paths = raw.get("unattested_paths")
    if not isinstance(paths, dict) or set(paths) != set(EXPECTED_UNATTESTED_PATHS):
        fail("G2 unattested path allowlist drifted")
    normalized: dict[str, dict[str, tuple[str, ...]]] = {}
    for raw_path, rule in paths.items():
        path = _safe_repository_path(raw_path)
        if not isinstance(rule, dict) or set(rule) != {"operations", "modes"}:
            fail(f"G2 unattested path rule has an unexpected shape: {path}")
        operations = rule.get("operations")
        modes = rule.get("modes")
        if (
            not isinstance(operations, list)
            or any(not isinstance(item, str) for item in operations)
            or not isinstance(modes, list)
            or any(not isinstance(item, str) for item in modes)
        ):
            fail(f"G2 unattested path rule is not a string-list rule: {path}")
        normalized_rule = {
            "operations": tuple(operations),
            "modes": tuple(modes),
        }
        if normalized_rule != EXPECTED_UNATTESTED_PATHS[path]:
            fail(f"G2 unattested path rule drifted: {path}")
        normalized[path] = normalized_rule
    return normalized


def _validate_scope_evidence(markdown: str) -> tuple[str, str | None, str | None]:
    labels = re.findall(r"^\*\*([^:*]+):\*\*", markdown, re.MULTILINE)
    expected_labels = {
        "Status",
        "Tested source commit",
        "CI attestation",
        "Scope schema",
        "Scope manifest",
        "Scope manifest SHA-256",
        "Historic G2 evidence",
    }
    if len(labels) != len(expected_labels) or set(labels) != expected_labels:
        fail("G2 scope evidence fields are missing, duplicated, or unexpected")
    status = field(markdown, "Status")
    if status not in {"candidate", "passed"}:
        fail("G2 scope evidence status must be candidate or passed")
    if field(markdown, "Scope schema") != SCOPE_SCHEMA:
        fail("G2 scope evidence schema drifted")
    if field(markdown, "Scope manifest") != SCOPE_MANIFEST_PATH:
        fail("G2 scope evidence manifest path drifted")
    if normalize_grouped(field(markdown, "Scope manifest SHA-256"), "sha256:") != SCOPE_MANIFEST_SHA256:
        fail("G2 scope evidence manifest digest drifted")
    if field(markdown, "Historic G2 evidence") != HISTORIC_EVIDENCE_PATH:
        fail("G2 scope evidence historic evidence path drifted")
    tested = field(markdown, "Tested source commit")
    run_url = field(markdown, "CI attestation")
    if status == "candidate":
        if tested != "not recorded before remote review" or run_url != "not recorded before remote review":
            fail("candidate G2 scope evidence must use exact remote-review placeholders")
        return status, None, None
    if not re.fullmatch(r"[0-9a-f]{40}", tested):
        fail("passed G2 scope evidence requires a 40-hex tested source")
    if not re.fullmatch(r"https://github\.com/OpenCoven/psyche/actions/runs/[0-9]+", run_url):
        fail("passed G2 scope evidence requires an immutable Actions run URL")
    if re.search(r"not recorded|pending|placeholder|TBD|TODO", markdown, re.IGNORECASE):
        fail("passed G2 scope evidence contains a candidate placeholder")
    return status, tested, run_url


def _load_scope_state(
    root: pathlib.Path,
    overrides: Mapping[str, str] | None = None,
) -> tuple[dict[str, dict[str, tuple[str, ...]]], str, str | None, str | None]:
    scope = _read_scope_manifest(root, overrides)
    evidence = read_text(root, SCOPE_EVIDENCE_PATH, overrides or {})
    status, tested, run_url = _validate_scope_evidence(evidence)
    return scope, status, tested, run_url


def _historic_tested_source(root: pathlib.Path) -> str:
    return field(read_text(root, HISTORIC_EVIDENCE_PATH, {}), "Tested source commit")


def _entry_map_from_remote_tree(tree: object, expected_tree_sha: str) -> dict[str, tuple[str, str, str]]:
    if not isinstance(tree, dict) or tree.get("sha") != expected_tree_sha:
        fail("GitHub recursive tree response does not match the requested tree")
    if tree.get("truncated") is not False:
        fail("GitHub recursive tree response is truncated or missing its truncation proof")
    entries = tree.get("tree")
    if not isinstance(entries, list):
        fail("GitHub recursive tree response has no entry list")
    result: dict[str, tuple[str, str, str]] = {}
    for raw in entries:
        if not isinstance(raw, dict):
            fail("GitHub recursive tree contains a non-object entry")
        entry_type = raw.get("type")
        if entry_type == "tree":
            continue
        path = _safe_repository_path(raw.get("path"))
        mode = raw.get("mode")
        sha = raw.get("sha")
        if (
            entry_type not in {"blob", "commit"}
            or not isinstance(mode, str)
            or not re.fullmatch(r"[0-9]{6}", mode)
            or not isinstance(sha, str)
            or not re.fullmatch(r"[0-9a-f]{40}", sha)
            or path in result
        ):
            fail(f"GitHub recursive tree contains an invalid or duplicate entry: {path}")
        result[path] = (mode, entry_type, sha)
    return result


def _remote_tree_entries(root: pathlib.Path, commit: str) -> dict[str, tuple[str, str, str]]:
    commit_data = run_json(["gh", "api", f"repos/OpenCoven/psyche/git/commits/{commit}"], root)
    tree = commit_data.get("tree") if isinstance(commit_data, dict) else None
    tree_sha = tree.get("sha") if isinstance(tree, dict) else None
    if (
        not isinstance(commit_data, dict)
        or commit_data.get("sha") != commit
        or not isinstance(tree_sha, str)
        or not re.fullmatch(r"[0-9a-f]{40}", tree_sha)
    ):
        fail("GitHub commit response does not identify the requested commit tree")
    response = run_json(
        ["gh", "api", f"repos/OpenCoven/psyche/git/trees/{tree_sha}?recursive=1"],
        root,
    )
    return _entry_map_from_remote_tree(response, tree_sha)


def _local_tree_entries(root: pathlib.Path, ref: str) -> dict[str, tuple[str, str, str]]:
    completed = subprocess.run(
        ["git", "ls-tree", "-r", "-z", "--full-tree", ref],
        cwd=root,
        capture_output=True,
        check=False,
    )
    if completed.returncode:
        stderr = completed.stderr.decode("utf-8", "replace") if isinstance(completed.stderr, bytes) else str(completed.stderr)
        fail(f"cannot read Git tree for retained G2 scope at {ref}: {stderr.strip()}")
    output = completed.stdout if isinstance(completed.stdout, bytes) else str(completed.stdout).encode()
    result: dict[str, tuple[str, str, str]] = {}
    for record in output.split(b"\0"):
        if not record:
            continue
        try:
            metadata, raw_path = record.split(b"\t", 1)
            mode, entry_type, sha = metadata.decode("ascii").split(" ", 2)
            path = raw_path.decode("utf-8")
        except (ValueError, UnicodeDecodeError) as error:
            fail(f"local Git tree contains an unparseable entry: {error}")
        path = _safe_repository_path(path)
        if (
            entry_type not in {"blob", "commit"}
            or not re.fullmatch(r"[0-9]{6}", mode)
            or not re.fullmatch(r"[0-9a-f]{40}", sha)
            or path in result
        ):
            fail(f"local Git tree contains an invalid or duplicate entry: {path}")
        result[path] = (mode, entry_type, sha)
    return result


def _change_status(
    before: tuple[str, str, str] | None,
    after: tuple[str, str, str] | None,
) -> str:
    if before is None:
        return "added"
    if after is None:
        return "deleted"
    return "modified"


def _validate_attested_changes(
    before: Mapping[str, tuple[str, str, str]],
    after: Mapping[str, tuple[str, str, str]],
    scope: Mapping[str, Mapping[str, tuple[str, ...]]],
) -> list[tuple[str, str]]:
    changes: list[tuple[str, str]] = []
    rejected: list[tuple[str, str]] = []
    for path in sorted(set(before) | set(after)):
        old = before.get(path)
        new = after.get(path)
        if old == new:
            continue
        status = _change_status(old, new)
        changes.append((path, status))
        if path == SCOPE_EVIDENCE_PATH:
            if (
                status != "modified"
                or old is None
                or new is None
                or old[0] != "100644"
                or new[0] != "100644"
                or old[1] != "blob"
                or new[1] != "blob"
            ):
                rejected.append((path, status))
            continue
        rule = scope.get(path)
        if (
            rule is None
            or status not in rule["operations"]
            or new is None
            or new[1] != "blob"
            or new[0] not in rule["modes"]
        ):
            rejected.append((path, status))
    if rejected:
        rendered = ", ".join(f"{path} ({status})" for path, status in rejected[:20])
        suffix = "" if len(rejected) <= 20 else f", and {len(rejected) - 20} more"
        fail(f"retained G2 attestation does not cover changed paths: {rendered}{suffix}")
    return changes


def _compare_common(compare: object, tested: str, terminal: str) -> tuple[str, list[dict[str, object]]]:
    if not isinstance(compare, dict):
        fail("GitHub compare response is invalid")
    base = compare.get("base_commit")
    merge_base = compare.get("merge_base_commit")
    commits = compare.get("commits")
    if not isinstance(base, dict) or base.get("sha") != tested:
        fail("GitHub compare response does not match the tested source")
    if (
        not isinstance(commits, list)
        or not commits
        or not isinstance(commits[-1], dict)
        or commits[-1].get("sha") != terminal
    ):
        fail("GitHub compare response does not end at the pull-request terminal commit")
    if compare.get("ahead_by") != len(commits) or compare.get("total_commits") != len(commits):
        fail("GitHub compare response commit counts are inconsistent or truncated")
    status = compare.get("status")
    if status not in {"ahead", "diverged"}:
        fail("GitHub compare response has an unsupported source relationship")
    if (
        not isinstance(merge_base, dict)
        or not isinstance(merge_base.get("sha"), str)
        or not re.fullmatch(r"[0-9a-f]{40}", merge_base["sha"])
    ):
        fail("GitHub compare response has no valid merge base")
    files = compare.get("files")
    if not isinstance(files, list) or not files or len(files) >= 300:
        fail("GitHub compare file list is absent or potentially truncated")
    normalized: list[dict[str, object]] = []
    seen: set[str] = set()
    for raw in files:
        if not isinstance(raw, dict):
            fail("GitHub compare file list contains a non-object entry")
        path = _safe_repository_path(raw.get("filename"))
        file_status = raw.get("status")
        if path in seen or file_status not in {"added", "modified", "removed", "renamed", "copied", "changed"}:
            fail(f"GitHub compare file list contains an invalid or duplicate entry: {raw}")
        if file_status in {"renamed", "copied"}:
            fail(f"GitHub compare uses an unsupported rename/copy status: {path}")
        seen.add(path)
        normalized.append(raw)
    return str(status), normalized


def _compare_requires_tree_scope(
    compare: object,
    tested: str,
    terminal: str,
    scope: Mapping[str, Mapping[str, tuple[str, ...]]],
) -> bool:
    status, files = _compare_common(compare, tested, terminal)
    exact_legacy_evidence = (
        len(files) == 1
        and files[0].get("filename") == HISTORIC_EVIDENCE_PATH
        and files[0].get("status") == "modified"
    )
    merge_base = compare["merge_base_commit"]
    is_ancestor = status == "ahead" and isinstance(merge_base, dict) and merge_base.get("sha") == tested
    if exact_legacy_evidence:
        if is_ancestor:
            return False
        fail("tested source is not the pull-request head's merge-base ancestor")
    for raw in files:
        path = str(raw["filename"])
        file_status = str(raw["status"])
        if path == SCOPE_EVIDENCE_PATH:
            if file_status != "modified":
                fail(f"passed source-to-pull-request diff has an invalid scope evidence change: {raw}")
            continue
        rule = scope.get(path)
        translated = "deleted" if file_status == "removed" else file_status
        if rule is None or translated not in rule["operations"]:
            if is_ancestor:
                fail(f"passed source-to-pull-request diff is not one modified evidence file: {files}")
            fail(f"retained G2 attestation does not cover compare path: {path} ({file_status})")
    return True


def validate_evidence_only_compare(compare: object, tested: str, terminal: str) -> None:
    """Retain the historic exact-evidence contract used by the v1 mutation tests."""
    _LEGACY_VALIDATE_EVIDENCE_ONLY_COMPARE(compare, tested, terminal)


def _scope_relationship_for_actual_evidence(root: pathlib.Path, tested: str) -> tuple[
    dict[str, dict[str, tuple[str, ...]]], str, str | None
] | None:
    if tested != _historic_tested_source(root):
        return None
    scope, status, scope_tested, _ = _load_scope_state(root)
    return scope, status, scope_tested


def verify_local_source_relationship(root: pathlib.Path, tested: str) -> None:
    state = _scope_relationship_for_actual_evidence(root, tested)
    if state is None:
        _LEGACY_VERIFY_LOCAL_SOURCE_RELATIONSHIP(root, tested)
        return
    scope, status, scope_tested = state
    if status == "candidate":
        return
    assert scope_tested is not None
    before = _local_tree_entries(root, scope_tested)
    after = _local_tree_entries(root, "HEAD")
    _validate_attested_changes(before, after, scope)


def _actions_terminal(root: pathlib.Path) -> str:
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        fail("GitHub Actions event payload path is absent")
    try:
        event = json.loads(pathlib.Path(event_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"GitHub Actions event payload is invalid: {error}")
    if not isinstance(event, dict):
        fail("GitHub Actions event payload is not an object")
    repository = event.get("repository")
    if not isinstance(repository, dict) or repository.get("full_name") != "OpenCoven/psyche":
        fail("GitHub Actions repository provenance is invalid")
    pull_request = event.get("pull_request")
    if isinstance(pull_request, dict):
        base = pull_request.get("base")
        head = pull_request.get("head")
        base_repository = base.get("repo") if isinstance(base, dict) else None
        head_repository = head.get("repo") if isinstance(head, dict) else None
        terminal = head.get("sha") if isinstance(head, dict) else None
        if (
            not isinstance(base, dict)
            or base.get("ref") != "main"
            or not isinstance(base_repository, dict)
            or base_repository.get("full_name") != "OpenCoven/psyche"
            or not isinstance(head_repository, dict)
            or head_repository.get("full_name") != "OpenCoven/psyche"
        ):
            fail("GitHub Actions pull-request repository/base provenance is invalid")
        if not isinstance(terminal, str) or not re.fullmatch(r"[0-9a-f]{40}", terminal):
            fail("GitHub Actions pull-request head provenance is invalid")
        return terminal
    return squash_merge_terminal(root, event)


def verify_actions_source_relationship(root: pathlib.Path, tested: str) -> None:
    state = _scope_relationship_for_actual_evidence(root, tested)
    if state is None:
        _LEGACY_VERIFY_ACTIONS_SOURCE_RELATIONSHIP(root, tested)
        return
    scope, status, scope_tested = state
    if status == "candidate":
        return
    assert scope_tested is not None
    terminal = _actions_terminal(root)
    compare = run_json(
        ["gh", "api", f"repos/OpenCoven/psyche/compare/{scope_tested}...{terminal}"],
        root,
    )
    needs_trees = _compare_requires_tree_scope(compare, scope_tested, terminal, scope)
    if not needs_trees:
        return
    before = _remote_tree_entries(root, scope_tested)
    after = _remote_tree_entries(root, terminal)
    _validate_attested_changes(before, after, scope)


def _verify_scope_ci_attestation(root: pathlib.Path, tested: str, run_url: str) -> None:
    match = re.fullmatch(r"https://github\.com/(OpenCoven)/(psyche)/actions/runs/([0-9]+)", run_url)
    if not match:
        fail("G2 scope CI attestation URL is malformed")
    owner, repo, run_id = match.groups()
    run = run_json(
        ["gh", "run", "view", run_id, "--repo", f"{owner}/{repo}", "--json",
         "conclusion,event,headSha,url,workflowName"],
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
        fail(f"G2 scope CI attestation does not match the tested source: {run}")
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
        fail(f"G2 scope CI REST attestation does not match the tested workflow run: {rest}")
    workflow_metadata = run_json(
        ["gh", "api", f"repos/OpenCoven/psyche/actions/workflows/{CI_WORKFLOW_ID}"],
        root,
    )
    expected_workflow = {
        "id": CI_WORKFLOW_ID,
        "name": "CI",
        "path": ".github/workflows/ci.yml",
        "state": "active",
    }
    if (
        not isinstance(workflow_metadata, dict)
        or any(workflow_metadata.get(key) != value for key, value in expected_workflow.items())
    ):
        fail(f"G2 scope CI workflow metadata is not the active reviewed workflow: {workflow_metadata}")


def verify_passed(root: pathlib.Path, markdown: str, source_rows: list[list[str]]) -> None:
    tested = field(markdown, "Tested source commit")
    _LEGACY_VERIFY_PASSED(root, markdown, source_rows)
    if tested != _historic_tested_source(root):
        return
    _, status, scope_tested, run_url = _load_scope_state(root)
    if status == "passed":
        assert scope_tested is not None and run_url is not None
        _verify_scope_ci_attestation(root, scope_tested, run_url)


def validate_repository(
    root: pathlib.Path,
    *,
    manifest: Mapping[str, object] | None = None,
    evidence: str | None = None,
    listed_tests: Mapping[str, str] | None = None,
    source_overrides: Mapping[str, str] | None = None,
    verify_remote: bool = True,
) -> None:
    _load_scope_state(root.resolve(), source_overrides or {})
    _LEGACY_VALIDATE_REPOSITORY(
        root,
        manifest=manifest,
        evidence=evidence,
        listed_tests=listed_tests,
        source_overrides=source_overrides,
        verify_remote=verify_remote,
    )


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    try:
        validate_repository(root)
        _, scope_status, _, _ = _load_scope_state(root)
    except (EvidenceError, subprocess.CalledProcessError, OSError) as error:
        print(f"G2 evidence check failed: {error}", file=sys.stderr)
        return 1
    print(f"G2 evidence relationships verified; scope attestation {scope_status}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
