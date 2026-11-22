# Code map

Lookup tables from a concept to its defining file, its store location and the tests that pin it. Policy stays in `AGENTS.md`; this file only says where things are.

`scripts/check-agent-guide.py` verifies this file wherever it runs. Keep to these rules when editing:

- A backticked token containing a slash is a repository path and must exist. Write full paths from the repository root.
- A path followed by ` :: ` and backticked names, as in `crates/psyche-core/src/digest.rs` :: `digest`, `canonical_bytes`, lists symbols that the file must declare. In Rust files a lowercase name needs `fn`, `mod` or `macro_rules!`, an uppercase name needs `const` or `static`, a type name needs `struct`, `enum`, `trait` or `type`, and `Owner::member` checks the member (function or field) with both names present. In Python files a lowercase name needs `def`. Elsewhere the text must occur as a whole word.
- The record-kind table must equal `RecordKind`, its `prefix()` arms and the schema registry.
- No line numbers. Where no test or vector exists, say so.

## Record kinds

Fifteen `RecordKind` variants, sixteen schema strings. The sixteenth, `psyche.error.v1`, decodes to `ErrorEnvelope` and is never stored.

| `RecordKind` | Prefix | Schema string | Rust type | Table |
|---|---|---|---|---|
| `IdentitySnapshot` | `ids_` | `psyche.identity_snapshot.v1` | `crates/psyche-core/src/contracts/identity.rs` :: `IdentitySnapshot` | `canonical_records` |
| `Intent` | `int_` | `psyche.intent.v1` | `crates/psyche-core/src/contracts/intent.rs` :: `Intent` | `canonical_records` |
| `Graph` | `grf_` | `psyche.graph.v1` | `crates/psyche-core/src/contracts/graph.rs` :: `Graph` | `canonical_records` |
| `GraphNode` | `nod_` | `psyche.graph_node.v1` | `crates/psyche-core/src/contracts/graph.rs` :: `GraphNode` | `canonical_records` |
| `Attempt` | `att_` | `psyche.execution_binding.v1` | `crates/psyche-core/src/contracts/execution.rs` :: `ExecutionBinding` | `execution_binding_revisions` |
| `Delegation` | `dlg_` | `psyche.delegation.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Delegation` | `canonical_records` |
| `Budget` | `bud_` | `psyche.budget.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Budget` | `canonical_records` |
| `Approval` | `apr_` | `psyche.approval.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Approval` | `canonical_records` |
| `Evidence` | `evd_` | `psyche.evidence.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Evidence` | `canonical_records` |
| `Verdict` | `vrd_` | `psyche.verdict.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Verdict` | `canonical_records` |
| `Recovery` | `rcv_` | `psyche.recovery.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Recovery` | `canonical_records` |
| `Addon` | `adn_` | `psyche.addon.v1` | `crates/psyche-core/src/contracts/foundation.rs` :: `Addon` | `canonical_records` |
| `SurfaceEvent` | `sev_` | `psyche.surface_event.v1` | `crates/psyche-core/src/contracts/surface.rs` :: `SurfaceEvent` | `canonical_records` |
| `SurfaceEffect` | `sfx_` | `psyche.surface_effect.v1` | `crates/psyche-core/src/contracts/surface.rs` :: `SurfaceEffect` | `canonical_records` |
| `Delivery` | `del_` | `psyche.delivery.v1` | `crates/psyche-core/src/contracts/surface.rs` :: `Delivery` | `canonical_records` |

- Registry and decoder: `crates/psyche-core/src/contracts/mod.rs` :: `RecordKind`, `RecordKind::ALL`, `RecordKind::prefix`, `SchemaKind`, `SchemaKind::ALL`, `SchemaKind::name`, `SchemaKind::record_kind`, `SchemaVersion::parse`, `CanonicalDocument`, `decode_document`, `VersionedRecord`.
- Identifiers: `crates/psyche-core/src/id.rs` :: `RecordId::parse`, `RecordId::parse_any`, `RecordId::kind`, `RequestId` (`req_`, never a record).
- Table definitions: `crates/psyche-store/migrations/001_foundation.sql` :: `canonical_records`, `execution_binding_revisions`, `transitions`, `quarantine_records`, `audit_events`, `schema_migrations`.
- `canonical_records.kind` holds the schema name (`identity_snapshot`, `graph_node`), not the `RecordKind` variant: `crates/psyche-store/src/records.rs` :: `Store::ingest`, `Store::insert`, `Store::load`, `Store::load_canonical_bytes`, `insert_canonical_in_transaction`, `validate_stored_canonical_record`, `kind_key`.
- `Attempt` never enters `canonical_records`; a row there with kind `execution_binding` is reported as corruption. Its append-only revisions: `crates/psyche-store/src/execution_bindings.rs` :: `insert_in_transaction`, `validate_revision_chain`, `validate_next_revision`, `Store::execution_binding_revisions`.
- Durable but not record kinds: `crates/psyche-store/src/transitions.rs` :: `Transition` (table `transitions`); `crates/psyche-store/src/quarantine.rs` :: `QuarantineId`, `QuarantineRecord`, `AuditEvent` (`qua_`, tables `quarantine_records` and `audit_events`).

