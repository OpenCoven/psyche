# G2 Testing

G2 tests use deterministic fake scripts at adapter-neutral boundaries. A
fixture advertises availability, consumes an exact script, exposes typed fault
points, supports restart, and reports durable observations. Positive paths
must return `Verified`; negative paths reject malformed, widened, stale,
unordered, or non-durable behavior. `ExpectedUnsupported` is only a diagnostic
for a real adapter and never counts as passed scripted evidence.

Property runs set `PROPTEST_CASES` and `PROPTEST_RNG_SEED`; CI fixes them to
2048 cases and the all-zero seed. The state-machine models compare durable
store state after each operation. Crash tests inject named before/after-commit
points, kill a writer, reopen the database, and observe only committed state.
The C-S6 matrix covers immutable correlation, durable return, durable fence,
fault injection, restart, and a no-redispatch decision unless a fence makes a
new dispatch eligible.

The full-request digest suite constructs digests only through
`AdoptionRequest::new`, forces both owners to recompute them, and retains a
stale digest while mutating every typed field. Launch and input goldens require
RFC3339 string timestamps, byte-for-byte canonical JSON, no trailing newline,
and pinned SHA-256. `scripts/g2-test-manifest.json` maps each allowlisted atomic
matrix command to an exact test name. The checker lists every Cargo target and
rejects zero tests, missing names, substring filters, or unused entries.

O5 tests reject raw statuses `created`, `running`, `idle`, `completed`,
`failed`, `killed`, and `orphaned` as cancellation acknowledgement. Immutable Coven
blob URLs and SHA-256 values bind the evidence to the reviewed sources.

## Reusable conformance matrix

- C-S1 positive: exact contract/capability negotiation; negative: version or
  capability widening is a structured denial.
- C-S2 positive: session lifecycle is correlation-stable; negative: invalid
  request/session transitions do not persist.
- C-S3 positive: snapshot and Attempt binding agree; negative: mismatched
  snapshot, attempt, project, or graph correlation is rejected.
- C-S4 positive: stable adoption replays one disposition; negative: every
  full-request digest mutation and post-commit ambiguity is rejected/reconciled.
- C-S5 positive: durable non-adoption proof permits the modeled decision;
  negative: unknown or adopted dispositions never masquerade as proof.
- C-S6 positive: immutable correlation yields durable return or durable fence;
  negative: faults, restart, unresolved state, and no-redispatch without fence
  remain blocked.
- C-S7 positive: cursor pages are ordered and restart-stable; negative: gaps,
  duplicates, drift, and before/after-page faults are rejected.
- C-S8 positive: typed terminal authority persists before use; negative: raw
  terminal strings and unpersisted terminal observations are non-authoritative.
- C-S9 positive: core-owned O5 acknowledgement/unresolved evidence persists;
  negative: every raw status, correlation mismatch, and lifetime violation is
  rejected.
- C-S10 positive: strict result/artifact digest, media type, size, expiry,
  correlation, and lifetime match; negative: each independent mutation fails.
- C-S11 positive: durable state survives every declared crash point and
  restart; negative: volatile observations reset and indeterminate persistence
  cannot claim success.
- C-S12 positive: known denials preserve the canonical structured error;
  negative: unknown enums/kinds/majors quarantine rather than dispatch.

## Canonical document vectors

The canonical vector suite, `crates/psyche-core/tests/canonical_vectors.rs`,
pins one document for each of the sixteen registered schema kinds under
`crates/psyche-core/tests/fixtures/vectors/`: the fifteen durable record kinds
and the error envelope. Each `psyche.<kind>.v1.json` file is byte-for-byte the
RFC 8785 output of `canonical_bytes` for a typed synthetic document, with no
trailing newline, and its SHA-256 is a literal in the test. The suite fails
when a file, a typed construction, the encoder, or a literal drifts, when a
kind has no file, and when the directory holds a file no kind claims. Beyond
the drift checks it has three acceptance controls and four denials. The
acceptance controls show that a non-canonical spelling (for surface_event's
free-form content, also respelled numbers) and a trailing newline decode and re-encode to the pinned bytes and
digest, and that a one-byte value change still decodes but no longer carries
the pinned digest. The denials cover an unknown field, an unknown major, an
unknown schema name, and a duplicate key. The vectors are experimental inputs
to #11, not a published artifact or store-acceptable records, and the suite is
not a G2 matrix row. The directory README gives the one regeneration command;
a default test run never writes a file, and a regenerating run that creates
or changes a file always fails.

## First Build profile prerelease kit

`crates/psyche-test-support/tests/first_build_profile.rs` checks each of the
80 generated cases in `profiles/first-build-profile/denials.json` against
`decode_document`. It covers unknown and older majors and an unknown field
for every selected kind, global shape, schema, enum and unsafe-integer
rejections, and reordered or re-spaced spellings that must re-encode to the
pinned vector. It then runs the `verify-profile` runner end to end on
assembled kits. The intact kit passes. Tampered, missing, extra or linked
members, re-pinned non-canonical vectors, older and newer profile majors,
unknown profiles, widened manifest fields, undispositioned kinds,
mismatched provenance, a laundered denial and an identity canonicalization
case are each refused with their own code. The workflow
`.github/workflows/first-build-profile.yml` builds the kit on five native
targets and runs its consumer smoke. That smoke includes the ADR 4 binding
dispositions and the network-isolated receipt-replay restart witness on
Linux and macOS. The kit is an experimental prerelease, not a G2 matrix row.
