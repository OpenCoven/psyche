use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};

use crate::StoreError;

pub(crate) fn open(path: &Path) -> Result<Connection, StoreError> {
    validate_path(path)?;
    prepare_parent_directory(path)?;
    prepare_database_file(path)?;
    let open_path = database_open_path(path)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    Ok(Connection::open_with_flags(open_path, flags)?)
}

pub(crate) fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA secure_delete = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )?;

    let foreign_keys =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, u32>(0))?;
    let journal_mode =
        connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?;
    let synchronous =
        connection.pragma_query_value(None, "synchronous", |row| row.get::<_, u32>(0))?;
    let secure_delete =
        connection.pragma_query_value(None, "secure_delete", |row| row.get::<_, u32>(0))?;
    let busy_timeout =
        connection.pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))?;

    if foreign_keys == 1
        && journal_mode == "wal"
        && synchronous == 2
        && secure_delete == 1
        && busy_timeout == 5_000
    {
        Ok(())
    } else {
        Err(StoreError::ConfigurationUnavailable)
    }
}

fn validate_path(path: &Path) -> Result<(), StoreError> {
    let is_special = path.as_os_str().is_empty()
        || path == Path::new(":memory:")
        || path.to_str().is_some_and(|path| path.starts_with("file:"));
    if is_special || path.file_name().is_none() {
        return Err(StoreError::InvalidDatabasePath);
    }
    Ok(())
}

fn prepare_parent_directory(path: &Path) -> Result<(), StoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if parent.has_root() && parent.parent().is_none() {
        return Err(StoreError::InvalidDatabasePath);
    }

    match fs::symlink_metadata(parent) {
        Ok(metadata) => validate_parent_metadata(&metadata)?,
        Err(error) if error.kind() == ErrorKind::NotFound => create_parent_directory(parent)?,
        Err(error) => return Err(StoreError::directory_operation(error)),
    }

    let metadata = fs::symlink_metadata(parent).map_err(StoreError::directory_operation)?;
    validate_parent_metadata(&metadata)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(StoreError::directory_operation)?;
        let metadata = fs::symlink_metadata(parent).map_err(StoreError::directory_operation)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o777 != 0o700
        {
            return Err(StoreError::InvalidDatabasePath);
        }
    }

    Ok(())
}

fn validate_parent_metadata(metadata: &fs::Metadata) -> Result<(), StoreError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StoreError::InvalidDatabasePath);
    }
    Ok(())
}

fn create_parent_directory(parent: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(parent)
            .map_err(StoreError::directory_operation)?;
    };

    #[cfg(not(unix))]
    fs::create_dir_all(parent).map_err(StoreError::directory_operation)?;

    Ok(())
}

fn prepare_database_file(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_database_metadata(&metadata)?,
        Err(error) if error.kind() == ErrorKind::NotFound => create_database_file(path)?,
        Err(error) => return Err(StoreError::file_operation(error)),
    }

    let metadata = fs::symlink_metadata(path).map_err(StoreError::file_operation)?;
    validate_database_metadata(&metadata)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(StoreError::file_operation)?;
        let metadata = fs::symlink_metadata(path).map_err(StoreError::file_operation)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(StoreError::InvalidDatabasePath);
        }
    }

    Ok(())
}

fn database_open_path(path: &Path) -> Result<PathBuf, StoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(StoreError::directory_operation)?;
    let file_name = path.file_name().ok_or(StoreError::InvalidDatabasePath)?;
    Ok(parent.join(file_name))
}

fn validate_database_metadata(metadata: &fs::Metadata) -> Result<(), StoreError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StoreError::InvalidDatabasePath);
    }
    Ok(())
}

fn create_database_file(path: &Path) -> Result<(), StoreError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }

    drop(options.open(path).map_err(StoreError::file_operation)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{configure, open};

    #[test]
    fn configure_sets_every_required_pragma() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private").join("psyche.sqlite3");

        let connection = open(&path).unwrap();
        configure(&connection).unwrap();

        assert_eq!(
            connection
                .pragma_query_value(None, "foreign_keys", |row| row.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "synchronous", |row| row.get::<_, u32>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "secure_delete", |row| row.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "busy_timeout", |row| row.get::<_, u32>(0))
                .unwrap(),
            5_000
        );
    }

    #[test]
    fn configure_rejects_pragma_fallback_with_a_stable_error() {
        let connection = Connection::open_in_memory().unwrap();

        let error = configure(&connection).unwrap_err();

        assert_eq!(
            error.to_string(),
            "required store database configuration is unavailable"
        );
    }
}