## Adding or changing a record kind

Edit sites, in order. The compiler forces the exhaustive `match` arms; it does not force the string tables, the tests or the checker counts.

1. Type: a `validated_struct!` block, `validate` and a `VersionedRecord` impl in `crates/psyche-core/src/contracts/`, plus its `pub use`. Sibling to copy: `crates/psyche-core/src/contracts/foundation.rs` :: `Addon`, `versioned`. Macro: `crates/psyche-core/src/contracts/mod.rs` :: `validated_struct`.
2. Core enums and matches, all in `crates/psyche-core/src/contracts/mod.rs` :: `RecordKind`, `RecordKind::ALL`, `RecordKind::prefix`, `SchemaKind`, `SchemaKind::ALL`, `SchemaKind::name`, `SchemaKind::record_kind`, `CanonicalDocument`, `CanonicalDocument::validate`, `CanonicalDocument::schema_version`, `CanonicalDocument::persistable_record_id`, `decode_document`, `inspect_typed_enums`. The `ALL` array lengths are written out (`15`, `16`). `inspect_typed_enums` repeats every typed enum's wire spellings by hand; a new enum value must be added there as well as on the enum.
3. Tests that enumerate kinds: `crates/psyche-core/src/contracts/mod.rs` :: `record_kind_all_has_exactly_fifteen_entries`, `schema_version_accepts_exactly_the_sixteen_known_strings`; `crates/psyche-core/tests/decode.rs` :: `recognized_registry_entries_never_report_unknown_or_unsupported`. `crates/psyche-core/src/id.rs` :: `record_id_all_fifteen_prefixes_round_trip_through_parse_any` iterates `RecordKind::ALL` and needs no edit.
4. Store kind tables, written by hand because `SchemaKind::name` and `SchemaKind::ALL` are private: `crates/psyche-store/src/records.rs` :: `kind_key`, `parse_kind_key`, `schema_kind_for_id`. `parse_kind_key` matches strings with a catch-all arm, so a forgotten kind still compiles; its only callers are the `validate_all` passes run by `Store::prune`, which then report `DatabaseCorruption`.
5. Evidence checker constants: `scripts/check-g2-evidence-v1.py` :: `validate_record_kinds` (exactly 15 variants, `[RecordKind; 15]`, prefixes of three letters and `_`), `validate_sources` (`[SchemaKind; 16]`, `[Self; 36]`), `validate_docs` (the sixteen schema strings in `docs/SCHEMAS.md`). Its mutation test: `scripts/check-g2-evidence-test.py` :: `test_record_kind_identity_mutations_are_rejected`.
6. Documents: the registry list and the "exactly fifteen" sentence in `docs/SCHEMAS.md`, and the table above.
7. Proof for the new type: a fixture in `crates/psyche-core/tests/fixtures/` with positive and negative cases in `crates/psyche-core/tests/contracts.rs`, and a store round trip in `crates/psyche-store/tests/records.rs`.

**SQL migration.** Not needed for a kind stored as an immutable canonical document: `canonical_records.kind` is unconstrained `TEXT`, and `Store::open` compares table definitions, not row kinds. Needed when the kind gets its own table or column, as `ExecutionBinding` did. Then also edit `crates/psyche-store/src/migrations.rs` :: `CURRENT_DATABASE_VERSION`, `FOUNDATION_TABLES`, `apply_migration_sql`, `foundation_indexes`, `validate_current_schema`; add a file in `crates/psyche-store/migrations/`; update the schema copies `crates/psyche-store/tests/fixtures/v1.sql` and `crates/psyche-store/tests/support/mod.rs` :: `FOUNDATION_SCHEMA`, `FOUNDATION_TABLES`; and extend `crates/psyche-store/tests/migrations.rs`. The per-kind routing for `ExecutionBinding` is in `crates/psyche-store/src/records.rs` :: `insert_with_status`, `Store::load_canonical_bytes`, `Store::count_records`.

**Quarantine.** No change. Bytes naming a kind this build does not know decode to `ContractError::UnknownSchema`. `Store::ingest` turns exactly three decode errors into a `quarantine_records` row and `IngestOutcome::Quarantined`: `UnknownSchema`, `UnsupportedMajor` and `UnknownEnumValue`. Every other decode error returns `StoreError::Contract` and writes nothing. Rows quarantined by an older build are not re-ingested after an upgrade; they stay until `Store::resolve_quarantine` records `QuarantineResolutionCode::SchemaNowSupported`.

