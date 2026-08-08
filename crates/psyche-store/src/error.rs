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
    DirectoryOperation {
        /// Underlying filesystem error, retained without rendering its payload.
        #[source]
        source: std::io::Error,
    },
    /// Preparing the database file failed.
    #[error("store file operation failed")]
    FileOperation {
        /// Underlying filesystem error, retained without rendering its payload.
        #[source]
        source: std::io::Error,
    },
    /// Opening, configuring, or querying SQLite failed.
    #[error("store database operation failed")]
    DatabaseOperation {
        /// Underlying SQLite error, retained without rendering its payload.
        #[source]
        source: rusqlite::Error,
    },
}

impl fmt::Debug for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "StoreError({self})")
    }
}

impl StoreError {
    pub(crate) fn directory_operation(source: std::io::Error) -> Self {
        Self::DirectoryOperation { source }
    }

    pub(crate) fn file_operation(source: std::io::Error) -> Self {
        Self::FileOperation { source }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(source: rusqlite::Error) -> Self {
        Self::DatabaseOperation { source }
    }
}

#[cfg(test)]
mod tests {
    use super::StoreError;

    #[test]
    fn debug_redacts_source_payloads() {
        let marker = "sensitive-path-or-sql";
        let errors = [
            StoreError::directory_operation(std::io::Error::other(marker)),
            StoreError::file_operation(std::io::Error::other(marker)),
            StoreError::from(rusqlite::Error::InvalidParameterName(marker.to_owned())),
        ];

        for error in errors {
            assert!(!error.to_string().contains(marker));
            assert!(!format!("{error:?}").contains(marker));
        }
    }
}
