#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::path::Path;
use std::process::{Command, Output};

use psyche_core::contracts::{CanonicalDocument, ContractError, RecordKind, SchemaKind};
use psyche_core::digest::digest;
use psyche_core::id::RecordId;
use psyche_store::{CURRENT_DATABASE_VERSION, Store, StoreError};
use rusqlite::{Connection, OptionalExtension};

const BASELINE_INTENT_ID: &str = "int_01J00000000000000000000001";
const COMMITTED_INTENT_ID: &str = "int_01J00000000000000000000002";
const PENDING_INTENT_ID: &str = "int_01J00000000000000000000003";
const ATTEMPT_ID: &str = "att_01J00000000000000000000004";

#[test]
fn binding_transaction_primitive_rejects_invalid_contract_before_writing() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("private").join("psyche.sqlite3");

    let error =
        psyche_store::migration_test_support::insert_invalid_binding_with_production_primitive(
            &database,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        StoreError::Contract(ContractError::InvalidShape {
            schema: SchemaKind::ExecutionBinding,
            field: "request_valid_until",
        })
    ));
    let connection = Connection::open(&database).unwrap();
    assert_eq!(row_count(&connection, "execution_binding_revisions"), 0);
}

const CRASH_MODES: [&str; 4] = [
    "exit-before-commit",
    "exit-after-record-before-transition",
    "exit-after-binding-revision-before-commit",
    "exit-after-commit-before-checkpoint",
];

#[test]
fn killed_writer_exposes_only_committed_state_after_reopen() {
    for mode in CRASH_MODES {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("private").join("psyche.sqlite3");

        let output = run_crash_writer(mode, &database);
        if mode != "exit-after-commit-before-checkpoint" {
            let expected_witness = match mode {
                "exit-before-commit" => "fault-ready:exit-before-commit:record+transition",
                "exit-after-record-before-transition" => {
                    "fault-ready:exit-after-record-before-transition:record-only"
                }
                "exit-after-binding-revision-before-commit" => {
                    "fault-ready:exit-after-binding-revision-before-commit:binding-revision"
                }
                _ => unreachable!(),
            };
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .any(|line| line == expected_witness),
                "{mode} did not prove the write was attempted"
            );
        }
        assert_integrity(&database);

        let store = Store::open(&database).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
        assert_authenticated_state(&store, mode);
        drop(store);
        Store::open(&database).unwrap();

        let connection = Connection::open(&database).unwrap();
        let committed = mode == "exit-after-commit-before-checkpoint";
        assert_eq!(
            row_count(&connection, "canonical_records"),
            1 + i64::from(committed)
        );
        assert_eq!(row_count(&connection, "execution_binding_revisions"), 2);
        assert_eq!(
            row_count(&connection, "transitions"),
            2 + i64::from(committed)
        );
        assert_eq!(row_count(&connection, "quarantine_records"), 1);
        assert_eq!(row_count(&connection, "audit_events"), 1);
        assert_no_identity_conflicts(&connection);
    }
}

#[test]
fn killed_inside_migration_transaction_rolls_back_before_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("private").join("psyche.sqlite3");

    let output = run_crash_writer("exit-during-migration", &database);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line == "fault-ready:exit-during-migration:sql-applied"),
        "migration helper did not reach the post-SQL pre-version fault boundary"
    );

    let connection = Connection::open(&database).unwrap();
    assert_eq!(user_version(&connection), 0);
    assert!(user_tables(&connection).is_empty());
    assert_eq!(integrity_check(&connection), "ok");
    drop(connection);

    let first = Store::open(&database).unwrap();
    assert_eq!(first.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
    drop(first);
    let second = Store::open(&database).unwrap();
    assert_eq!(second.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
    drop(second);

    let migrated = Connection::open(&database).unwrap();
    let expected_path = directory.path().join("private").join("expected.sqlite3");
    let expected = Connection::open(&expected_path).unwrap();
    expected
        .execute_batch(include_str!("fixtures/v1.sql"))
        .unwrap();
    drop(migrated);
    drop(expected);
    assert_eq!(schema_snapshot(&database), schema_snapshot(&expected_path));
    let migrated = Connection::open(&database).unwrap();
    assert_eq!(row_count(&migrated, "schema_migrations"), 1);
    assert_eq!(integrity_check(&migrated), "ok");
}

#[test]
fn crash_writer_rejects_every_unknown_fault_point() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("private").join("psyche.sqlite3");
    let output = Command::new(env!("CARGO_BIN_EXE_crash_writer"))
        .arg("unknown-fault")
        .arg(&database)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown crash mode"));
    assert!(!database.exists());
}

fn run_crash_writer(mode: &str, database: &Path) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_crash_writer"))
        .arg(mode)
        .arg(database)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "{mode} unexpectedly returned success"
    );
    assert!(database.exists(), "{mode} did not create a database");
    output
}

