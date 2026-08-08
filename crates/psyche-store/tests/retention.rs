#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use psyche_core::contracts::execution::{AdoptionState, CancellationState, ExecutionBinding};
use psyche_core::contracts::{
    CanonicalDocument, Intent, RecordKind, RejectedDocument, RejectionReason, SchemaKind,
    SchemaVersion,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use psyche_core::id::{RecordId, RequestId};
use psyche_store::{
    IngestOutcome, QuarantineId, QuarantineReasonCode, QuarantineResolution,
    QuarantineResolutionCode, ResolveQuarantineOutcome, Store, StoreError, Transition,
};
use rusqlite::{Connection, params};
use serde_json::{Map, json};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, UtcOffset};

fn test_store() -> (Store, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("psyche.sqlite3");
    let store = Store::open(&path).unwrap();
    (store, dir, path)
}

fn raw_connection(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn fixture_digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn record_id(kind: RecordKind, suffix: &str) -> RecordId {
    RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())).unwrap()
}

fn fixture_attempt_id() -> RecordId {
    record_id(RecordKind::Attempt, "01J00000000000000000000000")
}

fn fixture_snapshot_id() -> RecordId {
    record_id(RecordKind::IdentitySnapshot, "01J00000000000000000000001")
}

fn fixture_binding() -> ExecutionBinding {
    ExecutionBinding {
        schema_version: SchemaVersion::parse("psyche.execution_binding.v1").unwrap(),
        attempt_id: fixture_attempt_id(),
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: at("2026-08-05T12:00:00Z"),
        familiar_snapshot_id: fixture_snapshot_id(),
        project_id: "project-a".to_owned(),
        request_id: RequestId::parse("req_01J00000000000000000000002").unwrap(),
        request_digest: fixture_digest('a'),
        request_created_at: at("2026-08-05T11:59:00Z"),
        request_valid_until: at("2026-08-05T12:05:00Z"),
        coven_contract_version: "coven.v1".to_owned(),
        coven_session_id: None,
        adoption_state: AdoptionState::Adopted,
        event_cursor: Some("cursor:1".to_owned()),
        cancellation_state: CancellationState::NotRequested,
        termination_request: None,
        termination_reason_code: None,
        cancellation_acknowledgement: None,
        cancellation_unresolved: None,
        terminal_state: None,
    }
}

fn fixture_intent() -> Intent {
    Intent {
        schema_version: SchemaVersion::parse("psyche.intent.v1").unwrap(),
        intent_id: record_id(RecordKind::Intent, "01J00000000000000000000003"),
        principal_id: "principal-a".to_owned(),
        familiar_snapshot_id: fixture_snapshot_id(),
        project_id: "project-a".to_owned(),
        requested_outcome: "retain integrity".to_owned(),
        constraints: Map::new(),
        required_evidence: vec!["review".to_owned()],
        surface_event_id: None,
        created_at: at("2026-08-05T12:00:00Z"),
        digest: fixture_digest('c'),
    }
}

fn fixture_transition() -> Transition {
    Transition::new(
        SchemaKind::ExecutionBinding,
        fixture_attempt_id(),
        1,
        None,
        "admitted".to_owned(),
        at("2026-08-05T12:00:01Z"),
    )
    .unwrap()
}

