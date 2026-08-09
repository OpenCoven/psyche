//! Test-only migration crash driver.

use std::path::Path;

use psyche_core::contracts::execution::{AdoptionState, CancellationState};
use psyche_core::contracts::{
    CanonicalDocument, ExecutionBinding, Intent, RecordKind, SchemaKind, SchemaVersion,
};
use psyche_core::digest::{Sha256Digest, digest};
use psyche_core::id::{RecordId, RequestId};
use rusqlite::TransactionBehavior;
use serde_json::Map;
use time::format_description::well_known::Rfc3339;

use crate::{
    IngestOutcome, QuarantineResolution, QuarantineResolutionCode, Store, StoreError, Transition,
    execution_bindings, records, transitions,
};

const BASELINE_INTENT_ID: &str = "int_01J00000000000000000000001";
const COMMITTED_INTENT_ID: &str = "int_01J00000000000000000000002";
const PENDING_INTENT_ID: &str = "int_01J00000000000000000000003";
const ATTEMPT_ID: &str = "att_01J00000000000000000000004";

/// The migration boundary where the crash test aborts the process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationFaultPoint {
    /// Migration SQL ran inside the exclusive transaction, but its version did not commit.
    AfterMigrationSqlBeforeUserVersion,
}

/// Store transaction boundary where the crash test aborts the process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreFaultPoint {
    /// A valid canonical record was inserted but its transaction did not commit.
    BeforeCommit,
    /// A valid canonical record was inserted before its transition in the same transaction.
    AfterRecordBeforeTransition,
    /// The next authenticated binding revision was inserted but did not commit.
    AfterBindingRevisionBeforeCommit,
    /// A complete record and transition transaction committed before WAL checkpointing.
    AfterCommitBeforeCheckpoint,
}

