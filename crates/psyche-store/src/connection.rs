use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::StoreError;

const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
const CONFIGURATION_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(50);
const CONFIGURATION_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(crate) fn open(path: &Path) -> Result<(Connection, PathBuf), StoreError> {
    validate_path(path)?;
    prepare_parent_directory(path)?;
    prepare_database_file(path)?;
    let open_path = database_open_path(path)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(&open_path, flags)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    Ok((connection, open_path))
}

pub(crate) fn enforce_database_permissions(path: &Path) -> Result<(), StoreError> {
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

pub(crate) fn validate_sidecars(path: &Path) -> Result<(), StoreError> {
    for sidecar in sqlite_sidecar_paths(path) {
        match fs::symlink_metadata(sidecar) {
            Ok(metadata) => validate_database_metadata(&metadata)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(StoreError::file_operation(error)),
        }
    }
    Ok(())
}

pub(crate) fn enforce_sidecar_permissions(path: &Path) -> Result<(), StoreError> {
    for sidecar in sqlite_sidecar_paths(path) {
        enforce_existing_sidecar_permissions(&sidecar)?;
    }
    Ok(())
}

pub(crate) fn configure(connection: &Connection) -> Result<(), StoreError> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    let mut last_contention: Option<rusqlite::Error> = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return match last_contention {
                Some(error) => Err(error.into()),
                None => Err(StoreError::ConfigurationUnavailable),
            };
        }
        connection.busy_timeout(remaining.min(CONFIGURATION_ATTEMPT_TIMEOUT))?;

        match configure_once(connection) {
            Ok(()) => break,
            Err(error) if is_lock_contention(&error) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(error.into());
                }
                last_contention = Some(error);
                thread::sleep(remaining.min(CONFIGURATION_RETRY_DELAY));
            }
            Err(error) => return Err(error.into()),
        }
    }
    connection.busy_timeout(BUSY_TIMEOUT)?;

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

fn configure_once(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA secure_delete = ON;
        ",
    )
}

fn is_lock_contention(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(sqlite_error, _)
            if matches!(
                sqlite_error.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            )
    )
}

fn sqlite_sidecar_paths(path: &Path) -> [PathBuf; 3] {
    ["-journal", "-wal", "-shm"].map(|suffix| {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        PathBuf::from(sidecar)
    })
}

fn enforce_existing_sidecar_permissions(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_database_metadata(&metadata)?,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(StoreError::file_operation(error)),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        match fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(StoreError::file_operation(error)),
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(StoreError::file_operation(error)),
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(StoreError::InvalidDatabasePath);
        }
    }

    Ok(())
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

    match fs::symlink_metadata(parent) {
        Ok(metadata) => validate_parent_metadata(&metadata)?,
        Err(error) if error.kind() == ErrorKind::NotFound => create_parent_directory(parent)?,
        Err(error) => return Err(StoreError::directory_operation(error)),
    }

    let metadata = fs::symlink_metadata(parent).map_err(StoreError::directory_operation)?;
    validate_parent_metadata(&metadata)?;

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
        Err(error) if error.kind() == ErrorKind::NotFound => match create_database_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StoreError::file_operation(error)),
        },
        Err(error) => return Err(StoreError::file_operation(error)),
    }

    let metadata = fs::symlink_metadata(path).map_err(StoreError::file_operation)?;
    validate_database_metadata(&metadata)?;

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

fn create_database_file(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }

    drop(options.open(path)?);
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

        let (connection, _) = open(&path).unwrap();
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