**Error codes.** A new kind needs no new `ErrorCode`. The 36 codes are the closed wire vocabulary of `psyche.error.v1`; nothing maps a kind to a code, and no production crate outside `crates/psyche-core/src/contracts/` uses the type. Kind failures use `ContractError` and `StoreError` variants that carry a `SchemaKind`. Adding a code is a separate edit set: `crates/psyche-core/src/contracts/error.rs` :: `ErrorCode`, `ErrorCode::ALL`, `ErrorCode::as_str`; the spelling list in `inspect_typed_enums`; `crates/psyche-core/tests/fixtures/error-codes-v1.json`; and `validate_sources`.

## Digests and goldens

Primitive: `crates/psyche-core/src/digest.rs` :: `canonical_bytes`, `digest`, `Sha256Digest`, `Sha256Digest::parse`, `Sha256Digest::from_raw_bytes`. `canonical_bytes` is RFC 8785 through `serde_json_canonicalizer`; `digest` is `sha256:` plus lowercase hex over `canonical_bytes`; `from_raw_bytes` hashes raw bytes and is private to core. "Tracked" means the test is named in `scripts/g2-test-manifest.json`.

**Execution request digest** (`AdoptionRequest.request_digest`, copied into `ExecutionBinding.request_digest` and `ExecutionCorrelation.request_digest`)
- Constructed: `crates/psyche-coven/src/port.rs` :: `AdoptionRequest::new`, `AdoptionRequest::recompute_digest`, `AdoptionRequest::validate_digest`. Input: the complete `ExecutionRequestInput`, through `digest`.
- Goldens: `crates/psyche-coven/tests/fixtures/execution-request-launch.json`, `crates/psyche-coven/tests/fixtures/execution-request-input.json`. The evidence checker re-hashes both files against values written in `scripts/check-g2-evidence-v1.py` :: `validate_sources`, `validate_golden`. The launch golden is also packed into the receipt replay archive, so changing its bytes changes `profiles/receipt-replay/pack.py` :: `GOLDEN_DIGEST`, the literal in `profiles/receipt-replay/consumer/qualify.py` and the pin in `profiles/receipt-replay/archive.sha256`.
- Tests: `crates/psyche-coven/tests/request_digest.rs` :: `execution_request_launch_matches_golden_bytes_and_digest`, `execution_request_input_matches_golden_bytes_and_digest`, `execution_request_artifact_order_is_digest_bound`; `crates/psyche-test-support/tests/state_machine.rs` :: `request_digest_binds_every_typed_field`; `crates/psyche-test-support/tests/fakes.rs` :: `changed_request_with_retained_digest_fails_before_adoption`, `input_request_digest_binds_every_artifact_field_order_and_content`.
- Tracked: `scripts/g2-test-manifest.json` :: `execution_request_launch_matches_golden_bytes_and_digest`, `execution_request_input_matches_golden_bytes_and_digest`, `request_digest_binds_every_typed_field`, `c_s4_stable_adoption`. Not tracked: the artifact-order test and both `fakes.rs` tests.

**Effect digest** (`SurfaceEffect.effect_digest`, `Delivery.effect_digest`)
- Recomputed on every decode and validate: `crates/psyche-core/src/contracts/surface.rs` :: `SurfaceEffect::validate`, `Delivery::validate`. No constructor; a producer calls `digest` on `effect`.
- Known answers: the literal `effect_digest` in `crates/psyche-core/tests/fixtures/surface-effect.json` and `crates/psyche-core/tests/fixtures/delivery-ready.json`. Decoding fails unless it equals the recomputed value.
- Tests: `crates/psyche-core/tests/contracts.rs` :: `delivery_v1_fixture_round_trips_canonically`, `surface_event_and_effect_fixtures_round_trip`, `surface_values_reject_scalars_oversize_unknown_fields_wrong_ids_and_bad_digest`, `arbitrary_precision_effect_digests_are_stable_across_decode`.
- Tracked: `scripts/g2-test-manifest.json` :: `delivery_v1_fixture_round_trips_canonically`, `surface_event_and_effect_fixtures_round_trip`. Not tracked: the other two.

**Record digest** (`canonical_records.digest`)
- Constructed and rechecked on every read: `crates/psyche-store/src/records.rs` :: `insert_canonical_in_transaction`, `validate_stored_canonical_record`. Input: the whole `CanonicalDocument`, through `digest`.
- No known-answer vector. Tests: `crates/psyche-store/tests/records.rs` :: `delivery_direct_insert_round_trips_canonically`, `same_id_same_digest_is_idempotent_but_changed_payload_conflicts`, `canonical_record_detects_digest_only_bytes_noncanonical_and_malformed_tamper`.
- Tracked: `scripts/g2-test-manifest.json` :: `delivery_direct_insert_round_trips_canonically`. Not tracked: the other two.

