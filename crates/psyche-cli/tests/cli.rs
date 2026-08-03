//! End-to-end checks over the two shipped binaries.
//!
//! These run the real executables, so they cover the whole output surface an
//! operator sees — including the two security properties this crate owns:
//! `doctor` never renders a `Config` with `{:?}`, and never prints an extension
//! value.

use assert_cmd::Command;
// `.and(..)` on a predicate comes from this trait, not from `contains` itself.
use predicates::prelude::PredicateBooleanExt as _;
use predicates::str::contains;

/// A 30-byte stand-in for a credential parked in an extension table. Long and
/// distinctive so a partial echo is still detectable by the window scan below.
const SECRETISH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn config_body(data_dir: &std::path::Path) -> String {
    format!(
        r#"
schema_version = "psyche.config.v1"
data_dir = "{}"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
        data_dir.display()
    )
}

// These return `io::Result` and are unwrapped by their callers rather than
// unwrapping internally: `clippy.toml` sets `allow-unwrap-in-tests`, but clippy
// recognises only frames reachable from a `#[test]`-annotated function, so an
// `unwrap` in a free helper here is a hard `-D clippy::unwrap-used` error. That
// is the right place for the panic anyway — it names the failing test.
fn write_config(dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    write_config_with(dir, "")
}

fn write_config_with(dir: &std::path::Path, extra: &str) -> std::io::Result<std::path::PathBuf> {
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let path = dir.join("psyche.toml");
    std::fs::write(&path, format!("{}{extra}", config_body(&data_dir)))?;
    Ok(path)
}

/// Asserts `haystack` carries no 8-byte window of `needle`.
///
/// Substring equality alone would miss a truncated or line-wrapped echo, which
/// is still a disclosure. Mirrors the window scan psyche-config uses.
fn assert_no_trace_of(haystack: &str, needle: &str, label: &str) {
    for window in needle.as_bytes().windows(8) {
        let fragment = String::from_utf8_lossy(window);
        assert!(
            !haystack.contains(fragment.as_ref()),
            "{label} echoed {fragment:?}: {haystack}"
        );
    }
}

#[test]
fn doctor_succeeds_without_any_telegram_credentials() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("PSYCHE_TELEGRAM_TOKEN")
        .args(["doctor", "--config", config.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("config: ok").and(contains("data_dir: ok")));
}

#[test]
fn status_reports_stopped_when_no_daemon_is_running() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["status", "--config", config.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(contains("\"state\":\"stopped\""));
}

/// `stopped` is not observed — there is no daemon IPC in this build — and the
/// document has to say so. A consumer that learns to trust a bare `state` field
/// and is told about the caveat in a later release has already shipped the code
/// that ignores it.
///
/// Parsed rather than substring-matched, which also pins that stdout is a whole
/// valid JSON document: a log line leaking onto stdout would fail here.
#[test]
fn status_json_marks_the_answer_as_unobserved() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    let assert = Command::cargo_bin("psyche")
        .unwrap()
        .args(["status", "--config", config.to_str().unwrap(), "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let document: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert_eq!(document["state"], serde_json::json!("stopped"), "{stdout}");
    assert_eq!(
        document.get("observed"),
        Some(&serde_json::json!(false)),
        "the answer must be marked as not observed: {stdout}"
    );
}

/// The human rendering carries the same caveat. Without it, the two output modes
/// disagree about how much the command actually knows.
#[test]
fn status_text_says_the_state_was_not_observed() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["status", "--config", config.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("stopped").and(contains("not observed")));
}

#[test]
fn doctor_fails_clearly_on_an_unsupported_schema_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("psyche.toml");
    std::fs::write(
        &path,
        r#"
schema_version = "psyche.config.v99"
data_dir = "/tmp"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
    )
    .unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("unsupported schema_version").and(contains("psyche.config.v99")));
}

