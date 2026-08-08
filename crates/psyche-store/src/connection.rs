use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, ErrorKind, Read},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::StoreError;

const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
const CONFIGURATION_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(50);
const CONFIGURATION_RETRY_DELAY: Duration = Duration::from_millis(10);
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const SQLITE_HEADER_SIZE: usize = 100;
const WAL_HEADER_SIZE: usize = 32;
const WAL_FRAME_HEADER_SIZE: usize = 24;
const WAL_FORMAT_VERSION: u32 = 3_007_000;
const WAL_MAGIC: u32 = 0x377f_0682;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DatabaseFileState {
    Existing,
    Created,
}

pub(crate) fn prepare(path: &Path) -> Result<(PathBuf, DatabaseFileState), StoreError> {
    validate_path(path)?;
    prepare_parent_directory(path)?;
    let state = prepare_database_file(path)?;
    let open_path = database_open_path(path)?;
    Ok((open_path, state))
}

pub(crate) fn open_read_only(path: &Path) -> Result<Connection, StoreError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    Ok(connection)
}

pub(crate) fn open_read_write(path: &Path) -> Result<Connection, StoreError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    Ok(connection)
}

pub(crate) fn file_user_version(path: &Path) -> Result<Option<u32>, StoreError> {
    // A read-only WAL query may update reader marks in `-shm`. Read committed
    // page-one frames first so a future schema can be rejected without that.
    let [journal_path, wal_path, _] = sqlite_sidecar_paths(path);
    if existing_file_len(&journal_path)?.is_some_and(|len| len > 0) {
        return Ok(None);
    }

    let Some((main_version, page_size)) = main_file_header(path)? else {
        return Ok(None);
    };
    if page_size == 0 {
        return Ok(Some(main_version));
    }

    match wal_file_user_version(&wal_path, page_size)? {
        WalFileVersion::Absent => Ok(Some(main_version)),
        WalFileVersion::Invalid => Ok(None),
        WalFileVersion::Valid(version) => Ok(Some(version.unwrap_or(main_version))),
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WalFileVersion {
    Absent,
    Invalid,
    Valid(Option<u32>),
}

fn existing_file_len(path: &Path) -> Result<Option<u64>, StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_database_metadata(&metadata)?;
            Ok(Some(metadata.len()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StoreError::file_operation(error)),
    }
}

fn main_file_header(path: &Path) -> Result<Option<(u32, u32)>, StoreError> {
    let mut file = File::open(path).map_err(StoreError::file_operation)?;
    let len = file.metadata().map_err(StoreError::file_operation)?.len();
    if len == 0 {
        return Ok(Some((0, 0)));
    }
    if len < SQLITE_HEADER_SIZE as u64 {
        return Ok(None);
    }

    let mut header = [0_u8; SQLITE_HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(StoreError::file_operation)?;
    if &header[..SQLITE_HEADER.len()] != SQLITE_HEADER {
        return Ok(None);
    }

    let encoded_page_size = u16::from_be_bytes([header[16], header[17]]);
    let page_size = if encoded_page_size == 1 {
        65_536
    } else {
        u32::from(encoded_page_size)
    };
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return Ok(None);
    }

    Ok(Some((read_u32_be(&header[60..64]), page_size)))
}

fn wal_file_user_version(
    path: &Path,
    expected_page_size: u32,
) -> Result<WalFileVersion, StoreError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(WalFileVersion::Absent);
        }
        Err(error) => return Err(StoreError::file_operation(error)),
    };
    if file.metadata().map_err(StoreError::file_operation)?.len() < WAL_HEADER_SIZE as u64 {
        return Ok(WalFileVersion::Absent);
    }

    let mut reader = BufReader::new(file);
    let mut header = [0_u8; WAL_HEADER_SIZE];
    reader
        .read_exact(&mut header)
        .map_err(StoreError::file_operation)?;
    let magic = read_u32_be(&header[..4]);
    let page_size = read_u32_be(&header[8..12]);
    if magic & !1 != WAL_MAGIC
        || read_u32_be(&header[4..8]) != WAL_FORMAT_VERSION
        || page_size != expected_page_size
    {
        return Ok(WalFileVersion::Invalid);
    }

    let checksum_big_endian = magic & 1 == 1;
    let mut checksum = [0_u32; 2];
    extend_wal_checksum(&header[..24], checksum_big_endian, &mut checksum);
    if checksum != [read_u32_be(&header[24..28]), read_u32_be(&header[28..])] {
        return Ok(WalFileVersion::Invalid);
    }

    let salt = &header[16..24];
    let mut frame_header = [0_u8; WAL_FRAME_HEADER_SIZE];
    let mut page = vec![0_u8; page_size as usize];
    let mut pending_page_one = None;
    let mut committed_page_one = None;
    loop {
        if !read_exact_frame_part(&mut reader, &mut frame_header)?
            || !read_exact_frame_part(&mut reader, &mut page)?
        {
            break;
        }
        if read_u32_be(&frame_header[..4]) == 0 || &frame_header[8..16] != salt {
            break;
        }

        let mut frame_checksum = checksum;
        extend_wal_checksum(&frame_header[..8], checksum_big_endian, &mut frame_checksum);
        extend_wal_checksum(&page, checksum_big_endian, &mut frame_checksum);
        if frame_checksum
            != [
                read_u32_be(&frame_header[16..20]),
                read_u32_be(&frame_header[20..]),
            ]
        {
            break;
        }
        checksum = frame_checksum;

        if read_u32_be(&frame_header[..4]) == 1 {
            pending_page_one = Some(read_u32_be(&page[60..64]));
        }
        if read_u32_be(&frame_header[4..8]) != 0 {
            if let Some(version) = pending_page_one.take() {
                committed_page_one = Some(version);
            }
        }
    }

    Ok(WalFileVersion::Valid(committed_page_one))
}

