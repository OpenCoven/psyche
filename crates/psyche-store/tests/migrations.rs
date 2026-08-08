//! Forward-only SQLite foundation migration integration tests.
#![allow(clippy::unwrap_used)]

mod support;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use psyche_store::{CURRENT_DATABASE_VERSION, Store, StoreError};
use support::{
    FOUNDATION_TABLES, Fixture, execute_batch, fixture_db, foundation_tables, journal_mode,
    scalar_text, schema_migrations, table_exists, user_version,
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
    assert_private_file(&path);
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
    #[cfg(unix)]
    set_mode(&path, 0o644);
    let original_contents = std::fs::read(&path).unwrap();
    let original_metadata = std::fs::metadata(&path).unwrap();
    let original_journal_mode = journal_mode(&path);
    let original_sidecars = sqlite_sidecar_state(&path);

    let error = Store::open(&path).unwrap_err();
    assert!(matches!(
        error,
        StoreError::UnsupportedDatabaseVersion { found: 99, .. }
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
    assert_eq!(journal_mode(&path), original_journal_mode);
    assert_eq!(sqlite_sidecar_state(&path), original_sidecars);
    assert_eq!(std::fs::read(&path).unwrap(), original_contents);
    let metadata = std::fs::metadata(&path).unwrap();
    assert_eq!(metadata.len(), original_metadata.len());
    assert_eq!(
        metadata.modified().unwrap(),
        original_metadata.modified().unwrap()
    );
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o644);

    let reopened_error = Store::open(&path).unwrap_err();
    assert!(matches!(
        reopened_error,
        StoreError::UnsupportedDatabaseVersion { found: 99, .. }
    ));
    assert_eq!(user_version(&path), 99);
    assert!(!table_exists(&path, "schema_migrations"));
    assert_eq!(journal_mode(&path), original_journal_mode);
    assert_eq!(sqlite_sidecar_state(&path), original_sidecars);
}

#[test]
fn production_migration_failure_rolls_back_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::MigrationConflictV1);

    assert_eq!(user_version(&path), 0);
    assert!(!table_exists(&path, "schema_migrations"));
    assert_eq!(
        scalar_text(&path, "SELECT marker FROM canonical_records"),
        "preserve-conflict"
    );

    let error = Store::open(&path).unwrap_err();
    assert_eq!(error.to_string(), "store database operation failed");

    assert_eq!(user_version(&path), 0);
    assert!(!table_exists(&path, "schema_migrations"));
    assert_eq!(
        scalar_text(&path, "SELECT marker FROM canonical_records"),
        "preserve-conflict"
    );
    assert_eq!(foundation_tables(&path), ["canonical_records"]);
    assert!(!table_exists(&path, "execution_binding_revisions"));
    assert!(!table_exists(&path, "transitions"));
    assert!(!table_exists(&path, "quarantine_records"));
    assert!(!table_exists(&path, "audit_events"));

    execute_batch(&path, "DROP TABLE canonical_records;");

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

#[test]
fn empty_path_is_rejected_with_a_stable_error() {
    assert_invalid_database_path(Path::new(""));
}

#[test]
fn memory_path_is_rejected_with_a_stable_error() {
    assert_invalid_database_path(Path::new(":memory:"));
}

#[test]
fn uri_path_is_rejected_with_a_stable_error() {
    assert_invalid_database_path(Path::new("file:psyche.sqlite3"));
}

#[test]
fn root_path_is_rejected_with_a_stable_error() {
    assert_invalid_database_path(Path::new("/"));
}

#[test]
fn concurrent_first_open_applies_migration_once() {
    const THREADS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("psyche.sqlite3"));
    let barrier = Arc::new(Barrier::new(THREADS));
    let handles = (0..THREADS)
        .map(|_| {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let store = Store::open(&path)?;
                store.schema_version()
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        assert_eq!(handle.join().unwrap().unwrap(), CURRENT_DATABASE_VERSION);
    }
    assert_eq!(schema_migrations(&path).len(), 1);
}

#[cfg(unix)]
#[test]
fn existing_shared_parent_permissions_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("existing");
    let path = parent.join("psyche.sqlite3");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, []).unwrap();
    set_mode(&parent, 0o755);
    set_mode(&path, 0o755);

    drop(Store::open(&path).unwrap());

    assert_eq!(mode(&parent), 0o755);
    assert_private_file(&path);
}

#[cfg(unix)]
#[test]
fn relative_filename_preserves_current_directory_permissions() {
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    set_mode(dir.path(), 0o755);

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "relative_filename_open_helper", "--nocapture"])
        .env("PSYCHE_STORE_RELATIVE_OPEN_HELPER", "1")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "relative open helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(mode(dir.path()), 0o755);
    assert_private_file(&dir.path().join("psyche.sqlite3"));
}

#[cfg(unix)]
#[test]
fn relative_filename_open_helper() {
    if std::env::var_os("PSYCHE_STORE_RELATIVE_OPEN_HELPER").is_none() {
        return;
    }

    drop(Store::open(Path::new("psyche.sqlite3")).unwrap());
}

#[cfg(unix)]
#[test]
fn symlink_parent_is_rejected_without_creating_a_database() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real_parent = dir.path().join("real");
    let linked_parent = dir.path().join("linked");
    std::fs::create_dir(&real_parent).unwrap();
    symlink(&real_parent, &linked_parent).unwrap();
    let path = linked_parent.join("psyche.sqlite3");

    assert_invalid_database_path(&path);

    assert!(!real_parent.join("psyche.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn symlink_database_is_rejected_without_mutating_its_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.sqlite3");
    let path = dir.path().join("linked.sqlite3");
    std::fs::write(&target, []).unwrap();
    symlink(&target, &path).unwrap();

    assert_invalid_database_path(&path);

    assert_eq!(std::fs::read(&target).unwrap(), Vec::<u8>::new());
}

fn assert_invalid_database_path(path: &Path) {
    let error = Store::open(path).unwrap_err();
    assert_eq!(error.to_string(), "store database path is invalid");
}

fn sqlite_sidecar_state(path: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    ["-journal", "-wal", "-shm"]
        .into_iter()
        .map(|suffix| {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            let sidecar = PathBuf::from(sidecar);
            let contents = match std::fs::read(sidecar) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("failed to read SQLite sidecar: {error}"),
            };
            (suffix.to_owned(), contents)
        })
        .collect()
}

#[cfg(unix)]
fn assert_private_directory(path: &Path) {
    assert_eq!(mode(path), 0o700);
}

#[cfg(not(unix))]
fn assert_private_directory(_path: &Path) {}

#[cfg(unix)]
fn assert_private_file(path: &Path) {
    assert_eq!(mode(path), 0o600);
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn assert_private_file(_path: &Path) {}