**Binding revision digest** (`execution_binding_revisions.digest`, `ExecutionBinding.previous_revision_digest`)
- Constructed: `crates/psyche-store/src/execution_bindings.rs` :: `insert_in_transaction`, `validate_revision_chain`. The link to the next revision is built in `crates/psyche-coven/src/port.rs` :: `derive_termination_outcome_revision`.
- No known-answer vector. Tests: `crates/psyche-store/tests/records.rs` :: `execution_binding_revision_rejects_forks_gaps_and_changed_correlation`, `execution_binding_revision_rejects_same_revision_changed_bytes`, `execution_revision_chain_detects_blob_digest_link_schema_timestamp_and_gap_tamper`.
- Tracked: `scripts/g2-test-manifest.json` :: `execution_binding_revision_rejects_forks_gaps_and_changed_correlation`, `execution_binding_revision_rejects_same_revision_changed_bytes`. Not tracked: the tamper test.

**Transition digest** (`Transition.transition_digest`)
- Constructed: `crates/psyche-store/src/transitions.rs` :: `Transition::new`, `Transition::validate`, `TransitionDigestInput`.
- No known-answer vector; the digest test recomputes with a mirror struct. Tests: `crates/psyche-store/tests/records.rs` :: `transition_digest_uses_the_exact_owned_canonical_contract`, `transition_validation_rejects_wrong_id_kind_and_digest_without_writing`, `transition_append_rejects_prior_digest_corruption_without_writing`. None is tracked.

**Content reference digest** (`ContentAddressedReference.digest`, for result and artifact bytes)
- Constructed: `crates/psyche-coven/src/port.rs` :: `ContentAddressedReference::for_bytes`, `ContentAddressedReference::validate_payload`, `raw_digest`. SHA-256 over the raw payload, computed with `sha2` in the adapter crate, not with `canonical_bytes`.
- Golden: `crates/psyche-coven/tests/fixtures/result-bundle.json`. Its content digests are placeholders, so it pins bytes and shape, not a hash. No known-answer vector for `raw_digest`.
- Tests: `crates/psyche-coven/tests/bindings.rs` :: `content_reference_rejects_digest_size_media_type_and_lifetime_mismatch`, `result_bundle_fixture_round_trips_complete_content_references`, `result_bundle_fixture_uses_launch_request_correlation`. All three are tracked: `scripts/g2-test-manifest.json` :: `content_reference_rejects_digest_size_media_type_and_lifetime_mismatch`, `result_bundle_fixture_round_trips_complete_content_references`, `result_bundle_fixture_uses_launch_request_correlation`.

**Rejected-document and quarantine digests**
- Raw payload digest and private attestation: `crates/psyche-core/src/contracts/mod.rs` :: `RejectedDocument::from_bytes`, `RejectedDocument::retained_payload_digest`, `RejectedDocument::is_authentic`, `rejected_document_attestation`. Known answer for the raw digest only (SHA-256 of `abc`): `crates/psyche-core/tests/decode.rs` :: `rejected_document_hashes_full_raw_bytes_and_bounds_retained_payload`. Not tracked.
- Stored integrity and resolution digests: `crates/psyche-store/src/quarantine.rs` :: `compute_quarantine_integrity_digest`, `compute_resolution_digest`, `QuarantineIntegrityInput`, `ResolutionDigestInput`. No known-answer vector. Tests: `crates/psyche-store/tests/retention.rs` :: `quarantine_resolution_is_durable_and_idempotent`, `short_quarantine_integrity_tampering_fails_closed`, `corrupted_resolution_columns_or_metadata_fail_closed`. Tracked: `scripts/g2-test-manifest.json` :: `quarantine_resolution_is_durable_and_idempotent`.

**Format-checked only.** These fields accept any well-formed digest; no production code constructs or recomputes them: `Intent.digest`, the five `IdentitySnapshot` digests, `SurfaceEvent.adapter_event_digest`, `Delegation.scope_digest`, `Delegation.evidence_scope_digest`, the four `Addon` digests, `Evidence.content_digest`, `Verdict.sealed_evidence_digest`, `DeliverySurfaceDecision.request_digest`, `ReconciliationRequest.ambiguity_digest`, `CovenEvent.event_digest`, `ExecutionArtifactBinding.digest`, the component digests inside `ExecutionRequestInput` (`context_manifest_digest`, `delegation_digest`, `budget_digest`, `payload_digest`, `input_digest`), and `CancellationAcknowledgementEvidence.authority_evidence_digest` (which must also be non-zero). `ReconciliationDisposition::validate_for` only checks that the reply echoes the request's `ambiguity_digest`.

## Lifecycle semantics, code and proof