fn read_exact_frame_part(reader: &mut impl Read, buffer: &mut [u8]) -> Result<bool, StoreError> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(StoreError::file_operation(error)),
    }
}

fn extend_wal_checksum(bytes: &[u8], big_endian: bool, checksum: &mut [u32; 2]) {
    debug_assert_eq!(bytes.len() % 8, 0);
    for words in bytes.chunks_exact(8) {
        let first = read_checksum_word(&words[..4], big_endian);
        checksum[0] = checksum[0].wrapping_add(first).wrapping_add(checksum[1]);
        let second = read_checksum_word(&words[4..], big_endian);
        checksum[1] = checksum[1].wrapping_add(second).wrapping_add(checksum[0]);
    }
}

fn read_checksum_word(bytes: &[u8], big_endian: bool) -> u32 {
    let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

fn read_u32_be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o777 != 0o700 {
            return Err(StoreError::InvalidDatabasePath);
        }
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

fn prepare_database_file(path: &Path) -> Result<DatabaseFileState, StoreError> {
    let state = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_database_metadata(&metadata)?;
            DatabaseFileState::Existing
        }
        Err(error) if error.kind() == ErrorKind::NotFound => match create_database_file(path) {
            Ok(()) => DatabaseFileState::Created,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => DatabaseFileState::Existing,
            Err(error) => return Err(StoreError::file_operation(error)),
        },
        Err(error) => return Err(StoreError::file_operation(error)),
    };

    let metadata = fs::symlink_metadata(path).map_err(StoreError::file_operation)?;
    validate_database_metadata(&metadata)?;

    Ok(state)
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

    use super::{configure, open_read_write, prepare};

    #[test]
    fn configure_sets_every_required_pragma() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private").join("psyche.sqlite3");

        let (path, _) = prepare(&path).unwrap();
        let connection = open_read_write(&path).unwrap();
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
