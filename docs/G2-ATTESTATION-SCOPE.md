# G2 retained-attestation scope

**Status:** active trust-root policy  
**Schema:** `psyche.g2-attestation-scope/v1`  
**Machine-readable policy:** `scripts/g2-attestation-scope.json`  
**Scope evidence:** `docs/G2-SCOPE-EVIDENCE.md`  
**Historic implementation evidence:** `docs/G2-EVIDENCE.md`

## Purpose

The historic G2 evidence proves a specific implementation, test matrix, reviewed workflow, immutable Coven inputs, and CI run. It must remain verifiable without freezing every unrelated repository file forever.

The retained-attestation scope answers one narrower question:

> Has any file that can alter the G2 contract, implementation, test, migration, fixture, dependency, toolchain, workflow, verifier, or evidence policy changed since the scope was tested?

The answer is fail-closed. A path is protected unless the versioned scope manifest explicitly classifies that exact path as outside the historic G2 claim.

## What remains protected

The default classification is `protected`. This includes, without relying on a fragile exhaustive prose list:

- all Rust crates, tests, fixtures, migrations, and schemas;
- the npm wrapper covered by the reviewed workflow;
- Cargo manifests, lockfile, Rust toolchain, clippy and dependency policy;
- CI workflows and every setup/action/command they contain;
- the historic evidence checker core and its mutation tests;
- the scope wrapper, scope tests, manifest, and scope evidence;
- the G2 test manifest;
- architecture, schema, testing, plan, and evidence documents;
- every new or unknown path.

A changed protected path invalidates the retained scope attestation until a new candidate head passes the complete reviewed CI and an evidence-only commit records that immutable run.

## Explicitly unattested paths

The manifest names a small exact set of repository-readiness and governance files that the historic G2 implementation evidence does not claim to prove. Each rule constrains the allowed operation and Git mode.

The initial set exists only to permit the roadmap/agent-readiness work tracked in #10 and draft PR #15:

- root onboarding, contribution, security, license, and agent guidance;
- the active roadmap and proposed protocol-ownership document;
- the machine-readable agent repository manifest;
- the bootstrap and fast/full verification wrappers.

Unknown paths do not inherit safety from a directory or filename pattern. Adding another unattested path changes the protected scope policy and therefore requires a new scope attestation.

## Two-layer evidence model

1. `docs/G2-EVIDENCE.md` remains the immutable historic implementation attestation. Its tested source, CI run, matrix, source hashes, and approved plan provenance do not change merely because repository-readiness files evolve.
2. `docs/G2-SCOPE-EVIDENCE.md` attests the current scope checker, scope manifest, reviewed workflow, and complete current G2 test run.

The scope evidence has two states:

- `candidate`: the current pull-request head runs the full matrix but makes no retained scope claim;
- `passed`: an evidence-only commit records the successful candidate head and immutable pull-request run.

The candidate-to-passed transition is intentionally a separate commit. The relationship checker permits that evidence-file modification and no protected source change.

## Relationship verification

For the repository's actual historic G2 evidence:

- a candidate scope validates structure, full tests, the reviewed workflow, historic run provenance, and immutable Coven inputs, but does not claim a retained source relationship;
- a passed scope verifies its own immutable successful CI run and compares the tested source tree with the current terminal tree;
- GitHub Actions uses commit/tree APIs and rejects truncated recursive trees;
- local verification uses exact Git tree entries;
- directory tree hashes are ignored, but every blob and submodule path, mode, type, and object ID is compared;
- the scope evidence may be modified in place;
- explicitly unattested files may only use their declared operations and modes;
- deletions, mode changes, symlinks, submodules, renames, copies, unknown paths, and protected changes fail closed.

The original evidence-only relationship behavior remains available for the legacy checker mutation suite and for synthetic evidence tests. It is not used to bless later protected-source changes.

## Updating the protected surface

When a protected source, test, workflow, dependency, policy, verifier, or evidence document must change:

1. open a focused R4 issue and identify the affected invariant;
2. change the scope evidence to `candidate` or introduce the candidate trust-root update in the same reviewed slice;
3. run the full CI matrix on the exact candidate head;
4. inspect all positive, denial, crash, restart, migration, supply-chain, secret, npm, and scope tests;
5. add an evidence-only commit naming the candidate source SHA and immutable successful CI run;
6. rerun CI on the evidence commit;
7. merge only after protected-owner review and retained evidence are complete.

Never add a path to the unattested set merely to make a red check green.
