use std::path::{Path, PathBuf};

use rusqlite::Connection;

pub(super) const FOUNDATION_TABLES: [&str; 6] = [
    "audit_events",
    "canonical_records",
    "execution_binding_revisions",
    "quarantine_records",
    "schema_migrations",
    "transitions",
];

pub(super) enum Fixture {
    Version0,
    Version1,
    Version99,
    MigrationConflictV1,
}

pub(super) fn fixture_db(root: &Path, fixture: Fixture) -> PathBuf {
    let name = match fixture {
        Fixture::Version0 => "version-v0.sqlite3",
        Fixture::Version1 => "version-v1.sqlite3",
        Fixture::Version99 => "future-v99.sqlite3",
        Fixture::MigrationConflictV1 => "migration-conflict-v1.sqlite3",
    };
    let path = root.join(name);
    let connection = Connection::open(&path).unwrap();

    match fixture {
        Fixture::Version0 => connection
            .execute_batch(
                "
                CREATE TABLE fixture_v0_marker (
                  value TEXT NOT NULL
                ) STRICT;
                INSERT INTO fixture_v0_marker (value) VALUES ('preserve-me');
                PRAGMA user_version = 0;
                ",
            )
            .unwrap(),
        Fixture::Version1 => {
            connection.execute_batch(FOUNDATION_SCHEMA).unwrap();
            connection
                .execute(
                    "
                    INSERT INTO schema_migrations (version, applied_at)
                    VALUES (1, 'fixture-v1')
                    ",
                    [],
                )
                .unwrap();
            connection.pragma_update(None, "user_version", 1).unwrap();
        }
        Fixture::Version99 => connection
            .execute_batch(
                "
                CREATE TABLE future_owner (
                  value TEXT NOT NULL
                ) STRICT;
                INSERT INTO future_owner (value) VALUES ('future-owned');
                PRAGMA user_version = 99;
                ",
            )
            .unwrap(),
        Fixture::MigrationConflictV1 => connection
            .execute_batch(
                "
                CREATE TABLE canonical_records (
                  marker TEXT NOT NULL
                ) STRICT;
                INSERT INTO canonical_records (marker) VALUES ('preserve-conflict');
                PRAGMA user_version = 0;
                ",
            )
            .unwrap(),
    }

    drop(connection);
    path
}

pub(super) fn execute_batch(path: &Path, sql: &str) {
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

pub(super) fn user_version(path: &Path) -> u32 {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

pub(super) fn foundation_tables(path: &Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "
            SELECT name
            FROM sqlite_master
            WHERE type = 'table'
              AND name IN (
                'audit_events',
                'canonical_records',
                'execution_binding_revisions',
                'quarantine_records',
                'schema_migrations',
                'transitions'
              )
            ORDER BY name
            ",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn schema_migrations(path: &Path) -> Vec<(u32, String)> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT version, applied_at FROM schema_migrations ORDER BY version")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn table_exists(path: &Path, name: &str) -> bool {
    let connection = Connection::open(path).unwrap();
    connection
        .query_row(
            "
            SELECT EXISTS(
              SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
            )
            ",
            [name],
            |row| row.get(0),
        )
        .unwrap()
}

pub(super) fn table_columns(path: &Path, name: &str) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({name})"))
        .unwrap();
    statement
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

pub(super) fn scalar_text(path: &Path, sql: &str) -> String {
    let connection = Connection::open(path).unwrap();
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

pub(super) fn journal_mode(path: &Path) -> String {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap()
}

const FOUNDATION_SCHEMA: &str = r#"
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
) STRICT;
CREATE TABLE canonical_records (
  kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  digest TEXT NOT NULL,
  canonical_json BLOB NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (kind, record_id),
  UNIQUE (kind, record_id, digest)
) STRICT;
CREATE TABLE execution_binding_revisions (
  attempt_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  schema_version TEXT NOT NULL,
  digest TEXT NOT NULL,
  previous_revision_digest TEXT,
  canonical_json BLOB NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (attempt_id, revision),
  UNIQUE (attempt_id, digest),
  CHECK (
    (revision = 1 AND previous_revision_digest IS NULL)
    OR
    (revision > 1 AND previous_revision_digest IS NOT NULL)
  ),
  FOREIGN KEY (attempt_id, previous_revision_digest)
    REFERENCES execution_binding_revisions(attempt_id, digest)
) STRICT;
CREATE TABLE transitions (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  record_version INTEGER NOT NULL,
  transition_digest TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (kind, record_id, record_version)
) STRICT;
CREATE TABLE quarantine_records (
  quarantine_id TEXT PRIMARY KEY,
  schema_version TEXT,
  payload_digest TEXT NOT NULL,
  original_payload_len INTEGER NOT NULL CHECK (original_payload_len >= 0),
  retained_payload_digest TEXT NOT NULL,
  bounded_payload BLOB NOT NULL,
  reason TEXT NOT NULL,
  discovered_at TEXT NOT NULL,
  resolved_at TEXT,
  resolution_code TEXT,
  resolution_digest TEXT,
  CHECK (
    (resolved_at IS NULL AND resolution_code IS NULL AND resolution_digest IS NULL)
    OR
    (resolved_at IS NOT NULL AND resolution_code IS NOT NULL AND resolution_digest IS NOT NULL)
  )
) STRICT;
CREATE TABLE audit_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  event_code TEXT NOT NULL,
  correlation_id TEXT NOT NULL,
  public_details_json BLOB NOT NULL,
  created_at TEXT NOT NULL
) STRICT;
"#;
