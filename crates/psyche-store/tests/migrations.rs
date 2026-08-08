//! Forward-only SQLite foundation migration integration tests.
#![allow(clippy::unwrap_used)]

mod support;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use psyche_store::{CURRENT_DATABASE_VERSION, Store, StoreError};
use rusqlite::Connection;
use support::{
    FOUNDATION_TABLES, Fixture, execute_batch, fixture_db, foundation_tables, journal_mode,
    scalar_text, schema_migrations, table_columns, table_exists, user_version,
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
fn v1_quarantine_schema_contains_durable_integrity_metadata() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    set_mode(dir.path(), 0o700);
    let fresh_path = dir.path().join("fresh.sqlite3");
    let store = Store::open(&fresh_path).unwrap();
    drop(store);
    let fixture_path = fixture_db(dir.path(), Fixture::Version1);
    let expected = [
        "quarantine_id",
        "schema_version",
        "payload_digest",
        "original_payload_len",
        "retained_payload_digest",
        "integrity_digest",
        "bounded_payload",
        "reason",
        "discovered_at",
        "resolved_at",
        "resolution_code",
        "resolution_digest",
    ];

    assert_eq!(table_columns(&fresh_path, "quarantine_records"), expected);
    assert_eq!(table_columns(&fixture_path, "quarantine_records"), expected);
    assert!(
        Connection::open(&fresh_path)
            .unwrap()
            .execute(
                "
                INSERT INTO quarantine_records (
                    quarantine_id, schema_version, payload_digest, original_payload_len,
                    retained_payload_digest, integrity_digest, bounded_payload, reason,
                    discovered_at
                ) VALUES (
                    'qua_01J00000000000000000000000',
                    NULL,
                    'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    -1,
                    'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                    'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                    X'',
                    'unknown_schema',
                    '2026-08-08T00:00:00Z'
                )
                ",
                [],
            )
            .is_err()
    );
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

#[cfg(unix)]
#[test]
fn crash_left_wal_future_version_is_rejected_without_mutating_any_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version1);

    run_crash_helper(&path, "wal-v99");

    let sidecars = sqlite_sidecar_paths(&path);
    let wal_path = &sidecars[1];
    let shm_path = &sidecars[2];
    assert!(wal_path.exists());
    assert!(shm_path.exists());
    assert_eq!(database_header_user_version(&path), 1);

    for file in [&path, wal_path, shm_path] {
        set_mode(file, 0o644);
    }
    let before = [
        snapshot_file(&path),
        snapshot_file(wal_path),
        snapshot_file(shm_path),
    ];

    let error = Store::open(&path).unwrap_err();

    assert!(
        matches!(
            &error,
            StoreError::UnsupportedDatabaseVersion { found: 99, .. }
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(
        error.to_string(),
        "unsupported database version 99; maximum supported version is 1"
    );
    assert_snapshot_unchanged("database", &before[0], &snapshot_file(&path));
    assert_snapshot_unchanged("WAL", &before[1], &snapshot_file(wal_path));
    assert_snapshot_unchanged("shared memory", &before[2], &snapshot_file(shm_path));
}

#[cfg(unix)]
#[test]
fn hot_journal_read_only_failure_does_not_recover_or_open_read_write() {
    use std::error::Error;

    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version0);
    execute_batch(
        &path,
        "
        CREATE TABLE hot_journal_seed (
          id INTEGER PRIMARY KEY,
          payload BLOB NOT NULL
        ) STRICT;
        WITH RECURSIVE counter(value) AS (
          SELECT 1
          UNION ALL
          SELECT value + 1 FROM counter WHERE value < 256
        )
        INSERT INTO hot_journal_seed (id, payload)
        SELECT value, zeroblob(4096) FROM counter;
        ",
    );

    run_crash_helper(&path, "hot-journal");

    let sidecars = sqlite_sidecar_paths(&path);
    let journal_path = &sidecars[0];
    let journal_contents = std::fs::read(journal_path).unwrap();
    assert!(journal_contents.len() > 512);
    assert!(journal_contents[..8].iter().any(|byte| *byte != 0));
    set_mode(&path, 0o644);
    set_mode(journal_path, 0o644);
    let before = [snapshot_file(&path), snapshot_file(journal_path)];

    let error = Store::open(&path).unwrap_err();

    assert!(
        matches!(&error, StoreError::DatabaseOperation),
        "unexpected error: {error:?}"
    );
    assert_eq!(error.to_string(), "store database operation failed");
    assert_eq!(
        format!("{error:?}"),
        "StoreError(store database operation failed)"
    );
    assert!(error.source().is_none());
    assert_snapshot_unchanged("database", &before[0], &snapshot_file(&path));
    assert_snapshot_unchanged("rollback journal", &before[1], &snapshot_file(journal_path));
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
    const ROUNDS: usize = 8;
    const THREADS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    set_mode(dir.path(), 0o700);
    for round in 0..ROUNDS {
        let path = Arc::new(dir.path().join(format!("psyche-{round}.sqlite3")));
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
}

#[cfg(unix)]
#[test]
fn existing_wal_sidecars_are_made_private_before_open_completes() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version1);
    let setup_connection = Connection::open(&path).unwrap();
    setup_connection
        .execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA wal_autocheckpoint = 0;
            CREATE TABLE sidecar_marker (value TEXT NOT NULL) STRICT;
            INSERT INTO sidecar_marker (value) VALUES ('keep-wal-open');
            ",
        )
        .unwrap();

    set_mode(&path, 0o644);
    let sidecars = sqlite_sidecar_paths(&path);
    assert!(sidecars[1].exists());
    assert!(sidecars[2].exists());
    for sidecar in &sidecars {
        if sidecar.exists() {
            set_mode(sidecar, 0o644);
        }
    }

    let store = Store::open(&path).unwrap();

    assert_private_file(&path);
    for sidecar in &sidecars {
        if sidecar.exists() {
            assert_private_file(sidecar);
        }
    }
    drop(store);
    drop(setup_connection);
}

