//! Durable SQLite substrate for Psyche contracts.

mod connection;
mod error;
mod migrations;

use std::path::Path;

use rusqlite::TransactionBehavior;

pub use error::StoreError;
pub use migrations::CURRENT_DATABASE_VERSION;

/// A configured connection to Psyche's durable SQLite substrate.
#[derive(Debug)]
pub struct Store {
    connection: rusqlite::Connection,
}

impl Store {
    /// Opens a store and atomically applies every missing known migration.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = connection::open(path)?;

        let found =
            connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?;
        if found > CURRENT_DATABASE_VERSION {
            return Err(StoreError::UnsupportedDatabaseVersion { found });
        }

        connection::configure(&connection)?;

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Exclusive)?;
        let found =
            transaction.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?;
        if found > CURRENT_DATABASE_VERSION {
            return Err(StoreError::UnsupportedDatabaseVersion { found });
        }

        if found < CURRENT_DATABASE_VERSION {
            for version in (found + 1)..=CURRENT_DATABASE_VERSION {
                migrations::apply_migration_sql(&transaction, version)?;
            }
            transaction.pragma_update(None, "user_version", CURRENT_DATABASE_VERSION)?;
        }
        transaction.commit()?;

        Ok(Self { connection })
    }

    /// Returns SQLite's current application schema version.
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }
}