fn quarantined_fixture(store: &mut Store) -> QuarantineId {
    let IngestOutcome::Quarantined { quarantine_id } = store
        .ingest(br#"{"schema_version":"psyche.intent.v2"}"#)
        .unwrap()
    else {
        panic!("fixture was not quarantined");
    };
    quarantine_id
}

fn resolution(code: QuarantineResolutionCode, resolved_at: OffsetDateTime) -> QuarantineResolution {
    QuarantineResolution { code, resolved_at }
}

fn assert_database_corruption<T: std::fmt::Debug>(result: Result<T, StoreError>) {
    let error = result.unwrap_err();
    assert!(matches!(error, StoreError::DatabaseCorruption));
    assert_eq!(
        error.to_string(),
        "stored database content failed integrity validation"
    );
    assert_eq!(
        format!("{error:?}"),
        "StoreError(stored database content failed integrity validation)"
    );
    assert!(error.source().is_none());
}

#[test]
fn quarantine_id_constructor_parser_and_serde_round_trip() {
    let generated = QuarantineId::new();
    assert!(generated.as_str().starts_with("qua_"));
    assert_eq!(generated.as_str().len(), 30);
    assert_eq!(QuarantineId::parse(generated.as_str()).unwrap(), generated);

    let encoded = serde_json::to_string(&generated).unwrap();
    assert_eq!(
        serde_json::from_str::<QuarantineId>(&encoded).unwrap(),
        generated
    );

    for malformed in [
        "01J00000000000000000000000",
        "qua_01J0000000000000000000000",
        "qua_01J000000000000000000000000",
        "qua_01j00000000000000000000000",
        "qua_81J00000000000000000000000",
        "qua_01I00000000000000000000000",
    ] {
        assert!(QuarantineId::parse(malformed).is_err(), "{malformed}");
        let json = serde_json::to_string(malformed).unwrap();
        assert!(serde_json::from_str::<QuarantineId>(&json).is_err());
    }
}

#[test]
fn unknown_major_is_quarantined_without_dispatchable_record() {
    let (mut store, _dir, _path) = test_store();
    let outcome = store
        .ingest(br#"{"schema_version":"psyche.intent.v2"}"#)
        .unwrap();
    assert!(matches!(outcome, IngestOutcome::Quarantined { .. }));
    assert_eq!(store.count_records(SchemaKind::Intent).unwrap(), 0);
}

#[test]
fn unknown_enum_is_quarantined_without_dispatchable_record() {
    let (mut store, _dir, _path) = test_store();
    let bytes = serde_json::to_vec(&json!({
        "schema_version": "psyche.graph.v1",
        "graph_id": "grf_01J00000000000000000000003",
        "root_intent_id": "int_01J00000000000000000000004",
        "owner_principal_id": "principal:one",
        "policy_revision": "policy:one",
        "state": "future_state",
        "version": 1
    }))
    .unwrap();
    let IngestOutcome::Quarantined { quarantine_id } = store.ingest(&bytes).unwrap() else {
        panic!("unknown enum was not quarantined");
    };
    let rejected = store.quarantine_record(&quarantine_id).unwrap().unwrap();
    assert_eq!(rejected.reason, QuarantineReasonCode::UnknownEnumValue);
    assert_eq!(rejected.bounded_payload, bytes);
    assert_eq!(store.count_records(SchemaKind::Graph).unwrap(), 0);
}

#[test]
fn quarantine_is_bounded_idempotent_and_reason_sensitive() {
    let (mut store, _dir, _path) = test_store();
    let mut bytes = b"payload-secret-marker".repeat(4_000);
    bytes.extend_from_slice(b"tail-not-retained");
    let rejected = RejectedDocument::from_bytes(&bytes, RejectionReason::TooLarge);
    let expected_digest = rejected.payload_digest.clone();
    let id = store.quarantine(rejected.clone()).unwrap();
    assert_eq!(store.quarantine(rejected).unwrap(), id);

    let persisted = store.quarantine_record(&id).unwrap().unwrap();
    assert_eq!(persisted.payload_digest, expected_digest);
    assert_eq!(persisted.bounded_payload.len(), 64 * 1024);
    assert_eq!(persisted.reason, QuarantineReasonCode::TooLarge);
    assert!(!format!("{persisted:?}").contains("payload-secret-marker"));

    let same_payload_different_reason =
        RejectedDocument::from_bytes(&bytes, RejectionReason::UnknownSchema);
    let other_id = store.quarantine(same_payload_different_reason).unwrap();
    assert_ne!(other_id, id);
}

#[test]
fn quarantine_resolution_is_durable_and_idempotent() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
    let resolution = resolution(
        QuarantineResolutionCode::ConfirmedInvalid,
        discovered_at + Duration::seconds(1),
    );
    let first = store.resolve_quarantine(&id, &resolution).unwrap();
    let ResolveQuarantineOutcome::Resolved { resolution_digest } = first else {
        panic!("first resolution did not resolve");
    };
    assert!(matches!(
        store.resolve_quarantine(&id, &resolution).unwrap(),
        ResolveQuarantineOutcome::AlreadyResolved {
            resolution_digest: repeated
        } if repeated == resolution_digest
    ));
    drop(store);

    let reopened = Store::open(&path).unwrap();
    let persisted = reopened.quarantine_record(&id).unwrap().unwrap();
    assert_eq!(
        persisted.resolution_code,
        Some(QuarantineResolutionCode::ConfirmedInvalid)
    );
    assert_eq!(persisted.resolution_digest, Some(resolution_digest));
    assert_eq!(reopened.audit_events().unwrap().len(), 1);
}

#[test]
fn exact_quarantine_replay_rejects_missing_corrupt_or_duplicate_resolution_audit() {
    for tamper in ["delete", "corrupt", "duplicate"] {
        let (mut store, _dir, path) = test_store();
        let rejected = RejectedDocument::from_bytes(
            br#"{"schema_version":"psyche.future.v1"}"#,
            RejectionReason::UnknownSchema,
        );
        let id = store.quarantine(rejected.clone()).unwrap();
        let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
        store
            .resolve_quarantine(
                &id,
                &resolution(
                    QuarantineResolutionCode::ConfirmedInvalid,
                    discovered_at + Duration::seconds(1),
                ),
            )
            .unwrap();

        let connection = raw_connection(&path);
        match tamper {
            "delete" => {
                connection
                    .execute(
                        "DELETE FROM audit_events WHERE correlation_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            "corrupt" => {
                connection
                    .execute(
                        "UPDATE audit_events SET public_details_json = X'7B' WHERE correlation_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            "duplicate" => {
                connection
                    .execute(
                        "
                        INSERT INTO audit_events (
                            event_code, correlation_id, public_details_json, created_at
                        )
                        SELECT event_code, correlation_id, public_details_json, created_at
                        FROM audit_events
                        WHERE correlation_id = ?1
                        ",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);

        assert_database_corruption(store.quarantine(rejected));
    }
}

#[test]
fn quarantine_resolution_rejects_unknown_stale_or_conflicting_requests() {
    let (mut store, _dir, _path) = test_store();
    let unknown = QuarantineId::parse("qua_01J00000000000000000000000").unwrap();
    let unknown_resolution = resolution(
        QuarantineResolutionCode::ConfirmedInvalid,
        OffsetDateTime::UNIX_EPOCH,
    );
    assert!(matches!(
        store.resolve_quarantine(&unknown, &unknown_resolution),
        Err(StoreError::QuarantineNotFound { .. })
    ));

    let id = quarantined_fixture(&mut store);
    let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
    assert!(matches!(
        store.resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at - Duration::nanoseconds(1),
            )
        ),
        Err(StoreError::InvalidQuarantineResolution { .. })
    ));
    assert!(matches!(
        store.resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at.to_offset(UtcOffset::from_hms(1, 0, 0).unwrap()),
            )
        ),
        Err(StoreError::InvalidQuarantineResolution { .. })
    ));

    store
        .resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
        .unwrap();
    assert!(matches!(
        store.resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::DuplicatePayload,
                discovered_at + Duration::seconds(2),
            )
        ),
        Err(StoreError::QuarantineResolutionConflict { .. })
    ));
    assert_eq!(store.audit_events().unwrap().len(), 1);
}