fn assert_authenticated_state(store: &Store, mode: &str) {
    let baseline_id = RecordId::parse(RecordKind::Intent, BASELINE_INTENT_ID).unwrap();
    let committed_id = RecordId::parse(RecordKind::Intent, COMMITTED_INTENT_ID).unwrap();
    let pending_id = RecordId::parse(RecordKind::Intent, PENDING_INTENT_ID).unwrap();
    let baseline = store.load(SchemaKind::Intent, &baseline_id).unwrap();
    assert!(matches!(baseline, Some(CanonicalDocument::Intent(_))));
    assert_eq!(
        store.load(SchemaKind::Intent, &pending_id).unwrap(),
        None,
        "uncommitted valid record became visible after {mode}"
    );
    assert!(
        store.transitions(&pending_id).unwrap().is_empty(),
        "uncommitted valid transition became visible after {mode}"
    );
    assert_eq!(
        store
            .load(SchemaKind::Intent, &committed_id)
            .unwrap()
            .is_some(),
        mode == "exit-after-commit-before-checkpoint"
    );

    let attempt_id = RecordId::parse(RecordKind::Attempt, ATTEMPT_ID).unwrap();
    let revisions = store.execution_binding_revisions(&attempt_id).unwrap();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].revision, 1);
    assert_eq!(revisions[0].previous_revision_digest, None);
    assert_eq!(revisions[1].revision, 2);
    assert_eq!(
        revisions[1].previous_revision_digest.as_ref(),
        Some(&digest(&revisions[0]).unwrap())
    );
    for revision in &revisions {
        revision.validate().unwrap();
    }

    let transitions = store.transitions(&baseline_id).unwrap();
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0].record_version, 1);
    assert_eq!(transitions[0].from_state, None);
    assert_eq!(transitions[1].record_version, 2);
    assert_eq!(
        transitions[1].from_state.as_deref(),
        Some(transitions[0].to_state.as_str())
    );
    for transition in &transitions {
        transition.validate().unwrap();
    }
    let committed_transitions = store.transitions(&committed_id).unwrap();
    if mode == "exit-after-commit-before-checkpoint" {
        assert_eq!(committed_transitions.len(), 1);
        assert_eq!(committed_transitions[0].record_version, 1);
        assert_eq!(committed_transitions[0].from_state, None);
        committed_transitions[0].validate().unwrap();
    } else {
        assert!(committed_transitions.is_empty());
    }

    let audit = store.audit_events().unwrap();
    assert_eq!(audit.len(), 1);
    let quarantine_id = psyche_store::QuarantineId::parse(&audit[0].correlation_id).unwrap();
    let quarantine = store.quarantine_record(&quarantine_id).unwrap().unwrap();
    assert!(quarantine.resolved_at.is_some());
    assert!(quarantine.resolution_code.is_some());
    assert!(quarantine.resolution_digest.is_some());
}

fn assert_integrity(path: &Path) {
    let connection = Connection::open(path).unwrap();
    assert_eq!(integrity_check(&connection), "ok");
}

fn integrity_check(connection: &Connection) -> String {
    connection
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .unwrap()
}

fn user_version(connection: &Connection) -> u32 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn row_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn user_tables(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct SchemaSnapshot {
    objects: Vec<(String, String, String, String)>,
    tables: Vec<TableSnapshot>,
    indexes: Vec<(String, Vec<String>)>,
}

#[derive(Debug, PartialEq, Eq)]
struct TableSnapshot {
    name: String,
    columns: Vec<String>,
    foreign_keys: Vec<String>,
    indexes: Vec<String>,
}

fn schema_snapshot(path: &Path) -> SchemaSnapshot {
    let connection = Connection::open(path).unwrap();
    let mut objects_statement = connection
        .prepare(
            "
            SELECT type, name, tbl_name, COALESCE(sql, '')
            FROM sqlite_schema
            WHERE name NOT LIKE 'sqlite_%'
            ORDER BY type, name
            ",
        )
        .unwrap();
    let objects = objects_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();

    let table_names = objects
        .iter()
        .filter(|(kind, _, _, _)| kind == "table")
        .map(|(_, name, _, _)| name.clone())
        .collect::<Vec<_>>();
    let mut index_names = objects
        .iter()
        .filter(|(kind, _, _, _)| kind == "index")
        .map(|(_, name, _, _)| name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let tables = table_names
        .into_iter()
        .map(|table| {
            let columns = pragma_rows(&connection, &format!("PRAGMA table_info('{table}')"));
            let foreign_keys =
                pragma_rows(&connection, &format!("PRAGMA foreign_key_list('{table}')"));
            let index_list = pragma_rows(&connection, &format!("PRAGMA index_list('{table}')"));
            let mut statement = connection
                .prepare("SELECT name FROM pragma_index_list(?1) ORDER BY name")
                .unwrap();
            let names = statement
                .query_map([&table], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            index_names.extend(names);
            TableSnapshot {
                name: table,
                columns,
                foreign_keys,
                indexes: index_list,
            }
        })
        .collect();

    let indexes = index_names
        .into_iter()
        .map(|index| {
            let columns = pragma_rows(&connection, &format!("PRAGMA index_xinfo('{index}')"));
            (index, columns)
        })
        .collect();

    SchemaSnapshot {
        objects,
        tables,
        indexes,
    }
}

fn pragma_rows(connection: &Connection, pragma: &str) -> Vec<String> {
    use rusqlite::types::ValueRef;

    let mut statement = connection.prepare(pragma).unwrap();
    let column_count = statement.column_count();
    statement
        .query_map([], |row| {
            let mut values = Vec::with_capacity(column_count);
            for index in 0..column_count {
                let value = match row.get_ref(index)? {
                    ValueRef::Null => "null".to_owned(),
                    ValueRef::Integer(value) => value.to_string(),
                    ValueRef::Real(value) => value.to_string(),
                    ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
                    ValueRef::Blob(value) => format!("blob:{}", value.len()),
                };
                values.push(value);
            }
            Ok(values.join("\u{1f}"))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn assert_no_identity_conflicts(connection: &Connection) {
    let record_conflict: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM canonical_records GROUP BY kind, record_id HAVING COUNT(*) > 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    let binding_conflict: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM execution_binding_revisions GROUP BY attempt_id, revision HAVING COUNT(*) > 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(record_conflict, None);
    assert_eq!(binding_conflict, None);
}
