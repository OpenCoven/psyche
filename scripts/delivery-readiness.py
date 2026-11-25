"""Evaluate a completed CI run using only default-branch code and GitHub data."""

import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("g2", ROOT / "scripts/check-g2-evidence.py")
g2 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(g2)
REPO = "OpenCoven/psyche"


def api(path):
    return g2.run_json(["gh", "api", f"repos/{REPO}/{path}"], ROOT)


def read_blob(entries, path):
    mode, kind, sha = entries[path]
    if (mode, kind) != ("100644", "blob"):
        g2.fail(f"Expected regular file at {path}")
    blob = api(f"git/blobs/{sha}")
    if blob["sha"] != sha or blob["encoding"] != "base64" or blob["size"] > 65536:
        g2.fail(f"Invalid Git blob for {path}")
    return base64.b64decode("".join(blob["content"].splitlines()), validate=True).decode("utf-8")


def review_head(run):
    head = run["head_sha"]
    if (not re.fullmatch("[0-9a-f]{40}", head) or run["workflow_id"] != g2.CI_WORKFLOW_ID
            or run["repository"]["full_name"] != REPO or run["head_repository"]["full_name"] != REPO
            or run["path"] != ".github/workflows/ci.yml" or run["status"] != "completed"
            or run["conclusion"] != "success"):
        g2.fail("Delivery requires successful CI from the reviewed repository and workflow")
    if run["event"] == "push" and run["head_branch"] == "main":
        parents = api(f"git/commits/{head}")["parents"]
        if len(parents) != 1:
            g2.fail("Main delivery requires a single-parent squash commit")
        base = parents[0]["sha"]
        terminal = g2.squash_merge_terminal(ROOT, dict(ref="refs/heads/main", before=base, after=head))
        return base, terminal
    if run["event"] != "pull_request":
        g2.fail("Expected pull-request CI or a main push")
    pulls = api(f"commits/{head}/pulls")
    bases = {p["base"]["sha"] for p in pulls if p["head"]["sha"] == head
             and p["head"]["repo"]["full_name"] == REPO and p["base"]["ref"] == "main"
             and p["base"]["repo"]["full_name"] == REPO}
    if len(bases) != 1:
        g2.fail("No unique repository pull-request base for this CI head")
    base = bases.pop()
    if not re.fullmatch("[0-9a-f]{40}", base):
        g2.fail("Invalid pull-request base")
    return api(f"compare/{base}...{head}")["merge_base_commit"]["sha"], head


def verify_job_coverage(run_id):
    jobs = api(f"actions/runs/{run_id}/jobs?per_page=100")
    rust_steps = {"Format", "Clippy", "Tests", "G2 state machine (fixed seed)", "G2 reusable conformance",
                  "G2 migrations", "G2 crash recovery", "G2 fault-injection clippy", "G2 evidence checker unit tests",
                  "G2 attestation scope tests", "G2 evidence relationships"}
    expected = {f"Rust checks ({os_name}-latest)": rust_steps for os_name in ("ubuntu", "macos", "windows")}
    expected.update({"Dependency audit": {"Audit"}, "Secret guard": {"Scan history"},
                     "npm distribution (Node 20)": {"Wrapper tests", "Pack dry run"},
                     "npm distribution (Node 22)": {"Wrapper tests", "Pack dry run"}})
    if jobs["total_count"] != len(expected) or len(jobs["jobs"]) != len(expected):
        g2.fail("CI job inventory is incomplete or unexpected")
    for job in jobs["jobs"]:
        required = expected.pop(job["name"], None)
        successful = {step["name"] for step in job["steps"] if step["conclusion"] == "success"}
        if (required is None or job["run_id"] != run_id or job["conclusion"] != "success"
                or not required.issubset(successful)):
            g2.fail("CI job or required step did not pass")


def verify_transition(base_entries, before, after, tested, terminal):
    evidence = g2.SCOPE_EVIDENCE_PATH
    if base_entries.get(evidence) == after[evidence]:
        return
    parents = api(f"git/commits/{terminal}")["parents"]
    changed = {path for path in before.keys() | after.keys() if before.get(path) != after.get(path)}
    if changed != {evidence} or [p["sha"] for p in parents] != [tested]:
        g2.fail("A new passing attestation must be an evidence-only child of the qualified source")


def verify_proposal_operations(base, terminal, prior, after, scope):
    # Local Git detects copies from unchanged paths that the REST file list can omit.
    # Only fetch objects. Proposed files never materialize or execute.
    with TemporaryDirectory() as directory:
        repo = Path(directory)
        subprocess.run(["git", "init", "--bare", "--quiet", directory], check=True)
        subprocess.run(["git", "fetch", "--quiet", "--depth=1", "--no-tags", f"https://github.com/{REPO}.git",
                        base, terminal], cwd=repo, check=True)
        subprocess.run(["git", "update-ref", "HEAD", terminal], cwd=repo, check=True)
        g2._validate_readiness_operations(repo, base, prior, after, scope)


def evaluate(run):
    base, terminal = review_head(run)
    if not re.fullmatch("[0-9a-f]{40}", base):
        g2.fail("Invalid proposal merge base")
    after = g2._remote_tree_entries(ROOT, terminal)
    evidence = read_blob(after, g2.SCOPE_EVIDENCE_PATH)
    status, tested, run_url = g2._validate_scope_evidence(evidence)
    if status != "passed":
        g2.fail("Qualification candidate: record passing source CI in an evidence-only commit")
    g2.validate_ci_workflow(read_blob(after, ".github/workflows/ci.yml"))
    g2._verify_scope_ci_attestation(ROOT, tested, run_url)
    before = g2._remote_tree_entries(ROOT, tested)
    candidate, _, _ = g2._validate_scope_evidence(read_blob(before, g2.SCOPE_EVIDENCE_PATH))
    if candidate != "candidate":
        g2.fail("Tested source must carry the qualification candidate record")
    verify_job_coverage(int(run_url.rsplit("/", 1)[1]))
    prior = g2._remote_tree_entries(ROOT, base)
    verify_transition(prior, before, after, tested, terminal)
    scope = g2._read_scope_manifest(ROOT, {g2.SCOPE_MANIFEST_PATH: read_blob(after, g2.SCOPE_MANIFEST_PATH)})
    compare = api(f"compare/{tested}...{terminal}")
    g2._validate_scope_relationship(compare, tested, terminal, scope, before, after)
    verify_proposal_operations(base, terminal, prior, after, scope)


if __name__ == "__main__":
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as event_file:
        evaluate(json.load(event_file)["workflow_run"])