**Adoption**
- Code: `crates/psyche-coven/src/port.rs` :: `AdoptionRequest`, `ExecutionRequestInput`, `ExecutionCorrelation`, `AdoptionDisposition`, `CovenPort::adopt`, `CovenPort::lookup`; `crates/psyche-core/src/contracts/execution.rs` :: `AdoptionState`, `ExecutionBinding::request_digest`, `ExecutionBinding::coven_session_id`.
- Store: `execution_binding_revisions`. Request fields are frozen and the session is append-only: `crates/psyche-store/src/execution_bindings.rs` :: `frozen_execution_fields_match`, `session_binding_is_append_only`. No rule constrains `adoption_state` between revisions. The adoption disposition itself is durable in Coven, which the tests model with `crates/psyche-test-support/src/coven.rs` :: `FakeCoven`.
- Positive: `crates/psyche-test-support/tests/fakes.rs` :: `stable_adoption_replay_survives_fake_restart_and_rejects_changed_intent`; `crates/psyche-test-support/tests/lookup.rs` :: `lookup_replays_durable_adoption_after_restart_without_a_script_step`.
- Negative: `crates/psyche-test-support/tests/fakes.rs` :: `changed_request_with_retained_digest_fails_before_adoption`, `advertised_adoption_requires_a_scripted_adoption_step`; `crates/psyche-store/tests/records.rs` :: `execution_binding_revision_rejects_every_frozen_execution_field_change`.

**Dispatch and fencing**
- Code: `crates/psyche-coven/src/port.rs` :: `ReconciliationRequest`, `ReconciliationDisposition`, `ReconciliationDisposition::validate_for`, `CovenPort::reconcile`; `crates/psyche-core/src/contracts/execution.rs` :: `AdoptionState::Fenced`; `crates/psyche-core/src/contracts/foundation.rs` :: `Recovery::fence_token`.
- Store: no fence or dispatch table. `Recovery` is a row in `canonical_records` whose fields, `fence_token` included, are only bounds-checked. The only persist-before-dispatch coordinator in production code is `persist_then_terminate` (cancellation, below). There is no production redispatch decision: `crates/psyche-test-support/src/suites/coven.rs` :: `redispatch_decision`, `reconciliation_redispatch_decision` are test models, and eligibility is a fixture observation, `crates/psyche-test-support/src/coven.rs` :: `RedispatchEligibility`, `CovenConformanceFixture::redispatch_eligibility`.
- Positive: `crates/psyche-test-support/tests/fakes.rs` :: `fenced_reconciliation_survives_after_commit_disconnect_and_restart`, `reconcile_after_commit_replays_and_changed_correlation_conflicts`.
- Negative: `crates/psyche-test-support/tests/state_machine.rs` :: `c_s6_model_never_redispatches_without_fence`, `c_s6_fixture_reports_fence_eligibility_without_redispatch`; `crates/psyche-test-support/tests/fakes.rs` :: `unresolved_reconciliation_remains_retryable_until_returned_or_fenced`, `reconciliation_resolution_requires_exact_correlation_digest_utc_and_lower_bound`.

**Cancellation**
- Vocabulary and pairing rules: `crates/psyche-core/src/contracts/execution.rs` :: `CancellationState`, `CancellationAcknowledgementKind`, `CancellationAcknowledgementEvidence`, `CancellationUnresolvedEvidence`, `TerminationRequestCorrelation`, `ExecutionBinding::validate`, `validate_cancellation`, `validate_ack`, `validate_evidence_bindings`.
- Acknowledged: `CancellationState::AcknowledgedTerminated` and `CancellationState::AcknowledgedAlreadyTerminal` require `ExecutionBinding::cancellation_acknowledgement` of the matching kind and forbid unresolved evidence. Unresolved: `CancellationState::TerminationUnknown` requires `ExecutionBinding::cancellation_unresolved` and forbids an acknowledgement.
- Dispatch: `crates/psyche-coven/src/port.rs` :: `TerminationDisposition`, `TerminationRequest`, `TerminationPersistence`, `persist_then_terminate`, `derive_termination_outcome_revision`, `CovenPort::terminate`.
- Store: both evidence structs live inside the binding JSON in `execution_binding_revisions.canonical_json`; there are no evidence columns. Legal moves: `crates/psyche-store/src/execution_bindings.rs` :: `cancellation_binding_is_append_only`, `termination_binding_is_append_only`.
- Positive: `crates/psyche-test-support/tests/fakes.rs` :: `termination_dispatch_persists_acknowledged_outcome_before_success`, `termination_dispatch_persists_unresolved_outcome_before_success`, `termination_dispatch_restart_recovers_missing_outcome`; `crates/psyche-store/tests/records.rs` :: `execution_binding_revision_appends_termination_outcomes_without_record_conflict`, `execution_cancellation_legal_forward_paths_and_stable_terminal_revisions_append`.
- Negative: `crates/psyche-core/tests/contracts.rs` :: `cancellation_state_vocabulary_requires_matching_o5_evidence`, `cancellation_binding_normalizes_every_correlation_and_evidence_failure`, `cancellation_binding_decode_maps_nested_wire_failures_to_evidence_mismatch`; `crates/psyche-store/tests/records.rs` :: `direct_insert_rejects_acknowledged_cancellation_without_evidence`, `direct_insert_rejects_mismatched_cancellation_evidence`, `execution_cancellation_rejects_unknown_to_acknowledged`, `execution_cancellation_rejects_terminal_switches_and_evidence_mutation`; `crates/psyche-coven/tests/bindings.rs` :: `termination_dispatch_rejects_invalid_request_before_persistence`.
- Raw-status rejection: `crates/psyche-test-support/tests/fakes.rs` :: `raw_session_statuses_never_become_termination_acknowledgement`, `killed_then_orphaned_statuses_do_not_create_termination_evidence`. The first covers all seven ledger statuses and the second covers `killed` followed by `orphaned`; both run against a real store and neither is tracked. `crates/psyche-test-support/src/suites/coven.rs` :: `RAW_LEDGER_STATES`, `assert_c_s9_cancellation_acknowledgement` loops all seven against an in-memory fixture and is tracked as `c_s9_cancellation_acknowledgement`. The tracked core test `cancellation_state_vocabulary_requires_matching_o5_evidence` uses the string `terminated`, which is not one of the seven.

