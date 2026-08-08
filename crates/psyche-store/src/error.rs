use std::fmt;

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

#[cfg(test)]
mod tests {
    use std::error::Error;

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
}
