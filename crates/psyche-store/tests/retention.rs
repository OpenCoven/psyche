#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};

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

fn complete_64_kib_payload() -> Vec<u8> {
    let empty = r#"{"schema_version":"psyche.future.v1","padding":""}"#;
    let padding = "x".repeat(64 * 1024 - empty.len());
    let bytes =
        format!(r#"{{"schema_version":"psyche.future.v1","padding":"{padding}"}}"#).into_bytes();
    assert_eq!(bytes.len(), 64 * 1024);
    bytes
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

fn assert_quarantine_paths_detect_corruption(
    store: &mut Store,
    id: &QuarantineId,
    discovered_at: OffsetDateTime,
) {
    assert_database_corruption(store.quarantine_record(id));
    assert_database_corruption(store.resolve_quarantine(
        id,
        &resolution(
            QuarantineResolutionCode::ConfirmedInvalid,
            discovered_at + Duration::seconds(1),
        ),
    ));
    assert_database_corruption(store.prune(discovered_at + Duration::days(1)));
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
fn quarantine_rejects_forged_exactly_64_kib_digest() {
    let (mut store, _dir, _path) = test_store();
    let mut rejected =
        RejectedDocument::from_bytes(&vec![b'x'; 64 * 1024], RejectionReason::TooLarge);
    rejected.payload_digest = fixture_digest('a');

    let error = store.quarantine(rejected).unwrap_err();
    assert!(matches!(error, StoreError::InvalidQuarantineRecord));
    assert_eq!(error.to_string(), "quarantine record is invalid");
    assert_eq!(
        format!("{error:?}"),
        "StoreError(quarantine record is invalid)"
    );
    assert!(error.source().is_none());
}

#[test]
fn quarantine_rejects_exactly_64_kib_public_field_mutations() {
    let (mut store, _dir, _path) = test_store();
    let original =
        RejectedDocument::from_bytes(&vec![b'x'; 64 * 1024], RejectionReason::UnknownSchema);

    let mut schema_mutated = original.clone();
    schema_mutated.schema_version = Some("psyche.other.v1".to_owned());
    let mut digest_mutated = original.clone();
    digest_mutated.payload_digest = fixture_digest('b');
    let mut payload_mutated = original.clone();
    payload_mutated.bounded_payload[0] = b'y';
    let mut reason_mutated = original;
    reason_mutated.reason = RejectionReason::TooLarge;

    for rejected in [
        schema_mutated,
        digest_mutated,
        payload_mutated,
        reason_mutated,
    ] {
        let error = store.quarantine(rejected).unwrap_err();
        assert!(matches!(error, StoreError::InvalidQuarantineRecord));
        assert_eq!(error.to_string(), "quarantine record is invalid");
        assert!(error.source().is_none());
    }
}

#[test]
fn complete_64_kib_quarantine_integrity_tampering_fails_closed() {
    for tamper in [
        "bounded_payload",
        "retained_payload_digest",
        "original_payload_len",
        "negative_original_payload_len",
        "payload_digest",
        "schema_version",
    ] {
        let (mut store, _dir, path) = test_store();
        let payload = complete_64_kib_payload();
        let id = store
            .quarantine(RejectedDocument::from_bytes(
                &payload,
                RejectionReason::UnknownSchema,
            ))
            .unwrap();
        let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
        let connection = raw_connection(&path);
        match tamper {
            "bounded_payload" => {
                let mut mutated = payload;
                let index = mutated.iter().rposition(|byte| *byte == b'x').unwrap();
                mutated[index] = b'y';
                connection
                    .execute(
                        "UPDATE quarantine_records SET bounded_payload = ?1 WHERE quarantine_id = ?2",
                        params![mutated, id.as_str()],
                    )
                    .unwrap();
            }
            "retained_payload_digest" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET retained_payload_digest = ?1 WHERE quarantine_id = ?2",
                        params![fixture_digest('d').as_str(), id.as_str()],
                    )
                    .unwrap();
            }
            "original_payload_len" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET original_payload_len = ?1 WHERE quarantine_id = ?2",
                        params![64 * 1024 - 1, id.as_str()],
                    )
                    .unwrap();
            }
            "negative_original_payload_len" => {
                connection
                    .execute_batch("PRAGMA ignore_check_constraints = ON;")
                    .unwrap();
                connection
                    .execute(
                        "UPDATE quarantine_records SET original_payload_len = -1 WHERE quarantine_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            "payload_digest" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET payload_digest = ?1 WHERE quarantine_id = ?2",
                        params![fixture_digest('e').as_str(), id.as_str()],
                    )
                    .unwrap();
            }
            "schema_version" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET schema_version = 'psyche.other.v1' WHERE quarantine_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);

        assert_quarantine_paths_detect_corruption(&mut store, &id, discovered_at);
    }
}

