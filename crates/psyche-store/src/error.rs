/// A stable, payload-free store failure.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
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
    /// Opening, configuring, or querying SQLite failed.
    #[error("store database operation failed")]
    DatabaseOperation {
        /// Underlying SQLite error, retained without rendering its payload.
        #[source]
        source: rusqlite::Error,
    },
}

impl StoreError {
    pub(crate) fn directory_operation(source: std::io::Error) -> Self {
        Self::DirectoryOperation { source }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(source: rusqlite::Error) -> Self {
        Self::DatabaseOperation { source }
    }
}