#[test]
fn concurrent_quarantine_resolution_has_one_durable_winner() {
    let (mut first, _dir, path) = test_store();
    let id = quarantined_fixture(&mut first);
    let discovered_at = first.quarantine_record(&id).unwrap().unwrap().discovered_at;
    let second = Store::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let left_id = id.clone();
    let left_barrier = Arc::clone(&barrier);
    let left = std::thread::spawn(move || {
        let mut store = first;
        left_barrier.wait();
        store.resolve_quarantine(
            &left_id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
    });
    let right_id = id.clone();
    let right_barrier = Arc::clone(&barrier);
    let right = std::thread::spawn(move || {
        let mut store = second;
        right_barrier.wait();
        store.resolve_quarantine(
            &right_id,
            &resolution(
                QuarantineResolutionCode::DuplicatePayload,
                discovered_at + Duration::seconds(2),
            ),
        )
    });

    let results = [left.join().unwrap(), right.join().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(ResolveQuarantineOutcome::Resolved { .. })))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::QuarantineResolutionConflict { .. })))
            .count(),
        1
    );

    let reopened = Store::open(&path).unwrap();
    assert!(
        reopened
            .quarantine_record(&id)
            .unwrap()
            .unwrap()
            .resolution_digest
            .is_some()
    );
    assert_eq!(
        reopened
            .audit_events()
            .unwrap()
            .iter()
            .filter(|event| event.event_code == "quarantine_resolved")
            .count(),
        1
    );
}

