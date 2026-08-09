//! Durable-store lifecycle integration tests.
#![allow(clippy::unwrap_used)]

use psyche_config::Config;
use psyche_runtime::{LifecycleState, Runtime, RuntimeError};
use psyche_store::{CURRENT_DATABASE_VERSION, Store, StoreError};

fn test_config(data_dir: &std::path::Path) -> Config {
    let Some(data_dir) = data_dir.to_str() else {
        panic!("test data directory must be valid UTF-8");
    };
    let data_dir = toml::Value::String(data_dir.to_owned()).to_string();
    psyche_config::load_str(&format!(
        r#"
schema_version = "psyche.config.v1"
data_dir = {data_dir}

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#
    ))
    .unwrap()
}

#[test]
fn test_config_preserves_a_windows_style_data_directory() {
    let data_dir = std::path::Path::new(r"C:\Users\Val\AppData\Local\Psyche");

    let config = test_config(data_dir);

    assert_eq!(config.data_dir, data_dir);
}

#[test]
fn test_config_preserves_a_del_containing_utf8_data_directory() {
    let data_dir = std::path::Path::new("/tmp/psyche-\u{007f}-store");

    let config = test_config(data_dir);

    assert_eq!(config.data_dir, data_dir);
}

#[cfg(unix)]
#[test]
#[should_panic(expected = "test data directory must be valid UTF-8")]
fn test_config_explicitly_rejects_a_non_utf8_data_directory() {
    use std::os::unix::ffi::OsStringExt;

    let data_dir = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![
        b'/', b't', b'm', b'p', b'/', 0xff,
    ]));

    let _ = test_config(&data_dir);
}

#[tokio::test]
async fn start_opens_the_configured_store_and_shutdown_leaves_schema_v1_reopenable() {
    let directory = tempfile::tempdir().unwrap();
    let data_dir = directory.path().join("private");
    let database = data_dir.join("psyche.sqlite3");

    let runtime = Runtime::start(test_config(&data_dir)).await.unwrap();
    assert_eq!(runtime.state(), LifecycleState::Running);
    assert!(database.is_file(), "{} was not opened", database.display());

    runtime.shutdown().await.unwrap();
    drop(runtime);

    let reopened = Store::open(&database).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), CURRENT_DATABASE_VERSION);
    assert_eq!(CURRENT_DATABASE_VERSION, 1);
}

#[tokio::test]
async fn future_database_version_fails_start_before_running_is_published() {
    let directory = tempfile::tempdir().unwrap();
    let data_dir = directory.path().join("private");
    let database = data_dir.join("psyche.sqlite3");
    drop(Store::open(&database).unwrap());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .pragma_update(None, "user_version", CURRENT_DATABASE_VERSION + 1)
        .unwrap();
    drop(connection);

    let result = Runtime::start(test_config(&data_dir)).await;
    assert!(matches!(
        result,
        Err(RuntimeError::Store(
            StoreError::UnsupportedDatabaseVersion { .. }
        ))
    ));
}

#[tokio::test]
async fn malformed_current_store_fails_start_before_running_is_published() {
    let directory = tempfile::tempdir().unwrap();
    let data_dir = directory.path().join("private");
    let database = data_dir.join("psyche.sqlite3");
    drop(Store::open(&database).unwrap());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "
            CREATE TRIGGER injected_runtime_trigger
            AFTER INSERT ON canonical_records
            BEGIN
              SELECT 1;
            END;
            ",
        )
        .unwrap();
    drop(connection);

    let result = Runtime::start(test_config(&data_dir)).await;
    assert!(matches!(
        result,
        Err(RuntimeError::Store(StoreError::DatabaseCorruption))
    ));
}
