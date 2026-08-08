use rusqlite::{OptionalExtension, Transaction};

use crate::StoreError;

/// Latest SQLite schema version understood by this build.
pub const CURRENT_DATABASE_VERSION: u32 = 1;

const FOUNDATION_TABLES: [&str; 6] = [
    "schema_migrations",
    "canonical_records",
    "execution_binding_revisions",
    "transitions",
    "quarantine_records",
    "audit_events",
];

const FOUNDATION_SQL: &str = include_str!("../migrations/001_foundation.sql");

pub(super) fn apply_migration_sql(
    transaction: &Transaction<'_>,
    version: u32,
) -> Result<(), StoreError> {
    let sql = match version {
        1 => FOUNDATION_SQL,
        _ => return Err(StoreError::MigrationUnavailable { version }),
    };

    transaction.execute_batch(sql)?;
    transaction.execute(
        "
        INSERT INTO schema_migrations (version, applied_at)
        VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        ",
        [version],
    )?;
    Ok(())
}

pub(super) fn validate_current_schema(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    match current_schema_matches(transaction) {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(StoreError::DatabaseCorruption),
    }
}

fn current_schema_matches(transaction: &Transaction<'_>) -> rusqlite::Result<bool> {
    let user_version =
        transaction.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?;
    if user_version != CURRENT_DATABASE_VERSION {
        return Ok(false);
    }

    for table in FOUNDATION_TABLES {
        let actual_sql = transaction
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1 AND tbl_name = ?1",
                [table],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let Some(actual_sql) = actual_sql else {
            return Ok(false);
        };
        let Some(expected_sql) = foundation_table_sql(table) else {
            return Ok(false);
        };
        if normalize_sql(&actual_sql) != normalize_sql(expected_sql) {
            return Ok(false);
        }

        let mut index_statement = transaction.prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'index' AND tbl_name = ?1 ORDER BY name",
        )?;
        let indexes = index_statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if indexes != foundation_indexes(table) {
            return Ok(false);
        }
    }

    let mut statement =
        transaction.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let versions = statement
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(versions == [CURRENT_DATABASE_VERSION])
}

fn foundation_indexes(table: &str) -> &'static [&'static str] {
    match table {
        "canonical_records" => &[
            "sqlite_autoindex_canonical_records_1",
            "sqlite_autoindex_canonical_records_2",
        ],
        "execution_binding_revisions" => &[
            "sqlite_autoindex_execution_binding_revisions_1",
            "sqlite_autoindex_execution_binding_revisions_2",
        ],
        "transitions" => &["sqlite_autoindex_transitions_1"],
        "quarantine_records" => &["sqlite_autoindex_quarantine_records_1"],
        _ => &[],
    }
}

fn foundation_table_sql(table: &str) -> Option<&'static str> {
    let prefix = format!("CREATE TABLE {table} ");
    FOUNDATION_SQL
        .split(';')
        .map(str::trim)
        .find(|statement| statement.starts_with(&prefix))
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}
