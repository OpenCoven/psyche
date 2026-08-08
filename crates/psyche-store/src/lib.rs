//! Durable SQLite substrate for Psyche contracts.

mod connection;
mod error;
mod migrations;

use std::path::Path;

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
        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    /// Returns SQLite's current application schema version.
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        migrations::schema_version(&self.connection)
    }
}