#[cfg(unix)]
#[test]
fn symlink_wal_sidecar_is_rejected_before_migration_writes() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version0);
    let target = dir.path().join("sidecar-target");
    std::fs::write(&target, b"do-not-touch").unwrap();
    let wal_path = sqlite_sidecar_paths(&path)[1].clone();
    symlink(&target, &wal_path).unwrap();

    assert_invalid_database_path(&path);

    assert_eq!(std::fs::read(&target).unwrap(), b"do-not-touch");
    std::fs::remove_file(wal_path).unwrap();
    assert_eq!(user_version(&path), 0);
    assert!(!table_exists(&path, "schema_migrations"));
}

#[cfg(unix)]
#[test]
fn non_regular_wal_sidecar_is_rejected_before_migration_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version0);
    let wal_path = sqlite_sidecar_paths(&path)[1].clone();
    std::fs::create_dir(&wal_path).unwrap();

    assert_invalid_database_path(&path);

    std::fs::remove_dir(wal_path).unwrap();
    assert_eq!(user_version(&path), 0);
    assert!(!table_exists(&path, "schema_migrations"));
}

#[cfg(unix)]
#[test]
fn future_database_sidecar_permissions_are_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture_db(dir.path(), Fixture::Version99);
    let setup_connection = Connection::open(&path).unwrap();
    setup_connection
        .execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA wal_autocheckpoint = 0;
            INSERT INTO future_owner (value) VALUES ('keep-sidecars-open');
            ",
        )
        .unwrap();

    set_mode(&path, 0o644);
    let sidecars = sqlite_sidecar_paths(&path);
    assert!(sidecars[1].exists());
    assert!(sidecars[2].exists());
    for sidecar in &sidecars {
        if sidecar.exists() {
            set_mode(sidecar, 0o644);
        }
    }

    let error = Store::open(&path).unwrap_err();

    assert!(matches!(
        error,
        StoreError::UnsupportedDatabaseVersion { found: 99, .. }
    ));
    assert_eq!(mode(&path), 0o644);
    for sidecar in &sidecars {
        if sidecar.exists() {
            assert_eq!(mode(sidecar), 0o644);
        }
    }
    drop(setup_connection);
}

#[cfg(unix)]
#[test]
fn existing_shared_parent_is_rejected_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("existing");
    let path = parent.join("psyche.sqlite3");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, b"not-a-database").unwrap();
    set_mode(&parent, 0o755);
    set_mode(&path, 0o755);

    assert_invalid_database_path(&path);

    assert_eq!(mode(&parent), 0o755);
    assert_eq!(mode(&path), 0o755);
    assert_eq!(std::fs::read(&path).unwrap(), b"not-a-database");
}

