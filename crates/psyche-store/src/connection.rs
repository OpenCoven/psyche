use std::path::Path;

use rusqlite::Connection;

use crate::StoreError;

pub(crate) fn open(path: &Path) -> Result<Connection, StoreError> {
    create_parent_directory(path)?;
    let connection = Connection::open(path)?;
    configure(&connection)?;
    Ok(connection)
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
    Ok(())
}

fn create_parent_directory(path: &Path) -> Result<(), StoreError> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(parent)
            .map_err(StoreError::directory_operation)?;
    }

    #[cfg(not(unix))]
    std::fs::create_dir_all(parent).map_err(StoreError::directory_operation)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open;

    #[test]
    fn open_configures_every_required_pragma() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private").join("psyche.sqlite3");

        let connection = open(&path).unwrap();

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
}
