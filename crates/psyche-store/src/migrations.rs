use rusqlite::Transaction;

use crate::StoreError;

/// Latest SQLite schema version understood by this build.
pub const CURRENT_DATABASE_VERSION: u32 = 1;

pub(super) fn apply_migration_sql(
    transaction: &Transaction<'_>,
    version: u32,
) -> Result<(), StoreError> {
    let sql = match version {
        1 => include_str!("../migrations/001_foundation.sql"),
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