**Results and receipts**
- No Rust type, table or crate module is named receipt, and nothing under `crates/` reads one. The only receipt code is the experimental offline profile described in `profiles/receipt-replay/README.md`: `profiles/receipt-replay/run.py` :: `VERSION`, `compare`, `observation` compares two consumer-supplied observations for the same request digest, identical receipt bytes and an execution count of one. Its version string `psyche.experimental.receipt-replay.v1` is not a `SchemaKind`. It is qualified by `profiles/receipt-replay/qualify.sh` in `.github/workflows/receipt-replay.yml`.
- Results: `crates/psyche-coven/src/port.rs` :: `ResultBundle`, `ArtifactReference`, `ContentAddressedReference`, `ResultBundle::validate`, `CovenPort::result`.
- Store: none. `ResultBundle` is not a record kind and no table holds result metadata or content bytes.
- Positive: `crates/psyche-coven/tests/bindings.rs` :: `result_bundle_fixture_round_trips_complete_content_references`, `result_bundle_accepts_unique_artifacts_in_wire_order`; `crates/psyche-test-support/tests/fakes.rs` :: `result_references_survive_after_commit_disconnect_and_restart`.
- Negative: `crates/psyche-coven/tests/bindings.rs` :: `content_reference_rejects_digest_size_media_type_and_lifetime_mismatch`; `crates/psyche-test-support/tests/fakes.rs` :: `result_references_must_echo_the_adopted_correlation`, `result_references_reject_an_unadopted_session`, `invalid_scripted_results_are_response_errors`.

**Transition history**
- Code and store: `crates/psyche-store/src/transitions.rs` :: `Transition`, `Store::append_transition`, `Store::transitions`, `append_in_transaction`, `authenticate_stored_history`; table `transitions`.
- Positive: `crates/psyche-store/tests/records.rs` :: `transition_versions_are_monotonic_and_append_only`, `transition_exact_replay_is_idempotent_and_divergent_identity_conflicts`.
- Negative: `crates/psyche-store/tests/records.rs` :: `transition_append_requires_exact_version_and_prior_state`, `concurrent_transition_forks_have_one_durable_winner`, `transition_history_rejects_nonpositive_or_nonmonotonic_sequence`.

**Quarantine**
- Code: `crates/psyche-core/src/contracts/mod.rs` :: `RejectedDocument`, `RejectionReason`, `RejectedDocument::from_decode_error`; `crates/psyche-store/src/quarantine.rs` :: `QuarantineReasonCode`, `QuarantineResolution`, `QuarantineResolutionCode`, `ResolveQuarantineOutcome`, `Store::quarantine`, `Store::quarantine_record`, `Store::resolve_quarantine`; entry point `crates/psyche-store/src/records.rs` :: `Store::ingest`, `IngestOutcome`.
- Store: `quarantine_records`, and one `audit_events` row per resolution.
- Positive: `crates/psyche-store/tests/retention.rs` :: `unknown_major_is_quarantined_without_dispatchable_record`, `unknown_enum_is_quarantined_without_dispatchable_record`, `quarantine_resolution_is_durable_and_idempotent`, `concurrent_quarantine_resolution_has_one_durable_winner`; `crates/psyche-test-support/tests/state_machine.rs` :: `unknown_enum_operations_never_create_dispatchable_records`.
- Negative: `crates/psyche-store/tests/retention.rs` :: `quarantine_resolution_rejects_unknown_stale_or_conflicting_requests`, `quarantine_rejects_forged_exactly_64_kib_digest`, `short_quarantine_integrity_tampering_fails_closed`; `crates/psyche-store/tests/records.rs` :: `ingest_rejects_non_persistable_error_envelope`.
- Not established: no test passes an unknown kind through `Store::ingest`. Every ingest quarantine test sends an unknown major or an unknown enum value. The unknown-kind decode error is proven in `crates/psyche-core/tests/contracts.rs` :: `strict_probe_and_document_limit_fail_closed`, and the `UnknownSchema` reason is stored only through direct `Store::quarantine` calls.