#[test]
fn short_quarantine_integrity_tampering_fails_closed() {
    for tamper in [
        "bounded_payload",
        "retained_payload_digest",
        "original_payload_len",
        "payload_digest",
        "schema_version",
    ] {
        let (mut store, _dir, path) = test_store();
        let payload = br#"{"schema_version":"psyche.future.v1"}"#;
        let id = store
            .quarantine(RejectedDocument::from_bytes(
                payload,
                RejectionReason::UnknownSchema,
            ))
            .unwrap();
        let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
        let connection = raw_connection(&path);
        match tamper {
            "bounded_payload" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET bounded_payload = ?1 WHERE quarantine_id = ?2",
                        params![b"corrupted payload", id.as_str()],
                    )
                    .unwrap();
            }
            "retained_payload_digest" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET retained_payload_digest = ?1 WHERE quarantine_id = ?2",
                        params![fixture_digest('d').as_str(), id.as_str()],
                    )
                    .unwrap();
            }
            "original_payload_len" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET original_payload_len = original_payload_len + 1 WHERE quarantine_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            "payload_digest" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET payload_digest = ?1 WHERE quarantine_id = ?2",
                        params![fixture_digest('e').as_str(), id.as_str()],
                    )
                    .unwrap();
            }
            "schema_version" => {
                connection
                    .execute(
                        "UPDATE quarantine_records SET schema_version = 'psyche.other.v1' WHERE quarantine_id = ?1",
                        [id.as_str()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);

        assert_quarantine_paths_detect_corruption(&mut store, &id, discovered_at);
    }
}

#[test]
fn oversized_quarantine_integrity_metadata_round_trips_after_reopen() {
    let (mut store, _dir, path) = test_store();
    let mut payload = vec![b'x'; 64 * 1024];
    payload.extend_from_slice(b"tail-not-retained");
    let rejected = RejectedDocument::from_bytes(&payload, RejectionReason::TooLarge);
    let expected_payload_digest = rejected.payload_digest.clone();
    let expected_retained_digest = rejected.retained_payload_digest();
    let id = store.quarantine(rejected.clone()).unwrap();
    drop(store);

    let mut reopened = Store::open(&path).unwrap();
    let record = reopened.quarantine_record(&id).unwrap().unwrap();
    assert_eq!(record.original_payload_len, payload.len());
    assert_eq!(record.retained_payload_digest, expected_retained_digest);
    assert_eq!(record.payload_digest, expected_payload_digest);
    assert_eq!(record.bounded_payload, payload[..64 * 1024]);
    assert_eq!(reopened.quarantine(rejected).unwrap(), id);
}

#[test]
fn oversized_quarantine_immutable_metadata_tampering_fails_closed() {
    for mutation in [
        "payload_digest",
        "schema_version",
        "original_payload_len",
        "reason",
        "discovered_at",
    ] {
        let (mut store, _dir, path) = test_store();
        let payload = vec![b'x'; 64 * 1024 + 17];
        let id = store
            .quarantine(RejectedDocument::from_bytes(
                &payload,
                RejectionReason::TooLarge,
            ))
            .unwrap();
        let discovered_at = store.quarantine_record(&id).unwrap().unwrap().discovered_at;
        let connection = raw_connection(&path);
        let sql = match mutation {
            "payload_digest" => {
                "UPDATE quarantine_records SET payload_digest = \
                 'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd' \
                 WHERE quarantine_id = ?1"
            }
            "schema_version" => {
                "UPDATE quarantine_records SET schema_version = 'psyche.future.v1' \
                 WHERE quarantine_id = ?1"
            }
            "original_payload_len" => {
                "UPDATE quarantine_records SET original_payload_len = original_payload_len + 1 \
                 WHERE quarantine_id = ?1"
            }
            "reason" => {
                "UPDATE quarantine_records SET reason = 'unknown_schema' \
                 WHERE quarantine_id = ?1"
            }
            "discovered_at" => {
                "UPDATE quarantine_records SET discovered_at = '2026-08-08T00:00:00Z' \
                 WHERE quarantine_id = ?1"
            }
            _ => unreachable!(),
        };
        connection.execute(sql, [id.as_str()]).unwrap();
        drop(connection);

        assert_quarantine_paths_detect_corruption(&mut store, &id, discovered_at);
    }
}

#[test]
fn dedupe_rejects_corrupt_integrity_metadata() {
    let (mut store, _dir, path) = test_store();
    let payload = vec![b'x'; 64 * 1024 + 17];
    let rejected = RejectedDocument::from_bytes(&payload, RejectionReason::TooLarge);
    let id = store.quarantine(rejected.clone()).unwrap();
    raw_connection(&path)
        .execute(
            "UPDATE quarantine_records SET original_payload_len = original_payload_len + 1 WHERE quarantine_id = ?1",
            [id.as_str()],
        )
        .unwrap();

    assert_database_corruption(store.quarantine(rejected));
}

