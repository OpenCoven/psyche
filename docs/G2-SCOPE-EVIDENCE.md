# G2 Attestation Scope Evidence

**Status:** passed
**Tested source commit:** `cbb840e2b6bd4b1046f96c3ed186279a46957670`
**CI attestation:** https://github.com/OpenCoven/psyche/actions/runs/45048346459
**Scope schema:** `psyche.g2-attestation-scope/v1`
**Scope manifest:** `scripts/g2-attestation-scope.json`
**Scope manifest SHA-256:** `sha256:a562eb016dfd658087a688e738ee5d20f275a3afc43486dbf0bc6d8bfb7f7809`
**Historic G2 evidence:** `docs/G2-EVIDENCE.md`

This document attests the fail-closed path scope used to determine whether the historic G2 implementation evidence still applies to a later repository tree. A candidate proves the checker, full G2 suite, workflow, and scope on its own pull-request head; it does not become the retained scope attestation until an evidence-only commit records that successful immutable run.
