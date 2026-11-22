## Scope and owner

Link the owning issue. Name the objective, non-goals, risk class (R0–R4 in `AGENTS.md`), files intentionally changed, protected paths touched and reviewer.

## Contract and consumer impact

Describe the current behavior and the invariant preserved, and identify the affected producers, consumers, schemas, persisted state and generated artifacts. Include the generator command or explain why it does not apply. State any security or privacy impact.

## Verification

Record the exact head SHA, commands, results, and remaining proof gaps, including unavailable platform evidence. Link relevant CI, downstream canaries, and negative, crash, restart, ambiguity and downgrade cases where relevant. Include sanitized summaries only.

## Migration and rollback

Describe compatibility, migration, rollback and any approval needed before effects become irreversible.

## G2 scope

Every path is protected unless `scripts/g2-attestation-scope.json` lists it. For a protected path, set `docs/G2-SCOPE-EVIDENCE.md` to `candidate` in this change and record the passing run in an evidence-only commit before merge, as `docs/G2-ATTESTATION-SCOPE.md` describes. Otherwise state that no protected path changed.
