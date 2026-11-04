# Psyche roadmap

**Status:** active program  
**Last reconciled:** 2026-08-26  
**Roadmap control:** [OpenCoven/psyche#9](https://github.com/OpenCoven/psyche/issues/9)  
**Owner:** [@BunsDev](https://github.com/BunsDev)

Psyche is graduating from a strong G2 contract foundation into OpenCoven's consumer-ready canonical orchestration protocol.

The roadmap is gate-driven. Target dates coordinate work; they never replace compatibility, security, recovery, or retained-evidence requirements.

## Baseline

Implemented and evidenced today:

- canonical records, IDs, validation, serialization, digests, and structured denials;
- append-only durable storage, migrations, quarantine, retention, and crash recovery;
- typed Coven and bounded surface ports;
- runtime and CLI composition;
- deterministic property, conformance, migration, crash, restart, and evidence checks;
- Linux, macOS, Windows, dependency-audit, secret, and npm CI surfaces.

The current foundation is not yet a consumer-ready protocol release. Stable schemas, immutable profile artifacts, downstream pins, compatibility policy, reference-client canaries, support windows, and release governance remain explicit work.

## Program outcomes

| Issue | Outcome | Priority | Phase |
|---|---|---:|---:|
| [#10](https://github.com/OpenCoven/psyche/issues/10) | Repository and agent-readiness foundation | P0 | 0 |
| [#11](https://github.com/OpenCoven/psyche/issues/11) | Protocol v1 schemas, golden vectors, and conformance runner | P0 | 1 |
| [#12](https://github.com/OpenCoven/psyche/issues/12) | Cross-repository ownership and compatibility | P0 | 0–1 |
| [#13](https://github.com/OpenCoven/psyche/issues/13) | Psyche Build reference-client conformance | P1 | 2 |
| [#14](https://github.com/OpenCoven/psyche/issues/14) | Security, release, support, and governance readiness | P1 | 0 and 4 |

## Phase 0 — control and agent readiness

**Target:** Aug 26–Sep 8, 2026  
**Owners:** #10, #12, and the minimum security floor in #14

Deliver:

- root README, agent guide, contribution/security/license surfaces;
- machine-readable repository role and risk manifest;
- one bootstrap and fast/full verification interface;
- active roadmap and ownership map;
- protected/generated path policy;
- initial cross-repository decision record;
- minimum security reporting and ownership controls.

**Exit gate:** a clean checkout can be understood and verified without private maintainer context, and no canonical ownership remains implicit.

## Phase 1 — protocol v1 publication

**Target:** Sep 9–Sep 29, 2026  
**Owners:** #11 and #12

Deliver:

- public record/schema inventory with stability classification;
- versioned machine-readable schemas;
- byte-exact canonical JSON and digest vectors;
- positive, denial, ambiguity, restart, downgrade, and unknown-version fixtures;
- standalone bounded conformance runner;
- compatibility, migration, downgrade, deprecation, and rollback policy;
- immutable, checksummed consumer profile artifacts.

**Exit gate:** a downstream repository can pin one immutable artifact set and run conformance without copying internal Rust types or checking out Psyche source.

## Phase 2 — Psyche Build reference client

**Target:** Sep 30–Oct 20, 2026  
**Owners:** #13 and [OpenCoven/psyche-build#253](https://github.com/OpenCoven/psyche-build/issues/253)

Deliver:

- approved mapping of Psyche Build state to Psyche contracts;
- immutable profile pin and digest verification;
- positive and negative compatibility canaries;
- protocol-owned IDs introduced alongside product-local IDs;
- reversible adapter/persistence migrations;
- desktop restart/resume reference flow.

**Exit gate:** Psyche Build passes the pinned profile without making panes, worktrees, tmux, providers, Beads, GitHub, transports, or UI selections protocol identity.

## Phase 3 — lifecycle completion

**Target:** Oct 21–Nov 10, 2026

Deliver executable lifecycle semantics for:

- capability/lease issuance, expiry, revocation, and renewal;
- approval request, resolution, invalidation, and audit correlation;
- action/receipt pending, accepted, executing, succeeded, failed, unknown, and recovery-required states;
- cancellation acknowledgement and unresolved outcomes;
- retry/idempotency, ambiguity reconciliation, and fencing;
- artifact/evidence correlation and retention;
- cross-surface disconnect/restart/resume.

**Exit gate:** consequential work has one durable identity and terminal/recovery path across restarts and adapters; no surface infers truth from activity alone.

## Phase 4 — release candidate and governance

**Target:** Nov 11–Nov 24, 2026  
**Owner:** #14

Deliver:

- protected owners and enforced required checks;
- release/version/support policy;
- packed clean-install verification;
- checksums, SBOM, provenance, and signed immutable metadata;
- downstream compatibility matrix and canary evidence;
- migration, downgrade, rollback, deprecation, and emergency procedures;
- one end-to-end OpenCoven reference flow.

**Exit gate:** the release candidate is reproducible, supportable, reversible, and consumable by a pinned downstream client.

## Critical path

1. #10 makes the repository operable.
2. #12 settles boundaries while #10 lands.
3. #11 publishes stable consumer artifacts after #10/#12.
4. #13 pins those artifacts in Psyche Build.
5. lifecycle completion builds on the proven profile.
6. #14 rehearses and publishes the release candidate.

Minimum security work may run in parallel. Psyche Build mapping may begin before protocol publication, but no floating or invented profile may be treated as canonical.

## Metrics

| Metric | Target |
|---|---:|
| Active repo has root agent entrypoint and manifest | 100% |
| Clean-clone bootstrap success on claimed platforms | 100% |
| Stable records with schema and canonical vector | 100% |
| Stable transitions with positive and denial vectors | 100% |
| Unknown/widened inputs that fail closed | 100% |
| Required consumer canaries pinned immutably | 100% |
| Consequential effects with durable receipt/recovery state | 100% |
| Contract releases with migration and rollback guidance | 100% |
| Ambiguous canonical ownerships | 0 |
| Stable identities derived from implementation-local IDs | 0 |

## Tracking and closure

GitHub issues own public outcomes and acceptance gates. Pull requests own reviewable slices. Immutable artifacts and retained test/runtime evidence own completion claims.

A child issue closes only when its evidence is linked. The roadmap issue closes only after a successor release/maintenance mechanism is explicit.
