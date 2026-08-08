//! Durable SQLite substrate for Psyche contracts.

mod connection;
mod error;
mod execution_bindings;
mod migrations;
mod quarantine;
mod records;
mod retention;
mod transitions;

use std::{
    path::Path,
    sync::{Mutex, MutexGuard, OnceLock},
};

use rusqlite::TransactionBehavior;

pub use error::StoreError;
pub use migrations::CURRENT_DATABASE_VERSION;
pub use quarantine::{
    AuditEvent, QuarantineId, QuarantineReasonCode, QuarantineRecord, QuarantineResolution,
    QuarantineResolutionCode, ResolveQuarantineOutcome,
};
pub use records::IngestOutcome;
pub use retention::PruneReport;
pub use transitions::Transition;

/// A configured connection to Psyche's durable SQLite substrate.
#[derive(Debug)]
pub struct Store {
    connection: rusqlite::Connection,
}

static INITIALIZATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

impl Store {
    /// Opens a store and atomically applies every missing known migration.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let initialization_lock = INITIALIZATION_LOCK.get_or_init(|| Mutex::new(()));
        let _initialization_guard = initialization_guard(initialization_lock)?;
        let (mut connection, database_path) = connection::open(path)?;

        let found =
            match connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0)) {
                Ok(found) => found,
                Err(error) => {
                    connection::validate_sidecars(&database_path)?;
                    return Err(error.into());
                }
            };
        if found > CURRENT_DATABASE_VERSION {
            return Err(StoreError::UnsupportedDatabaseVersion { found });
        }

        connection::enforce_database_permissions(&database_path)?;
        connection::validate_sidecars(&database_path)?;
        connection::configure(&connection)?;
        connection::enforce_sidecar_permissions(&database_path)?;

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

fn initialization_guard(lock: &Mutex<()>) -> Result<MutexGuard<'_, ()>, StoreError> {
    lock.lock()
        .map_err(|_| StoreError::InitializationUnavailable)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::initialization_guard;

    #[test]
    fn poisoned_initialization_lock_returns_a_stable_error() {
        let lock = Arc::new(Mutex::new(()));
        let poisoner = Arc::clone(&lock);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison initialization lock");
        })
        .join();

        let error = initialization_guard(&lock).unwrap_err();

        assert_eq!(
            error.to_string(),
            "store database initialization is unavailable"
        );
        assert_eq!(
            format!("{error:?}"),
            "StoreError(store database initialization is unavailable)"
        );
    }
}