**Retention**
- Code: `crates/psyche-store/src/retention.rs` :: `Store::prune`, `PruneReport`, `Store::checkpoint`. `prune` validates every table, then deletes only resolved `quarantine_records` rows strictly older than the cutoff. Nothing deletes from `canonical_records`.
- Positive: `crates/psyche-store/tests/retention.rs` :: `prune_uses_strictly_older_cutoff_and_never_deletes_unresolved_rows`, `pruning_preserves_unresolved_quarantine_binding_revisions_and_transitions`, `checkpoint_preserves_committed_state`.
- Negative: `crates/psyche-store/tests/retention.rs` :: `malformed_persisted_quarantine_id_fails_prune_before_deleting_valid_rows`, `malformed_canonical_created_at_fails_prune_before_deleting_eligible_quarantine`.

**Migrations**
- Code: `crates/psyche-store/src/lib.rs` :: `Store::open`; `crates/psyche-store/src/migrations.rs` :: `CURRENT_DATABASE_VERSION`, `apply_migration_sql`, `validate_current_schema`; SQL in `crates/psyche-store/migrations/001_foundation.sql`. State: table `schema_migrations` and SQLite `user_version`.
- Positive: `crates/psyche-store/tests/migrations.rs` :: `fresh_store_applies_v1_once_and_reopens`, `version_zero_fixture_migrates_without_losing_existing_data`, `existing_v1_fixture_opens_without_reapplying_migration`, `concurrent_first_open_applies_migration_once`.
- Negative: `crates/psyche-store/tests/migrations.rs` :: `future_database_version_fails_before_any_migration`, `altered_foundation_columns_indexes_and_constraints_are_rejected_as_corruption`, `production_migration_failure_rolls_back_and_recovers`, `partially_applied_v1_transaction_rolls_back_and_recovers`.

**Crash and restart**
- Store process kill: `crates/psyche-store/tests/crash.rs` :: `killed_writer_exposes_only_committed_state_after_reopen`, `killed_inside_migration_transaction_rolls_back_before_reopen`, `crash_writer_rejects_every_unknown_fault_point`, `CRASH_MODES`. Writer: `crates/psyche-store/src/bin/crash_writer.rs` :: `main`. Fault points, behind the `test-fault-injection` feature: `crates/psyche-store/src/migration_test_support.rs` :: `StoreFaultPoint`, `MigrationFaultPoint`, `run_store_with_fault`, `run_migration_with_fault`.
- Adapter faults and restart: `crates/psyche-test-support/src/coven.rs` :: `CovenFaultPoint`, `CovenConformanceFixture::restart`, `CovenConformanceFixture::select_fault`; `crates/psyche-test-support/src/suites/coven.rs` :: `MANDATORY_FAULTS`, `assert_c_s11_restart_persistence`.
- Model against store: `crates/psyche-test-support/tests/state_machine.rs` :: `model_and_store_agree_after_any_foundation_operation_sequence`.
- Runtime shutdown and checkpoint: `crates/psyche-runtime/src/lib.rs` :: `tests::checkpoint_failure_stops_and_releases_every_shutdown_waiter`; `crates/psyche-runtime/tests/lifecycle.rs` :: `start_opens_the_configured_store_and_shutdown_leaves_schema_v1_reopenable`, `future_database_version_fails_start_before_running_is_published`.

### Conformance rows

Each row of the matrix in `docs/TESTING.md`. Assert functions are in `crates/psyche-test-support/src/suites/coven.rs`; tests are in `crates/psyche-test-support/tests/conformance.rs`. All twelve tests are tracked. Every row runs against `ScriptedG2Fixture`, an in-memory fixture, not against the store.