#[test]
fn resolution_audit_details_are_canonical_and_payload_redacted() {
    let (mut store, _dir, _path) = test_store();
    let secret = "resolution-payload-secret";
    let id = store
        .quarantine(RejectedDocument::from_bytes(
            secret.as_bytes(),
            RejectionReason::UnknownSchema,
        ))
        .unwrap();
    let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
    store
        .resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
        .unwrap();

    let events = store.audit_events().unwrap();
    assert_eq!(events.len(), 1);
    let details: serde_json::Value =
        serde_json::from_slice(&events[0].public_details_json).unwrap();
    assert_eq!(
        canonical_bytes(&details).unwrap(),
        events[0].public_details_json
    );
    assert!(
        !String::from_utf8(events[0].public_details_json.clone())
            .unwrap()
            .contains(secret)
    );
}

#[test]
fn corrupted_resolution_columns_or_metadata_fail_closed() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    raw_connection(&path)
        .execute_batch(
            "
            PRAGMA ignore_check_constraints = ON;
            UPDATE quarantine_records
            SET resolved_at = '2026-08-05T12:00:00Z'
            WHERE resolution_code IS NULL;
            ",
        )
        .unwrap();

    assert_database_corruption(store.quarantine_record(&id));
    assert_database_corruption(store.resolve_quarantine(
        &id,
        &resolution(
            QuarantineResolutionCode::ConfirmedInvalid,
            at("2026-08-05T12:00:01Z"),
        ),
    ));
    assert_database_corruption(store.prune(at("2026-08-06T00:00:00Z")));
}

#[test]
fn malformed_persisted_quarantine_id_fails_prune_before_deleting_valid_rows() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
    store
        .resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
        .unwrap();
    raw_connection(&path)
        .execute(
            "
            INSERT INTO quarantine_records (
                quarantine_id, schema_version, payload_digest, bounded_payload,
                reason, discovered_at
            ) VALUES ('bad-id', NULL, ?1, X'', 'unknown_schema', ?2)
            ",
            params![
                fixture_digest('b').as_str(),
                discovered_at.format(&Rfc3339).unwrap()
            ],
        )
        .unwrap();

    assert_database_corruption(store.prune(discovered_at + Duration::days(1)));
    assert!(store.quarantine_record(&id).unwrap().is_some());
}

#[test]
fn malformed_canonical_created_at_fails_prune_before_deleting_eligible_quarantine() {
    for tampered in [
        "not-a-timestamp",
        "2026-08-05T13:00:00+01:00",
        "2026-08-05T12:00:00+00:00",
    ] {
        let (mut store, _dir, path) = test_store();
        store
            .insert(&CanonicalDocument::Intent(fixture_intent()))
            .unwrap();
        let id = quarantined_fixture(&mut store);
        let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
        let resolved_at = discovered_at + Duration::seconds(1);
        store
            .resolve_quarantine(
                &id,
                &resolution(QuarantineResolutionCode::ConfirmedInvalid, resolved_at),
            )
            .unwrap();
        raw_connection(&path)
            .execute("UPDATE canonical_records SET created_at = ?1", [tampered])
            .unwrap();

        assert_database_corruption(store.prune(resolved_at + Duration::nanoseconds(1)));
        assert!(store.quarantine_record(&id).unwrap().is_some());
    }
}

