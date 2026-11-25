# Contributing to Psyche

Psyche is a protocol and durable-state repository. Contributions are welcome, but compatibility and failure behavior matter as much as the happy path.

## Before starting

1. Read `README.md`, `AGENTS.md`, and `docs/ROADMAP.md`.
2. Find the issue that owns the outcome, or use the issue chooser for an ordinary defect or change request. Define the acceptance criteria, dependencies, non-goals and evidence required. [Issue chooser](https://github.com/OpenCoven/psyche/issues/new/choose)
3. For cross-repository work, read `docs/PROTOCOL-OWNERSHIP.md`.
4. Classify the change using the risk classes in `AGENTS.md`.

Do not begin an R3/R4 contract change from an unreviewed implementation assumption.

## Setup

```sh
git clone https://github.com/OpenCoven/psyche.git
cd psyche
./scripts/agent-bootstrap
./scripts/agent-check fast
```

Run both helpers in Bash. The first bootstrap needs a network connection to fill the Cargo cache. On a warm cache use `PSYCHE_BOOTSTRAP_OFFLINE=1 ./scripts/agent-bootstrap`. The pinned Rust version is declared in `rust-toolchain.toml`. Use the locked Cargo dependency graph.

## Development loop

Keep each pull request focused on one contract or capability boundary.

```sh
./scripts/agent-check fast
# make a bounded change
./scripts/agent-check fast
# run the directly affected conformance/migration/fault tests
./scripts/agent-check full
```

Fast and full accept pre-existing staged, unstaged and untracked edits and verify that checks preserve the Git-visible contents. Run one verification at a time and save concurrent edits until it finishes.

Add behavior-level tests before changing canonical records, transitions, persistence, dispatch, or recovery.

## Pull request contract

A pull request should include:

- the owning issue;
- objective and non-goals;
- current behavior and invariant;
- risk class;
- public records, schemas, migrations, or consumers affected;
- exact commands and results;
- negative, crash, restart, ambiguity, and downgrade coverage where relevant;
- generated artifacts and generator command;
- migration and rollback;
- security/privacy impact;
- remaining proof gaps;
- exact final head SHA.

A merged source diff is not enough to close a runtime or compatibility outcome. Link retained evidence.

## Compatibility

Do not:

- change a stable record or ID without versioning;
- accept unknown versions/enums by guessing;
- silently widen capabilities;
- reuse process/UI/provider identifiers as protocol IDs;
- remove compatibility reads before migration evidence exists;
- convert ambiguous or indeterminate effects into success;
- modify golden bytes without an explicit compatibility decision.

Cross-repository changes must identify producers, consumers, release order, immutable canary pins, and rollback.

## Generated files

Use canonical generators. Never hand-edit generated schemas, fixtures, packed artifacts, or evidence summaries to make a check pass.

After generation:

```sh
git diff --check
git status --short
```

Review every generated change.

## Security and protected data

Send suspected vulnerabilities to the Psyche maintainers through GitHub’s private reporting form. If the form is unavailable, use the OpenCoven/coven private advisory intake and identify OpenCoven/psyche as affected. [Report privately](https://github.com/OpenCoven/psyche/security/advisories/new) · [Security policy and fallback](SECURITY.md#reporting-a-vulnerability)

Public issues, PRs, fixtures, and evidence must not contain secrets, credentials, private keys, raw prompts, unrestricted terminal output, private repository contents, full environment dumps, infrastructure details, or unnecessary personal paths.

## Review

R2–R4 changes require review from the owner of each affected path before merge. In the ownership file, the last matching rule identifies the reviewer. [CODEOWNERS](.github/CODEOWNERS)

Resolve every current review finding and rerun required checks on the exact final head.

Keep changes reversible until compatibility and runtime evidence is complete.

### Qualify the head for delivery

For a protected change, record the source as a candidate in the current G2 scope evidence and let the ordinary PR CI qualify that exact head. A successful candidate run supplies the source SHA and immutable run URL for a follow-up commit that changes only the scope evidence to `passed`. Push the evidence-only commit to the same PR and wait for the checks on its new head. For a change covered by the manifest’s permitted paths, file modes and operations, retain the existing passed record. CI verifies whether the new tree can inherit it. [Scope manifest](scripts/g2-attestation-scope.json) · [Scope evidence](docs/G2-SCOPE-EVIDENCE.md)

The separate `Delivery readiness` commit status evaluates completed `CI` runs for ordinary pull requests and pushes to `main`. A candidate can pass qualification CI while delivery readiness fails. To deliver it, finish the evidence-only transition above. A failed CI run or invalid evidence also fails delivery readiness. Follow the status’s target URL and repair the first failed condition. After a head or base update, inspect the current proposal again. If the status is absent, confirm that the default branch contains the publisher and that CI finished for the exact head and event. These read-only commands show GitHub’s check runs and commit statuses separately. Replace `HEAD_SHA` with the full recorded SHA.

```sh
gh api --paginate repos/OpenCoven/psyche/commits/HEAD_SHA/check-runs --jq '.check_runs[] | [.name, .status, .conclusion]'
gh api --paginate repos/OpenCoven/psyche/commits/HEAD_SHA/statuses --jq '.[] | [.context, .state, .target_url]'
```

The maintainer selected ten strict contexts (three Rust jobs, three contributor jobs, dependency audit, secret guard and both npm distribution jobs), plus delivery readiness and squash-only delivery. A publisher on the default branch emits the status. Effective enforcement requires a separate settings change and an ordinary PR showing that an unmet requirement blocks merge. Administrators must follow the same path without bypass. While BunsDev is the sole eligible CODEOWNER, GitHub requires zero formal approvals under the selected policy. Record affected-owner review on the PR for R2–R4 changes. At main `31a36ede` the main ruleset readback was empty. Consult #25 for subsequent enforcement evidence. [Maintainer decision](https://github.com/OpenCoven/psyche/issues/25#issuecomment-7153733959) · [Owner review and bypass decision](https://github.com/OpenCoven/psyche/issues/25#issuecomment-7159416225) · [Repository enforcement](https://github.com/OpenCoven/psyche/issues/25)

### Deliver the qualified head

Use GitHub’s **Squash and merge** when delivering a pull request to `main`. The G2 push verifier requires the previous tip of `main` to be the delivered commit’s only parent and the delivered tree to match the reviewed PR head.

1. Read the PR’s `head.sha`, `base.sha`, and proposed `merge_commit_sha` from `gh api repos/OpenCoven/psyche/pulls/NUMBER`. Record the full SHAs. Confirm the base equals the current `main` tip and that required check runs and commit statuses passed on the recorded head.
2. Fetch the head and proposed merge with `git fetch origin refs/pull/NUMBER/head refs/pull/NUMBER/merge`. Use `git show -s --format='%P %T' SHA` to confirm the proposed merge’s parents are the recorded base and head in that order, and its tree equals the head’s tree. Refresh the PR and `main` immediately before merging. If either SHA changed or the trees differ, qualify the updated proposal first.
3. Squash-merge the tested head with `gh api --method PUT repos/OpenCoven/psyche/pulls/NUMBER/merge -f merge_method=squash -f sha=HEAD_SHA`, substituting the PR number and recorded head. Fetch the returned commit and inspect its parents and tree with the same Git command. Confirm its sole parent is the recorded base and its tree equals the tested head’s tree. Follow the push CI for that delivered SHA and confirm the G2 evidence step passes on Linux, macOS, and Windows.
