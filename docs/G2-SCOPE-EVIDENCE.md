# G2 Attestation Scope Evidence

**Status:** candidate
**Tested source commit:** not recorded before remote review
**CI attestation:** not recorded before remote review
**Scope schema:** `psyche.g2-attestation-scope/v1`
**Scope manifest:** `scripts/g2-attestation-scope.json`
**Scope manifest SHA-256:** `sha256:4e535973d4ab49de80e78cd35177bde658d5c31f6d0fd3a5d91edb579a118fa4`
**Historic G2 evidence:** `docs/G2-EVIDENCE.md`

This document attests the fail-closed path scope used to determine whether the historic G2 implementation evidence still applies to a later repository tree. A candidate proves the checker, full G2 suite, workflow, and scope on its own pull-request head; it does not become the retained scope attestation until an evidence-only commit records that successful immutable run.
