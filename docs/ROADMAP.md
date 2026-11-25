# Psyche roadmap

[@BunsDev](https://github.com/BunsDev) owns the active program. This snapshot records the 25 November 2026 delivery state. Each milestone ends when its observable gate passes. [Roadmap control and handoff](https://github.com/OpenCoven/psyche/issues/9#issuecomment-7081646023) · [Original phased plan](https://github.com/OpenCoven/psyche/blob/dcd932ee6a5235a838615e1fc21ad8a83efb0093/docs/ROADMAP.md)

## Delivered foundation

The repository has a contributor entrypoint, eight-crate map, pinned toolchain, risk manifest, bootstrap and fast/full checks. The roadmap owner closed the readiness foundation after the crate map and squash verification landed. Contributors can run the local checks from the README. Historical G2 evidence remains tied to its original source and run. [Readiness closure](https://github.com/OpenCoven/psyche/issues/10#issuecomment-7117976476) · [Contributor commands](../README.md) · [Historical G2 evidence](G2-EVIDENCE.md) · [Current scope and attestation](G2-SCOPE-EVIDENCE.md)

The experimental adapter-lifecycle v2 producer landed at `dcd932ee6a5235a838615e1fc21ad8a83efb0093`. Psyche Build PR #601 at `a400364dba0fa4f6e021bd591aa5afc1f0312b79` pins producer source `9bf09b9c42e53fc38a15c7824db4f38aadb67a41` and archive SHA-256 `411932cffa050e0c73f29dac57f7b7b6b12a14e434cf76d616cf9efe37eccda3`. The open consumer PR exercises the instrumented native handler, lost acknowledgement, restart and rollback. Merge, distribution, live product activation and stable profile support remain separate gates. [Merged producer](https://github.com/OpenCoven/psyche/pull/51) · [Exact Build pin](https://github.com/OpenCoven/psyche-build/blob/a400364dba0fa4f6e021bd591aa5afc1f0312b79/protocol-fixtures/psyche-lifecycle/v2/pin.json) · [Build canary](https://github.com/OpenCoven/psyche-build/pull/601)

The experimental receipt binding shipped in [#59](https://github.com/OpenCoven/psyche/pull/59). [#62](https://github.com/OpenCoven/psyche/pull/62) delivered its offline native verifier at `f07d513eef1c093b6b145cdde4560d8a4d235763`. [Build #606](https://github.com/OpenCoven/psyche-build/pull/606) shipped the pinned canary at `dfd4b3cf2f06c7d415e647f778f007c6042fdbba`. The canary exercises exact receipt bytes and a single effect through restart and rollback, including a lost acknowledgement. The live desktop migration is still open under [Build #253](https://github.com/OpenCoven/psyche-build/issues/253).

## First executable task

Start [#11](https://github.com/OpenCoven/psyche/issues/11) with the experimental `psyche.intent.v1` schema. [#50](https://github.com/OpenCoven/psyche/pull/50) owns the sixteen canonical vectors and their digest index. [#60](https://github.com/OpenCoven/psyche/pull/60) identifies the first publication targets. Work from the current typed contract in `crates/psyche-core/src/contracts/intent.rs`, the `decode_document` entrypoint in `crates/psyche-core/src/contracts/mod.rs` and the intent fixture in `crates/psyche-core/tests/fixtures/intent-local.json`. After bootstrap and the fast gate, run `cargo test --locked -p psyche-core --test decode` for the baseline.

Add a Draft 2020-12 schema and a focused parity test. Check the [pinned #50 intent vector](https://github.com/OpenCoven/psyche/blob/2c6eb2a51677f601908120da3f57cffc4d11747d/crates/psyche-core/tests/fixtures/vectors/psyche.intent.v1.json) and its [SHA-256 entry](https://github.com/OpenCoven/psyche/blob/2c6eb2a51677f601908120da3f57cffc4d11747d/crates/psyche-core/tests/fixtures/vectors/SHA256SUMS) against both the schema and the typed decoder, preserve its exact canonical bytes and digest, and exercise unknown fields, wrong ID prefixes, unsupported major versions and malformed input. Document any validation that belongs to the parser or requires Rust semantics. The slice is ready for review when the positive and denial corpus passes offline in both implementations and the required G2 evidence is linked. Closing #11 additionally requires the rest of the approved surface, reproducible publication, and a pinned downstream compatibility and rollback canary.

## Remaining milestones

| Owning issue | Prerequisite | Next deliverable | Observable exit criterion |
|---|---|---|---|
| [#12 ownership and compatibility](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214) | First-profile producer assignments and four owner decisions | Map each historical rebind guarantee to a current producer, version, canary and denial. Return held rows 11 and 13 with containment and binding evidence. | The owner accepts each supported row against executable evidence. Identity changes on unmapped routes stay blocked. Emergency and deprecation procedures earn support through rehearsal. |
| [#11 protocol publication](https://github.com/OpenCoven/psyche/issues/11#issuecomment-7119169484) | Approved producer rows from #12 | Select the publication surface, qualify approved schemas and canonical vectors against their typed decoders, and publish one immutable checksummed profile with version negotiation and rollback. [Publication proposal](https://github.com/OpenCoven/psyche/pull/60) · [Binding implementation](https://github.com/OpenCoven/psyche/pull/59) | A downstream clean checkout pins one artifact set and verifies canonical bytes, provenance, both digests, denials, restart and downgrade without a Psyche source checkout. |
| [#13 Build reference client](https://github.com/OpenCoven/psyche/issues/13#issuecomment-7119170255) and [Build #253](https://github.com/OpenCoven/psyche-build/issues/253) | #11 prerelease verifier and profile, #12 binding policy | Preserve the shipped #606 pins and exercise the reversible desktop adapter and persistence migration. | The native flow preserves protocol IDs alongside local IDs, rejects unsupported evidence, retains the original receipt, and demonstrates desktop restart/resume and rollback at recorded pins. |
| [#18 typed Coven automation](https://github.com/OpenCoven/psyche/issues/18) | Coven #855 delivered the schema. #857 and #858 own live authority, receipt and certification evidence. [Upstream gates](https://github.com/OpenCoven/psyche/issues/18#issuecomment-7079100073) | Exercise adoption, idempotent execution and exact receipt correlation against the daemon. | Restart/concurrent recovery preserves the original identity and evidence. Real integration has retained daemon-side conformance. |
| [#19 process-attempt lifecycle](https://github.com/OpenCoven/psyche/issues/19) and [Coven #804](https://github.com/OpenCoven/coven/issues/804) | #12 fence decision and authenticated evidence from each resource owner | Run the #20 consumer model. Obtain authenticated containment evidence from the host for surviving children/tools and host restart, then review redispatch after fencing. | The host issues a fence only after every acting resource is contained. Unknown attempts stay blocked and each new dispatch requires fresh admission. |
| [#9 broader lifecycle](https://github.com/OpenCoven/psyche/issues/9) | Proven #11–#13 profile and relevant #18/#19 runtime evidence | Qualify lease issue/expiry/revocation/renewal, approvals and invalidation, action/receipt transitions, cancellation acknowledgement, artifact retention and cross-surface recovery. | Each consequential action has one durable identity across disconnect and restart, and each path yields correlated terminal evidence or an explicit unresolved outcome. |
| [#14 governance and release](https://github.com/OpenCoven/psyche/issues/14) | Contributor controls, eligible owners, settings decisions and a pinned downstream reference flow | The [contributor controls](https://github.com/OpenCoven/psyche/issues/24) shipped. At `f07d513`, [the settings readback](https://github.com/OpenCoven/psyche/issues/25#issuecomment-7160000378) found zero effective main rules, all three merge methods enabled and private reporting enabled. Once the [#65 publisher](https://github.com/OpenCoven/psyche/pull/65) emits its status on main, the maintainer can apply the [selected settings](https://github.com/OpenCoven/psyche/issues/25) and verify a refused candidate and accepted attested head against the [event-specific G2 check](https://github.com/OpenCoven/psyche/issues/58). Rehearse installed-package, support, migration, downgrade and emergency procedures. | Required checks and private reporting work on the intended surfaces. A pinned client verifies signed metadata, checksums, SBOM and provenance, and a clean install and rollback pass on claimed platforms. |

The Rust Telegram adapter belongs in Psyche. Interface and denial-vector planning can proceed. Actual integration needs G4/Coven #1230, production child dispatch needs G6, and cutover needs the applicable G10. [Telegram decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-6303089462) · [Producer assignments and remaining decisions](PROTOCOL-OWNERSHIP.md)

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