#[test]
fn dedupe_rejects_corrupt_lookup_keys_before_inserting() {
    for column in ["payload_digest", "reason"] {
        let (mut store, _dir, path) = test_store();
        let payload = vec![b'x'; 64 * 1024 + 17];
        let rejected = RejectedDocument::from_bytes(&payload, RejectionReason::TooLarge);
        let id = store.quarantine(rejected.clone()).unwrap();
        let connection = raw_connection(&path);
        let sql = match column {
            "payload_digest" => {
                "UPDATE quarantine_records SET payload_digest = \
                 'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd' \
                 WHERE quarantine_id = ?1"
            }
            "reason" => {
                "UPDATE quarantine_records SET reason = 'unknown_schema' \
                 WHERE quarantine_id = ?1"
            }
            _ => unreachable!(),
        };
        connection.execute(sql, [id.as_str()]).unwrap();
        drop(connection);

        assert_database_corruption(store.quarantine(rejected));
        assert_eq!(
            raw_connection(&path)
                .query_row("SELECT COUNT(*) FROM quarantine_records", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn quarantine_replay_validates_persisted_integrity_metadata() {
    let (mut store, _dir, path) = test_store();
    let payload = vec![b'x'; 64 * 1024 + 1];
    let rejected = RejectedDocument::from_bytes(&payload, RejectionReason::TooLarge);
    let id = store.quarantine(rejected.clone()).unwrap();
    raw_connection(&path)
        .execute(
            "UPDATE quarantine_records SET retained_payload_digest = ?1 WHERE quarantine_id = ?2",
            params![fixture_digest('f').as_str(), id.as_str()],
        )
        .unwrap();

    assert_database_corruption(store.quarantine(rejected));
}

#[test]
fn complete_persisted_quarantine_rejects_valid_format_payload_tampering() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    raw_connection(&path)
        .execute(
            "UPDATE quarantine_records SET bounded_payload = ?1 WHERE quarantine_id = ?2",
            params![br#"{"schema_version":"psyche.other.v1"}"#, id.as_str()],
        )
        .unwrap();

    assert_database_corruption(store.quarantine_record(&id));
}

#[test]
fn complete_persisted_quarantine_rejects_valid_format_digest_tampering() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    raw_connection(&path)
        .execute(
            "UPDATE quarantine_records SET payload_digest = ?1 WHERE quarantine_id = ?2",
            params![fixture_digest('d').as_str(), id.as_str()],
        )
        .unwrap();

    assert_database_corruption(store.quarantine_record(&id));
}

#[test]
fn complete_persisted_quarantine_rejects_valid_format_schema_tampering() {
    let (mut store, _dir, path) = test_store();
    let id = quarantined_fixture(&mut store);
    raw_connection(&path)
        .execute(
            "UPDATE quarantine_records SET schema_version = ?1 WHERE quarantine_id = ?2",
            params!["psyche.other.v1", id.as_str()],
        )
        .unwrap();

    assert_database_corruption(store.quarantine_record(&id));
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
fn concurrent_quarantine_reads_observe_consistent_resolution_snapshots() {
    const READER_COUNT: usize = 16;

    let (mut creator, _dir, path) = test_store();
    let id = quarantined_fixture(&mut creator);
    let discovered_at = creator
        .quarantine_record(&id)
        .unwrap()
        .unwrap()
        .discovered_at;
    drop(creator);

    let mut resolver = Store::open(&path).unwrap();
    let readers = (0..READER_COUNT)
        .map(|_| Store::open(&path).unwrap())
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(READER_COUNT + 1));
    let resolved = Arc::new(AtomicBool::new(false));
    let handles = readers
        .into_iter()
        .map(|reader| {
            let id = id.clone();
            let barrier = Arc::clone(&barrier);
            let resolved = Arc::clone(&resolved);
            std::thread::spawn(move || -> Result<(), StoreError> {
                reader.quarantine_record(&id)?;
                barrier.wait();
                while !resolved.load(Ordering::Acquire) {
                    reader.quarantine_record(&id)?;
                    std::thread::yield_now();
                }
                for _ in 0..32 {
                    reader.quarantine_record(&id)?;
                }
                Ok(())
            })
        })
        .collect::<Vec<_>>();

    barrier.wait();
    std::thread::sleep(std::time::Duration::from_millis(10));
    resolver
        .resolve_quarantine(
            &id,
            &resolution(
                QuarantineResolutionCode::ConfirmedInvalid,
                discovered_at + Duration::seconds(1),
            ),
        )
        .unwrap();
    resolved.store(true, Ordering::Release);

    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    let final_record = resolver.quarantine_record(&id).unwrap().unwrap();
    assert_eq!(
        final_record.resolution_code,
        Some(QuarantineResolutionCode::ConfirmedInvalid)
    );
    assert!(final_record.resolution_digest.is_some());
    assert_eq!(resolver.audit_events().unwrap().len(), 1);
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
    let empty_payload =
        RejectedDocument::from_bytes(b"", RejectionReason::UnknownSchema).payload_digest;
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
                quarantine_id, schema_version, payload_digest, original_payload_len,
                retained_payload_digest, integrity_digest, bounded_payload, reason, discovered_at
            ) VALUES ('bad-id', NULL, ?1, 0, ?1, ?1, X'', 'unknown_schema', ?2)
            ",
            params![
                empty_payload.as_str(),
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
