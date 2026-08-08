use std::fmt;

use psyche_core::contracts::{ContractError, SchemaKind};
use psyche_core::id::RecordId;

/// A stable, payload-free store failure.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The supplied path cannot safely name a persistent database.
    #[error("store database path is invalid")]
    InvalidDatabasePath,
    /// SQLite did not retain every required durability setting.
    #[error("required store database configuration is unavailable")]
    ConfigurationUnavailable,
    /// Another initialization attempt panicked while holding the process lock.
    #[error("store database initialization is unavailable")]
    InitializationUnavailable,
    /// The database's schema is newer than this build understands.
    #[error(
        "unsupported database version {found}; maximum supported version is {}",
        crate::CURRENT_DATABASE_VERSION
    )]
    UnsupportedDatabaseVersion {
        /// Version read from SQLite's `user_version`.
        found: u32,
    },
    /// The build has no SQL migration for a required version.
    #[error("database migration {version} is unavailable")]
    MigrationUnavailable {
        /// Missing migration version.
        version: u32,
    },
    /// A typed document failed its owned contract validation.
    #[error("record contract validation failed")]
    Contract(ContractError),
    /// A recognized document kind has no durable storage identity.
    #[error("document kind is not persistable")]
    NonPersistableKind {
        /// Recognized non-persistable schema kind.
        kind: SchemaKind,
    },
    /// A record identity already names different canonical content.
    #[error("record identity conflicts with stored canonical content")]
    RecordConflict {
        /// Schema kind of the conflicting record.
        kind: SchemaKind,
        /// Durable identity that was reused.
        record_id: RecordId,
    },
    /// An execution-binding revision would break its immutable linear history.
    #[error("execution binding revision conflicts with stored history")]
    ExecutionBindingRevisionConflict {
        /// Attempt whose revision history would fork.
        attempt_id: RecordId,
        /// Conflicting one-based revision.
        revision: u64,
    },
    /// A transition would break its record's immutable linear history.
    #[error("transition conflicts with stored history")]
    TransitionConflict {
        /// Schema kind of the transitioned record.
        kind: SchemaKind,
        /// Durable identity of the transitioned record.
        record_id: RecordId,
        /// Conflicting one-based record version.
        record_version: u64,
    },
    /// Persisted rows failed canonical or revision-chain integrity validation.
    #[error("stored database content failed integrity validation")]
    DatabaseCorruption,
    /// Creating the store's parent directory failed.
    #[error("store directory operation failed")]
    DirectoryOperation,
    /// Preparing the database file failed.
    #[error("store file operation failed")]
    FileOperation,
    /// Opening, configuring, or querying SQLite failed.
    #[error("store database operation failed")]
    DatabaseOperation,
}

impl fmt::Debug for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "StoreError({self})")
    }
}

impl StoreError {
    pub(crate) fn directory_operation(_source: std::io::Error) -> Self {
        Self::DirectoryOperation
    }

    pub(crate) fn file_operation(_source: std::io::Error) -> Self {
        Self::FileOperation
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(_source: rusqlite::Error) -> Self {
        Self::DatabaseOperation
    }
}

impl From<ContractError> for StoreError {
    fn from(source: ContractError) -> Self {
        Self::Contract(source)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use psyche_core::contracts::{ContractError, RecordKind, SchemaKind};
    use psyche_core::id::RecordId;

    use super::StoreError;

    #[test]
    fn display_debug_and_full_source_chain_redact_payloads() {
        let marker = "sensitive-path-or-sql";
        let errors = [
            (
                StoreError::directory_operation(std::io::Error::other(marker)),
                "store directory operation failed",
            ),
            (
                StoreError::file_operation(std::io::Error::other(marker)),
                "store file operation failed",
            ),
            (
                StoreError::from(rusqlite::Error::InvalidParameterName(marker.to_owned())),
                "store database operation failed",
            ),
        ];

        for (error, expected_display) in errors {
            assert_eq!(error.to_string(), expected_display);
            assert_eq!(
                format!("{error:?}"),
                format!("StoreError({expected_display})")
            );

            let mut current: &dyn Error = &error;
            loop {
                assert!(!current.to_string().contains(marker));
                assert!(!format!("{current:?}").contains(marker));
                let Some(source) = current.source() else {
                    break;
                };
                current = source;
            }
            assert!(error.source().is_none());
        }
    }

    #[test]
    fn record_failures_keep_identifiers_and_contract_details_out_of_rendering() {
        let id = RecordId::parse(RecordKind::Attempt, "att_01J00000000000000000000000").unwrap();
        let errors = [
            StoreError::Contract(ContractError::WrongRecordKind {
                schema: SchemaKind::ExecutionBinding,
                field: "record_id",
                expected: RecordKind::Attempt,
                found: RecordKind::Intent,
            }),
            StoreError::NonPersistableKind {
                kind: SchemaKind::Error,
            },
            StoreError::RecordConflict {
                kind: SchemaKind::ExecutionBinding,
                record_id: id.clone(),
            },
            StoreError::ExecutionBindingRevisionConflict {
                attempt_id: id.clone(),
                revision: 2,
            },
            StoreError::TransitionConflict {
                kind: SchemaKind::ExecutionBinding,
                record_id: id,
                record_version: 2,
            },
            StoreError::DatabaseCorruption,
        ];

        for error in errors {
            assert!(!error.to_string().contains("att_"));
            assert!(!format!("{error:?}").contains("att_"));
            assert!(error.source().is_none());
        }
    }
}