#[test]
fn prune_uses_strictly_older_cutoff_and_never_deletes_unresolved_rows() {
    let (mut store, _dir, _path) = test_store();
    let resolved = quarantined_fixture(&mut store);
    let discovered_at = store
        .quarantine_record(&resolved)
        .unwrap()
        .unwrap()
        .discovered_at;
    let resolved_at = discovered_at + Duration::seconds(1);
    store
        .resolve_quarantine(
            &resolved,
            &resolution(QuarantineResolutionCode::ConfirmedInvalid, resolved_at),
        )
        .unwrap();
    let unresolved = store
        .quarantine(RejectedDocument::from_bytes(
            br#"{"schema_version":"psyche.future.v1"}"#,
            RejectionReason::UnknownSchema,
        ))
        .unwrap();

    let equal = store.prune(resolved_at).unwrap();
    assert_eq!(equal.resolved_quarantine_deleted, 0);
    assert!(store.quarantine_record(&resolved).unwrap().is_some());

    let older = store.prune(resolved_at + Duration::nanoseconds(1)).unwrap();
    assert_eq!(older.resolved_quarantine_deleted, 1);
    assert_eq!(older.unresolved_quarantine_deleted, 0);
    assert!(store.quarantine_record(&resolved).unwrap().is_none());
    assert!(store.quarantine_record(&unresolved).unwrap().is_some());
}

#[test]
fn pruning_preserves_unresolved_quarantine_binding_revisions_and_transitions() {
    let (mut store, _dir, _path) = test_store();
    let binding = fixture_binding();
    store
        .insert(&CanonicalDocument::ExecutionBinding(binding.clone()))
        .unwrap();
    store.append_transition(&fixture_transition()).unwrap();

    let resolved = quarantined_fixture(&mut store);
    let discovered_at = store
        .quarantine_record(&resolved)
        .unwrap()
        .unwrap()
        .discovered_at;
    store
        .resolve_quarantine(
            &resolved,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
        .unwrap();
    let unresolved = store
        .quarantine(RejectedDocument::from_bytes(
            br#"{"schema_version":"psyche.other.v1"}"#,
            RejectionReason::UnknownSchema,
        ))
        .unwrap();

    let bindings_before = store
        .execution_binding_revisions(&fixture_attempt_id())
        .unwrap();
    let transitions_before = store.transitions(&fixture_attempt_id()).unwrap();
    let audit_before = store.audit_events().unwrap();
    let report = store.prune(discovered_at + Duration::days(1)).unwrap();
    assert_eq!(report.resolved_quarantine_deleted, 1);
    assert_eq!(report.unresolved_quarantine_deleted, 0);
    assert_eq!(report.execution_binding_revisions_deleted, 0);
    assert_eq!(report.transitions_deleted, 0);
    assert_eq!(report.audit_events_deleted, 0);
    assert!(store.quarantine_record(&unresolved).unwrap().is_some());
    assert_eq!(
        store
            .execution_binding_revisions(&fixture_attempt_id())
            .unwrap(),
        bindings_before
    );
    assert_eq!(
        store.transitions(&fixture_attempt_id()).unwrap(),
        transitions_before
    );
    assert_eq!(store.audit_events().unwrap(), audit_before);
}

#[test]
fn checkpoint_preserves_committed_state() {
    let (mut store, _dir, _path) = test_store();
    let id = quarantined_fixture(&mut store);
    store.checkpoint().unwrap();
    assert!(store.quarantine_record(&id).unwrap().is_some());
}
