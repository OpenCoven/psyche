//! Forward-only SQLite foundation migration integration tests.
#![allow(clippy::unwrap_used)]

mod support;

use std::path::Path;

use psyche_store::{CURRENT_DATABASE_VERSION, Store, StoreError};
use support::{
    FOUNDATION_TABLES, Fixture, fixture_db, foundation_tables, journal_mode, scalar_text,
    schema_migrations, table_exists, user_version,
};

#[test]
fn fresh_store_applies_v1_once_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("private");
    let path = parent.join("psyche.sqlite3");

    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
    drop(store);

    assert_eq!(user_version(&path), CURRENT_DATABASE_VERSION);
    assert_eq!(foundation_tables(&path), FOUNDATION_TABLES);
    assert_eq!(schema_migrations(&path).len(), 1);
    assert_eq!(journal_mode(&path), "wal");

    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
    drop(reopened);
    assert_eq!(schema_migrations(&path).len(), 1);

    assert_private_directory(&parent);
}

#[test]
fn version_zero_fixture_migrates_without_losing_existing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version0);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
    drop(store);

    assert_eq!(
        scalar_text(&path, "SELECT value FROM fixture_v0_marker"),
        "preserve-me"
    );
    assert_eq!(foundation_tables(&path), FOUNDATION_TABLES);
    assert_eq!(schema_migrations(&path).len(), 1);
}

#[test]
fn existing_v1_fixture_opens_without_reapplying_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version1);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
    drop(store);

    assert_eq!(schema_migrations(&path), vec![(1, "fixture-v1".to_owned())]);
    assert_eq!(foundation_tables(&path), FOUNDATION_TABLES);
}

#[test]
fn future_database_version_fails_before_any_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version99);

    let error = Store::open(&path).unwrap_err();
    assert!(matches!(
        error,
        StoreError::UnsupportedDatabaseVersion { found: 99 }
    ));
    assert_eq!(
        error.to_string(),
        "unsupported database version 99; maximum supported version is 1"
    );

    assert_eq!(user_version(&path), 99);
    assert_eq!(
        scalar_text(&path, "SELECT value FROM future_owner"),
        "future-owned"
    );
    assert!(!table_exists(&path, "schema_migrations"));

    let reopened_error = Store::open(&path).unwrap_err();
    assert!(matches!(
        reopened_error,
        StoreError::UnsupportedDatabaseVersion { found: 99 }
    ));
    assert_eq!(user_version(&path), 99);
    assert!(!table_exists(&path, "schema_migrations"));
}

#[test]
fn partially_applied_v1_transaction_rolls_back_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::PartiallyAppliedV1);

    assert_eq!(user_version(&path), 0);
    assert!(!table_exists(&path, "schema_migrations"));
    assert!(!table_exists(&path, "canonical_records"));

    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
    drop(store);

    assert_eq!(foundation_tables(&path), FOUNDATION_TABLES);
    assert_eq!(schema_migrations(&path).len(), 1);

    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), 1);
    drop(reopened);
    assert_eq!(schema_migrations(&path).len(), 1);
}

#[cfg(unix)]
fn assert_private_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[cfg(not(unix))]
fn assert_private_directory(_path: &Path) {}