#[cfg(unix)]
#[test]
fn existing_private_parent_is_accepted_without_changing_its_mode() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("existing");
    let path = parent.join("psyche.sqlite3");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(&path, []).unwrap();
    set_mode(&parent, 0o700);
    set_mode(&path, 0o600);

    drop(Store::open(&path).unwrap());

    assert_eq!(mode(&parent), 0o700);
    assert_private_file(&path);
    assert_eq!(user_version(&path), CURRENT_DATABASE_VERSION);
}

#[cfg(unix)]
#[test]
fn relative_filename_rejects_shared_current_directory_without_changes() {
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
    assert!(!dir.path().join("psyche.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn relative_filename_open_helper() {
    if std::env::var_os("PSYCHE_STORE_RELATIVE_OPEN_HELPER").is_none() {
        return;
    }

    assert_invalid_database_path(Path::new("psyche.sqlite3"));
}

#[cfg(unix)]
#[test]
fn crash_left_database_helper() {
    let Some(helper) = std::env::var_os("PSYCHE_STORE_CRASH_HELPER") else {
        return;
    };
    let path = PathBuf::from(
        std::env::var_os("PSYCHE_STORE_CRASH_HELPER_PATH")
            .unwrap_or_else(|| panic!("crash helper database path is missing")),
    );
    let connection = Connection::open(&path).unwrap();

    match helper.to_str() {
        Some("wal-v99") => connection
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA wal_autocheckpoint = 0;
                PRAGMA synchronous = FULL;
                BEGIN IMMEDIATE;
                CREATE TABLE wal_future_marker (
                  value TEXT NOT NULL
                ) STRICT;
                INSERT INTO wal_future_marker (value) VALUES ('future-in-wal');
                PRAGMA user_version = 99;
                COMMIT;
                ",
            )
            .unwrap(),
        Some("hot-journal") => connection
            .execute_batch(
                "
                PRAGMA journal_mode = DELETE;
                PRAGMA synchronous = FULL;
                PRAGMA cache_size = 1;
                PRAGMA cache_spill = ON;
                BEGIN IMMEDIATE;
                UPDATE hot_journal_seed
                SET payload = randomblob(4096);
                ",
            )
            .unwrap(),
        _ => panic!("unknown crash helper mode"),
    }

    std::process::exit(0);
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
    set_mode(dir.path(), 0o700);
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

#[cfg(unix)]
fn run_crash_helper(path: &Path, helper: &str) {
    use std::process::Command;

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_left_database_helper", "--nocapture"])
        .env("PSYCHE_STORE_CRASH_HELPER", helper)
        .env("PSYCHE_STORE_CRASH_HELPER_PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "crash helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
struct FileSnapshot {
    contents: Vec<u8>,
    len: u64,
    modified: std::time::SystemTime,
    mode: u32,
}

#[cfg(unix)]
fn snapshot_file(path: &Path) -> FileSnapshot {
    let contents = std::fs::read(path).unwrap();
    let metadata = std::fs::metadata(path).unwrap();
    FileSnapshot {
        contents,
        len: metadata.len(),
        modified: metadata.modified().unwrap(),
        mode: mode(path),
    }
}

#[cfg(unix)]
fn assert_snapshot_unchanged(label: &str, before: &FileSnapshot, after: &FileSnapshot) {
    assert_eq!(after.len, before.len, "{label} length changed");
    assert_eq!(after.modified, before.modified, "{label} mtime changed");
    assert_eq!(after.mode, before.mode, "{label} mode changed");
    assert_eq!(after.contents, before.contents, "{label} contents changed");
}

#[cfg(unix)]
fn database_header_user_version(path: &Path) -> u32 {
    let contents = std::fs::read(path).unwrap();
    u32::from_be_bytes(contents[60..64].try_into().unwrap())
}

fn sqlite_sidecar_state(path: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    sqlite_sidecar_paths(path)
        .into_iter()
        .zip(["-journal", "-wal", "-shm"])
        .map(|(sidecar, suffix)| {
            let contents = match std::fs::read(sidecar) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("failed to read SQLite sidecar: {error}"),
            };
            (suffix.to_owned(), contents)
        })
        .collect()
}

fn sqlite_sidecar_paths(path: &Path) -> [PathBuf; 3] {
    ["-journal", "-wal", "-shm"].map(|suffix| {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        PathBuf::from(sidecar)
    })
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