/// Applies migration one in the production transaction shape and aborts at `fault_point`.
pub fn run_migration_with_fault(
    path: &Path,
    fault_point: MigrationFaultPoint,
) -> Result<(), StoreError> {
    let (database_path, _) = crate::connection::prepare(path)?;
    let mut connection = crate::connection::open_read_write(&database_path)?;
    crate::connection::configure(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
    crate::migrations::apply_migration_sql(&transaction, 1)?;

    match fault_point {
        MigrationFaultPoint::AfterMigrationSqlBeforeUserVersion => {
            eprintln!("fault-ready:exit-during-migration:sql-applied");
            std::process::abort()
        }
    }
}

/// Seeds authenticated state through public APIs, then aborts at `fault_point`.
pub fn run_store_with_fault(path: &Path, fault_point: StoreFaultPoint) -> Result<(), StoreError> {
    let mut store = Store::open(path)?;
    seed_authenticated_baseline(&mut store)?;

    let pending = CanonicalDocument::Intent(intent(PENDING_INTENT_ID, "pending write")?);
    let committed = CanonicalDocument::Intent(intent(COMMITTED_INTENT_ID, "committed write")?);
    let pending_transition = Transition::new(
        SchemaKind::Intent,
        record_id(RecordKind::Intent, PENDING_INTENT_ID)?,
        1,
        None,
        "accepted".to_owned(),
        at("2026-08-08T00:00:03Z")?,
    )?;
    let committed_transition = Transition::new(
        SchemaKind::Intent,
        record_id(RecordKind::Intent, COMMITTED_INTENT_ID)?,
        1,
        None,
        "accepted".to_owned(),
        at("2026-08-08T00:00:04Z")?,
    )?;
    let binding_revision = binding_revision_3()?;
    binding_revision.validate()?;

    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    match fault_point {
        StoreFaultPoint::BeforeCommit => {
            records::insert_canonical_in_transaction(&transaction, &pending)?;
            transitions::append_in_transaction(&transaction, &pending_transition)?;
            eprintln!("fault-ready:exit-before-commit:record+transition");
            std::process::abort()
        }
        StoreFaultPoint::AfterRecordBeforeTransition => {
            records::insert_canonical_in_transaction(&transaction, &pending)?;
            eprintln!("fault-ready:exit-after-record-before-transition:record-only");
            std::process::abort()
        }
        StoreFaultPoint::AfterBindingRevisionBeforeCommit => {
            execution_bindings::insert_in_transaction(&transaction, &binding_revision)?;
            eprintln!("fault-ready:exit-after-binding-revision-before-commit:binding-revision");
            std::process::abort()
        }
        StoreFaultPoint::AfterCommitBeforeCheckpoint => {
            records::insert_canonical_in_transaction(&transaction, &committed)?;
            transitions::append_in_transaction(&transaction, &committed_transition)?;
            transaction.commit()?;
            std::process::abort()
        }
    }
}

/// Exercises the shared binding primitive with a deliberately invalid contract.
pub fn insert_invalid_binding_with_production_primitive(path: &Path) -> Result<(), StoreError> {
    let mut store = Store::open(path)?;
    let mut invalid = binding_revision_1()?;
    invalid.request_valid_until = invalid.request_created_at;
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = execution_bindings::insert_in_transaction(&transaction, &invalid);
    drop(transaction);
    result.map(|_| ())
}

fn seed_authenticated_baseline(store: &mut Store) -> Result<(), StoreError> {
    let baseline = CanonicalDocument::Intent(intent(BASELINE_INTENT_ID, "baseline")?);
    store.insert(&baseline)?;

    let revision_1 = binding_revision_1()?;
    let revision_2 = next_binding_revision(&revision_1)?;
    store.insert(&CanonicalDocument::ExecutionBinding(revision_1))?;
    store.insert(&CanonicalDocument::ExecutionBinding(revision_2))?;

    let baseline_id = record_id(RecordKind::Intent, BASELINE_INTENT_ID)?;
    store.append_transition(&Transition::new(
        SchemaKind::Intent,
        baseline_id.clone(),
        1,
        None,
        "accepted".to_owned(),
        at("2026-08-08T00:00:01Z")?,
    )?)?;
    store.append_transition(&Transition::new(
        SchemaKind::Intent,
        baseline_id,
        2,
        Some("accepted".to_owned()),
        "completed".to_owned(),
        at("2026-08-08T00:00:02Z")?,
    )?)?;

    let IngestOutcome::Quarantined { quarantine_id } =
        store.ingest(br#"{"schema_version":"psyche.intent.v2"}"#)?
    else {
        return Err(StoreError::DatabaseCorruption);
    };
    let quarantined = store
        .quarantine_record(&quarantine_id)?
        .ok_or(StoreError::DatabaseCorruption)?;
    store.resolve_quarantine(
        &quarantine_id,
        &QuarantineResolution {
            code: QuarantineResolutionCode::ConfirmedInvalid,
            resolved_at: quarantined.discovered_at + time::Duration::seconds(1),
        },
    )?;
    Ok(())
}

fn intent(id: &str, requested_outcome: &str) -> Result<Intent, StoreError> {
    Ok(Intent {
        schema_version: SchemaVersion::parse("psyche.intent.v1")?,
        intent_id: record_id(RecordKind::Intent, id)?,
        principal_id: "principal-crash-test".to_owned(),
        familiar_snapshot_id: record_id(
            RecordKind::IdentitySnapshot,
            "ids_01J00000000000000000000005",
        )?,
        project_id: "project-crash-test".to_owned(),
        requested_outcome: requested_outcome.to_owned(),
        constraints: Map::new(),
        required_evidence: vec!["review".to_owned()],
        surface_event_id: None,
        created_at: at("2026-08-08T00:00:00Z")?,
        digest: fixture_digest('a')?,
    })
}

fn binding_revision_1() -> Result<ExecutionBinding, StoreError> {
    Ok(ExecutionBinding {
        schema_version: SchemaVersion::parse("psyche.execution_binding.v1")?,
        attempt_id: record_id(RecordKind::Attempt, ATTEMPT_ID)?,
        revision: 1,
        previous_revision_digest: None,
        revision_created_at: at("2026-08-08T00:00:00Z")?,
        familiar_snapshot_id: record_id(
            RecordKind::IdentitySnapshot,
            "ids_01J00000000000000000000005",
        )?,
        project_id: "project-crash-test".to_owned(),
        request_id: RequestId::parse("req_01J00000000000000000000006")?,
        request_digest: fixture_digest('b')?,
        request_created_at: at("2026-08-07T23:59:00Z")?,
        request_valid_until: at("2026-08-08T00:05:00Z")?,
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
    })
}

fn next_binding_revision(previous: &ExecutionBinding) -> Result<ExecutionBinding, StoreError> {
    let mut next = previous.clone();
    next.revision = previous
        .revision
        .checked_add(1)
        .ok_or(StoreError::DatabaseOperation)?;
    next.previous_revision_digest = Some(digest(previous)?);
    next.revision_created_at += time::Duration::nanoseconds(1);
    Ok(next)
}

fn binding_revision_3() -> Result<ExecutionBinding, StoreError> {
    let revision_1 = binding_revision_1()?;
    let revision_2 = next_binding_revision(&revision_1)?;
    next_binding_revision(&revision_2)
}

fn fixture_digest(character: char) -> Result<Sha256Digest, StoreError> {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64)))
        .map_err(StoreError::from)
}

fn record_id(kind: RecordKind, value: &str) -> Result<RecordId, StoreError> {
    RecordId::parse(kind, value).map_err(StoreError::from)
}

fn at(value: &str) -> Result<time::OffsetDateTime, StoreError> {
    time::OffsetDateTime::parse(value, &Rfc3339).map_err(|_| StoreError::DatabaseOperation)
}
