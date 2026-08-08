use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::StoreError;

/// Latest SQLite schema version understood by this build.
pub const CURRENT_DATABASE_VERSION: u32 = 1;

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
    let found = schema_version(&transaction)?;

    if found > CURRENT_DATABASE_VERSION {
        return Err(StoreError::UnsupportedDatabaseVersion { found });
    }

    if found < CURRENT_DATABASE_VERSION {
        for version in (found + 1)..=CURRENT_DATABASE_VERSION {
            apply_migration_sql(&transaction, version)?;
        }
        transaction.pragma_update(None, "user_version", CURRENT_DATABASE_VERSION)?;
    }

    transaction.commit()?;
    Ok(())
}

pub(crate) fn apply_migration_sql(
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

pub(crate) fn schema_version(connection: &Connection) -> Result<u32, StoreError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}