| Row | Assert function | Conformance test |
|---|---|---|
| C-S1 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s1_contract_negotiation` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s1_contract_negotiation` |
| C-S2 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s2_session_lifecycle` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s2_session_lifecycle` |
| C-S3 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s3_snapshot_attempt_binding` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s3_snapshot_attempt_binding` |
| C-S4 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s4_stable_adoption` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s4_stable_adoption` |
| C-S5 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s5_non_adoption_proof` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s5_non_adoption_proof` |
| C-S6 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s6_ambiguity_fence` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s6_ambiguity_fence` |
| C-S7 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s7_ordered_cursor` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s7_ordered_cursor` |
| C-S8 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s8_terminal_authority` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s8_terminal_authority` |
| C-S9 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s9_cancellation_acknowledgement` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s9_cancellation_acknowledgement` |
| C-S10 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s10_result_artifact_binding` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s10_result_artifact_binding` |
| C-S11 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s11_restart_persistence` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s11_restart_persistence` |
| C-S12 | `crates/psyche-test-support/src/suites/coven.rs` :: `assert_c_s12_structured_denial` | `crates/psyche-test-support/tests/conformance.rs` :: `c_s12_structured_denial` |

Fixture controls: `crates/psyche-test-support/src/coven.rs` :: `CovenConformanceFixture`, `CovenConformanceCase`, `FixtureAvailability`; `crates/psyche-test-support/src/suites/coven.rs` :: `ScriptedG2Fixture`, `scripted_fixture`, `unsupported_fixture`. Surface delivery: `crates/psyche-test-support/src/suites/surface.rs` :: `assert_surface_unknown_delivery`.

## Where things are not

- `crates/psyche-core/src/schema.rs` :: `CONFIG_SCHEMA_VERSION`, `ensure_schema_version` gates only the configuration file's `schema_version`. The record and document registry is `SchemaKind` in `crates/psyche-core/src/contracts/mod.rs`.
- Core owns the digest primitive, not every digest. The request digest and the content digest are constructed in `crates/psyche-coven/src/port.rs`; the record, revision, transition and quarantine digests in `crates/psyche-store/src/`.
- `crates/psyche-coven/src/port.rs` :: `raw_digest` is a second SHA-256 path outside core. `profiles/receipt-replay/run.py` :: `digest`, `compare` is a third: it recomputes the request digest in Python from the request bytes.
- The only store-backed `TerminationPersistence` is `crates/psyche-test-support/src/coven.rs` :: `StoreTerminationPersistence`. No production crate implements `TerminationPersistence`, `CovenPort` or `SurfacePort`; the runtime and CLI call only `Store::open`, `Store::checkpoint` and `prepare_data_dir`.
- Two unrelated enums are both named `PortError`: `crates/psyche-coven/src/error.rs` :: `PortError` and `crates/psyche-surfaces/src/error.rs` :: `PortError`.
- `ErrorCode` in `crates/psyche-core/src/contracts/error.rs` is the wire vocabulary. The `ErrorCode` imported in `crates/psyche-store/src/connection.rs` is SQLite's.
- `crates/psyche-store/src/migration_test_support.rs` is production-crate source, compiled only with the `test-fault-injection` feature. It is not a migration.
- `Delivery` is the stored record. `crates/psyche-surfaces/src/port.rs` :: `DeliveryDisposition` is an adapter reply and is not stored.
- `agent/manifest.yaml` and `scripts/g2-attestation-scope.json` hold no kind, digest or test inventory. The best test-name index is `scripts/g2-test-manifest.json`, which lists tracked tests only.

## How to trace a sibling

`Addon` has no references beyond its enumeration sites, so its hits are the list of places a kind must appear:

```sh
grep -rnw Addon crates scripts docs    # type, both enums, every match arm, the store tables
grep -rnw addon crates scripts docs    # schema strings in tests, the checker and the documents
grep -rn  adn_  crates scripts docs    # the prefix arm
```

## Known stale statements elsewhere

Each of these contradicts the code cited above. They are listed, not fixed, because each file needs its owner's review.

- `crates/psyche-core/src/contracts/mod.rs`, module comment: says the module has "no `CanonicalDocument`" and that `QuarantineId` lands in a later task. `CanonicalDocument` is defined in that file, and `QuarantineId` exists in `crates/psyche-store/src/quarantine.rs`.
- `docs/TESTING.md`, C-S12 entry: "unknown enums/kinds/majors quarantine rather than dispatch". `assert_c_s12_structured_denial` never touches the decoder, the store or quarantine. The proof is in the Quarantine entry above.
- `docs/TESTING.md`, raw-status paragraph: true, but of the tests that cover all seven statuses only `c_s9_cancellation_acknowledgement` is tracked.
- `docs/SCHEMAS.md`: "every digest is SHA-256 over complete canonical typed content". The rejected-document and content-reference digests are over raw bytes, and many digest fields are only format-checked.
- `docs/SCHEMAS.md`, retention paragraph: "Content referenced by an unresolved or live record remains retained". No table stores result or artifact content, and `Store::prune` touches only `quarantine_records`.
- `crates/psyche-test-support/tests/state_machine.rs` :: `unknown_schema_operations_never_create_dispatchable_records` sends an unknown major (`psyche.intent.v2`), not an unknown schema.
- `AGENTS.md` and `README.md` ownership tables list "digests" under core only; see "Where things are not".