#[test]
fn start_and_stop_run_without_any_telegram_credentials() {
    // coven-psy1 acceptance requires all four subcommands to run with no
    // credentials present, not just doctor and status.
    //
    // `start` is driven with `--shutdown-after-start` because it now actually
    // starts the daemon. The previous form of this test passed against a stub
    // that printed a line and exited 0, which is precisely the thing that made
    // the subcommand's own help text a lie.
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    let path = config.to_str().unwrap();

    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("PSYCHE_TELEGRAM_TOKEN")
        .args(["start", "--config", path, "--shutdown-after-start"])
        .assert()
        .success();

    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("PSYCHE_TELEGRAM_TOKEN")
        .args(["stop", "--config", path])
        .assert()
        .success();
}

/// `psyche start` says it starts the daemon in the foreground, so it must run
/// the same lifecycle `psyched` runs — not exit 0 having started nothing, which
/// is what `psyche start && systemctl ...` would have read as success.
///
/// Asserted against the transitions themselves rather than the exit code: a stub
/// that exits 0 is exactly what this is here to catch.
#[test]
fn psyche_start_runs_the_same_lifecycle_as_psyched() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    // Rebuilt per iteration: `AndPredicate` is not `Clone` because its `Item`
    // type parameter is `str`, which is unsized.
    let expected = || {
        contains("psyche runtime started")
            .and(contains("\"state\":\"draining\""))
            .and(contains("\"state\":\"stopped\""))
    };

    for binary in ["psyche", "psyched"] {
        let mut command = Command::cargo_bin(binary).unwrap();
        if binary == "psyche" {
            command.arg("start");
        }
        command
            .args([
                "--config",
                config.to_str().unwrap(),
                "--shutdown-after-start",
            ])
            .assert()
            .success()
            .stderr(expected());
    }
}

#[test]
fn psyched_start_then_stop_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyched")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "--shutdown-after-start",
        ])
        .assert()
        .success()
        .stderr(contains("psyche lifecycle transition"));
}

/// Extension tables are untyped, so a future one may hold a credential. `doctor`
/// may report how many there are; it may never report what is in them.
///
/// Shown to fail rather than assumed: with `doctor` temporarily printing
/// `config.extensions.get::<serde_json::Value>("psyche.experiment.v1")`, this
/// test goes red on the `looks_like_a_secret` assertion. Restoring the count-only
/// detail returns it to green.
#[test]
fn doctor_output_never_contains_an_extension_value() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config_with(
        tmp.path(),
        &format!(
            "\n[extensions.\"psyche.experiment.v1\"]\nlooks_like_a_secret = \"{SECRETISH}\"\n"
        ),
    )
    .unwrap();
    let assert = Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", config.to_str().unwrap()])
        .assert()
        .success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The count is reportable and is reported: without this, a `doctor` that
    // simply never mentions extensions would pass the absence checks below
    // while proving nothing about the redaction.
    assert!(stdout.contains("extensions: ok"), "{stdout}");
    assert!(stdout.contains("1 table(s) present"), "{stdout}");

    for (stream, label) in [(&stdout, "stdout"), (&stderr, "stderr")] {
        // The inner key is part of the value, unlike the versioned table name.
        assert!(
            !stream.contains("looks_like_a_secret"),
            "{label} printed an extension key: {stream}"
        );
        assert_no_trace_of(stream, SECRETISH, label);
    }
}

/// Rule one of this crate's output discipline: `doctor` prints fields it chose,
/// never a struct dump. `Config`'s `Debug` redacts today, but the guarantee must
/// not rest on a type this crate does not own.
///
/// Also shown to fail: adding `println!("{config:?}")` to `doctor` trips both
/// assertions below.
#[test]
fn doctor_output_is_not_a_config_debug_dump() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config_with(
        tmp.path(),
        "\n[extensions.\"psyche.experiment.v1\"]\nenabled = true\n",
    )
    .unwrap();
    let assert = Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", config.to_str().unwrap()])
        .assert()
        .success();
    let output = assert.get_output();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("Config {"), "{combined}");
    // `Extensions`' redacting Debug renders "<N key(s) redacted>"; seeing it
    // means something rendered the struct rather than named fields.
    assert!(!combined.contains("redacted"), "{combined}");
}
