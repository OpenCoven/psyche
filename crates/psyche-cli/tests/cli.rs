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
// The exit codes are asserted against the constants the binaries return, not
// against literals: a test carrying its own copy of `3` would keep passing
// through a renumbering that broke every unit file in the field.
use psyche_cli::{EXIT_CHECK_FAILED, EXIT_CONFIG, EXIT_UNAVAILABLE};

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

/// `status` emits no `state` at all, because it observed none.
///
/// The document used to say `{"state":"stopped","observed":false}`, which is a
/// false statement on any host where a daemon *is* running — and `jq -r .state`
/// is what people actually write. The caveat has to be structural: a field that
/// is absent cannot be read past.
///
/// Parsed rather than substring-matched, which also pins that stdout is a whole
/// valid JSON document: a log line leaking onto stdout would fail here.
#[test]
fn status_json_does_not_report_a_state_it_could_not_observe() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    let assert = Command::cargo_bin("psyche")
        .unwrap()
        .args(["status", "--config", config.to_str().unwrap(), "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let document: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    // Versioned, like `psyche.config.v1` and `coven.daemon.v1`. This repository
    // treats schema versioning as first-class everywhere except, until now, its
    // own machine-readable output.
    assert_eq!(
        document["schema"],
        serde_json::json!("psyche.status.v1"),
        "{stdout}"
    );
    assert_eq!(
        document.get("observed"),
        Some(&serde_json::json!(false)),
        "the answer must be marked as not observed: {stdout}"
    );
    // Present and null, not absent: a consumer distinguishing "no state" from
    // "no such field" should not have to.
    assert_eq!(
        document.get("state"),
        Some(&serde_json::Value::Null),
        "state must be null when nothing was observed: {stdout}"
    );
    assert_eq!(document["reason"], serde_json::json!("no-ipc"), "{stdout}");
}

/// The human rendering carries the same caveat, and likewise names no state.
/// Without this the two output modes disagree about how much the command knows.
#[test]
fn status_text_says_the_state_was_not_observed() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["status", "--config", config.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("not observed").and(contains("no-ipc")))
        // The old rendering led with `state: stopped`, which is the claim being
        // retracted. An operator skimming for a state word must not find one.
        .stdout(contains("stopped").not());
}

/// The reason lands in the report, on stdout, inside the `config` check — not as
/// one raw error on stderr with no checks run. `doctor` is dispatched before the
/// configuration is loaded precisely so it has something to say here.
#[test]
fn doctor_reports_a_bad_schema_version_as_a_failed_check() {
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
        .code(i32::from(EXIT_CONFIG))
        .stdout(
            contains("config: fail")
                .and(contains("unsupported schema_version"))
                .and(contains("psyche.config.v99"))
                // Every dependent check is reported as not run. A shorter list
                // would read as though they had passed.
                .and(contains("data_dir: skipped"))
                .and(contains("coven_socket_path: skipped"))
                .and(contains("extensions: skipped")),
        );
}

/// `psyche doctor --config /nope.toml` used to print one raw `ConfigError` and
/// exit with **zero checks run** — the single case the command most exists for
/// was the one case it refused to run in.
#[test]
fn doctor_still_runs_when_the_config_file_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope.toml");
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", missing.to_str().unwrap()])
        .assert()
        .code(i32::from(EXIT_CONFIG))
        .stdout(contains("config: fail").and(contains("nope.toml")))
        // Four lines, always. The check list is the contract; a load failure
        // shortens no part of it.
        .stdout(predicates::function::function(|out: &str| {
            out.lines().count() == 4
        }));
}

/// An unwritable `data_dir` is a failed *check*, not a bad configuration, and
/// the two get different codes on purpose — an operator scripting `doctor`
/// cannot otherwise tell "your file is malformed" from "your disk is not
/// writable".
#[cfg(unix)]
#[test]
fn doctor_exits_with_the_check_failed_code_on_an_unwritable_data_dir() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let blocked = tmp.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o500)).unwrap();

    // Root ignores mode bits. Determined by trying rather than by reading a uid,
    // which would need libc — forbidden here.
    if std::fs::write(blocked.join(".root-check"), b"").is_ok() {
        eprintln!(
            "skipping: this process writes through mode 0o500 (root, or a permissionless fs)"
        );
        return;
    }

    let path = tmp.path().join("psyche.toml");
    std::fs::write(
        &path,
        format!(
            r#"
schema_version = "psyche.config.v1"
data_dir = "{}"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
            blocked.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", path.to_str().unwrap()])
        .assert()
        .code(i32::from(EXIT_CHECK_FAILED))
        .stdout(contains("data_dir: fail"));

    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// `doctor --json` is versioned for the same reason the configuration and the
/// Coven API are: the alternative is the `name: status (detail)` line format
/// getting `grep`ped into a contract nobody chose to make.
///
/// Parsed rather than substring-matched, which also pins that stdout is one
/// whole JSON document — a log line leaking onto stdout would fail here.
#[test]
fn doctor_json_emits_a_versioned_document() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    let assert = Command::cargo_bin("psyche")
        .unwrap()
        .args(["doctor", "--config", config.to_str().unwrap(), "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let document: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert_eq!(
        document["schema"],
        serde_json::json!("psyche.doctor.v1"),
        "{stdout}"
    );
    assert_eq!(document["failed"], serde_json::json!(0), "{stdout}");
    let checks = document["checks"].as_array().unwrap();
    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(
        names,
        ["config", "data_dir", "coven_socket_path", "extensions"],
        "{stdout}"
    );
    // `coven_socket_path` contacts nothing and cannot fail. Reporting it as `ok`
    // in a list where any non-`ok` fails the command was a claim of
    // verification it never performed.
    assert_eq!(checks[2]["status"], serde_json::json!("info"), "{stdout}");
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

    // `stop` is asserted against its documented code, not `.success()`. It has
    // no daemon IPC to use, so "did nothing" is the truthful answer and 0 would
    // be the false one — see `stop_reports_that_it_cannot_reach_a_daemon`.
    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("PSYCHE_TELEGRAM_TOKEN")
        .args(["stop", "--config", path])
        .assert()
        .code(i32::from(EXIT_UNAVAILABLE));
}

/// `psyche stop` used to print `no running daemon to stop` and exit 0, while its
/// own help text promised it would "ask a running daemon to shut down
/// gracefully". Structurally the same defect as the `psyche start` stub: the
/// scripted failure is `psyche stop && deploy`, or a rolling restart that
/// believes the old daemon is gone.
#[test]
fn stop_reports_that_it_cannot_reach_a_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .args(["stop", "--config", config.to_str().unwrap()])
        .assert()
        .code(i32::from(EXIT_UNAVAILABLE))
        .stderr(contains("not implemented in this build"))
        // The caveat belongs on stderr so a `--json`-style stdout pipeline stays
        // clean, and so `stop > /dev/null` cannot hide it.
        .stdout(predicates::str::is_empty());
}

/// `--config` beats `$PSYCHE_CONFIG` beats `./psyche.toml`, on both binaries.
///
/// The default is relative to the working directory, and a systemd system unit
/// defaults to `WorkingDirectory=/` — so a `psyched.service` without an explicit
/// `--config` was resolving `/psyche.toml`. An environment variable is also how a
/// container image parameterises this, and there was none.
#[test]
fn config_resolves_from_the_flag_then_the_environment_then_the_default() {
    let tmp = tempfile::tempdir().unwrap();
    let chosen = write_config(tmp.path()).unwrap();
    // A path that would fail to load if it were ever preferred over `--config`.
    let decoy = tmp.path().join("decoy.toml");
    std::fs::write(&decoy, "schema_version = \"psyche.config.v99\"\n").unwrap();

    for binary in ["psyche", "psyched"] {
        // `$PSYCHE_CONFIG` alone.
        let mut command = Command::cargo_bin(binary).unwrap();
        if binary == "psyche" {
            command.arg("start");
        }
        command
            .env("PSYCHE_CONFIG", &chosen)
            .arg("--shutdown-after-start")
            .assert()
            .success();

        // `--config` wins over `$PSYCHE_CONFIG`; the decoy would exit 3.
        let mut command = Command::cargo_bin(binary).unwrap();
        if binary == "psyche" {
            command.arg("start");
        }
        command
            .env("PSYCHE_CONFIG", &decoy)
            .args([
                "--config",
                chosen.to_str().unwrap(),
                "--shutdown-after-start",
            ])
            .assert()
            .success();
    }
}

/// With neither the flag nor the variable set, the default is `./psyche.toml` —
/// and the error has to name the path that was actually tried, or an operator
/// whose service resolved `/psyche.toml` has nothing to go on.
#[test]
fn the_default_config_is_relative_to_the_working_directory() {
    let present = tempfile::tempdir().unwrap();
    write_config(present.path()).unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("PSYCHE_CONFIG")
        .current_dir(present.path())
        .arg("doctor")
        .assert()
        .success();

    // `doctor` puts the reason in its report on stdout; `status` short-circuits
    // on stderr. Both have to name the file they actually tried.
    let absent = tempfile::tempdir().unwrap();
    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("PSYCHE_CONFIG")
        .current_dir(absent.path())
        .arg("doctor")
        .assert()
        .code(i32::from(EXIT_CONFIG))
        .stdout(contains("psyche.toml"));

    Command::cargo_bin("psyche")
        .unwrap()
        .env_remove("PSYCHE_CONFIG")
        .current_dir(absent.path())
        .arg("status")
        .assert()
        .code(i32::from(EXIT_CONFIG))
        .stderr(contains("psyche.toml"));
}

/// `psyche --config X status` reads as the natural order and used to be a usage
/// error. A global argument accepts both placements, and the old form still
/// works — this asserts the pair, because "global" is only worth having if it is
/// backwards compatible.
#[test]
fn config_is_accepted_before_or_after_the_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let config = write_config(tmp.path()).unwrap();
    let path = config.to_str().unwrap();

    for args in [["--config", path, "status"], ["status", "--config", path]] {
        Command::cargo_bin("psyche")
            .unwrap()
            .args(args)
            .assert()
            .success();
    }
}

/// The resolution order is stated in `--help`. An operator writing a unit file
/// reads that, not this repository.
#[test]
fn help_states_the_config_resolution_order() {
    for binary in ["psyche", "psyched"] {
        Command::cargo_bin(binary)
            .unwrap()
            .arg("--help")
            .assert()
            .success()
            .stdout(contains("PSYCHE_CONFIG").and(contains("psyche.toml")));
    }
}

/// Every entry point owes the same answer for the same broken configuration.
///
/// Asserted per code rather than as "non-zero": the whole point of the space is
/// that an operator scripting these can tell "your configuration is malformed"
/// from "your environment is not in the state it needs to be", and `failure()`
/// cannot see the difference.
#[test]
fn a_broken_config_exits_with_the_configuration_code() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist.toml");
    let unsupported = tmp.path().join("unsupported.toml");
    std::fs::write(
        &unsupported,
        r#"
schema_version = "psyche.config.v99"
data_dir = "/tmp"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
    )
    .unwrap();
    let malformed = tmp.path().join("malformed.toml");
    std::fs::write(
        &malformed,
        "schema_version = \"psyche.config.v1\"\nthis is not toml",
    )
    .unwrap();

    let entry_points: [(&str, &[&str]); 5] = [
        ("psyche", &["doctor"]),
        ("psyche", &["status"]),
        ("psyche", &["start", "--shutdown-after-start"]),
        ("psyche", &["stop"]),
        ("psyched", &["--shutdown-after-start"]),
    ];

    for path in [&missing, &unsupported, &malformed] {
        for (binary, args) in entry_points {
            let mut command = Command::cargo_bin(binary).unwrap();
            command.args(args);
            command
                .args(["--config", path.to_str().unwrap()])
                .assert()
                .code(i32::from(EXIT_CONFIG));
        }
    }
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

/// The signal path, driven with a real signal.
///
/// Every other lifecycle test here uses `--shutdown-after-start`, which by
/// design never installs a handler — so the whole signal branch was unexercised,
/// and `psyched` shipped handling SIGINT only. `systemctl stop`, `docker stop`,
/// and a bare `kill` all send SIGTERM, so the drain was unreachable in exactly
/// the deployments that matter.
///
/// Both binaries and both signals, because `psyche start` and `psyched` are
/// supposed to be the same daemon, and an operator's `kill` is as much a
/// shutdown request as their Ctrl-C.
#[cfg(unix)]
mod signals {
    use std::io::{BufRead as _, BufReader};
    use std::process::{Command, ExitStatus, Stdio};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    /// Emitted by `daemon::run` once the runtime is up. Waiting for it is what
    /// keeps the signal from arriving before a handler could possibly exist —
    /// without it this test would be racing startup rather than testing drain.
    const READY: &str = "psyche daemon ready";

    /// Bounds the whole exchange. A daemon that ignores the signal keeps running
    /// forever, and an unbounded read of its stderr would wedge CI rather than
    /// fail it — which has already happened once in this project, for 600s.
    const LIMIT: Duration = Duration::from_secs(30);

    /// Runs `binary args...`, waits for the daemon to report ready, sends
    /// `signal`, and returns the exit status with everything written to stderr.
    ///
    /// Returns `io::Result` and is unwrapped by its caller: `clippy.toml` allows
    /// `unwrap` only in frames reachable from a `#[test]` fn, and a free helper
    /// here is not one.
    fn drain_under(
        binary: &str,
        args: &[&str],
        signal: &str,
    ) -> std::io::Result<(ExitStatus, String)> {
        let mut child = Command::new(binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::other("child stderr was not piped"))?;

        // Read on another thread and deliver through a channel, so every wait
        // below is a `recv_timeout` rather than a blocking read. The thread ends
        // when the child closes stderr, which disconnects the channel and is how
        // the post-signal collection below learns it is done.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });

        let mut log = String::new();
        let timed_out = |e: RecvTimeoutError| {
            std::io::Error::other(format!("gave up waiting on {binary} stderr: {e:?}"))
        };
        loop {
            let line = rx.recv_timeout(LIMIT).map_err(timed_out)?;
            let ready = line.contains(READY);
            log.push_str(&line);
            log.push('\n');
            if ready {
                break;
            }
        }

        // `/bin/kill` through `Command`, not `libc::kill`: `unsafe_code` is
        // forbidden at the workspace level and cannot be re-allowed, so there is
        // no in-process way to raise a signal at another pid.
        let status = Command::new("kill")
            .args([&format!("-{signal}"), &child.id().to_string()])
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "kill -{signal} failed: {status}"
            )));
        }

        // Drain to EOF. Disconnected means the child closed stderr, i.e. exited.
        loop {
            match rx.recv_timeout(LIMIT) {
                Ok(line) => {
                    log.push_str(&line);
                    log.push('\n');
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(e) => return Err(timed_out(e)),
            }
        }

        Ok((child.wait()?, log))
    }

    /// Asserts `log` shows a full graceful drain: `draining`, then `stopped`.
    ///
    /// Ordered, not merely present. A daemon that jumped straight to `stopped`
    /// skipped the drain, which is the whole property being bought here.
    fn assert_drained(label: &str, status: ExitStatus, log: &str) {
        assert!(
            status.success(),
            "{label}: expected a graceful exit, got {status}\n{log}"
        );
        let draining = log
            .find("\"state\":\"draining\"")
            .unwrap_or_else(|| panic!("{label}: never entered draining\n{log}"));
        let stopped = log
            .find("\"state\":\"stopped\"")
            .unwrap_or_else(|| panic!("{label}: never reached stopped\n{log}"));
        assert!(
            draining < stopped,
            "{label}: reached stopped without draining first\n{log}"
        );
    }

    #[test]
    fn both_binaries_drain_on_sigterm_and_sigint() {
        let tmp = tempfile::tempdir().unwrap();
        let config = super::write_config(tmp.path()).unwrap();
        let config = config.to_str().unwrap();

        let cases: [(&str, Vec<&str>, &str); 3] = [
            (
                env!("CARGO_BIN_EXE_psyched"),
                vec!["--config", config],
                "TERM",
            ),
            (
                env!("CARGO_BIN_EXE_psyched"),
                vec!["--config", config],
                "INT",
            ),
            (
                env!("CARGO_BIN_EXE_psyche"),
                vec!["start", "--config", config],
                "TERM",
            ),
        ];

        for (binary, args, signal) in cases {
            let label = format!("{binary} {} on SIG{signal}", args.join(" "));
            let (status, log) = drain_under(binary, &args, signal).unwrap();
            assert_drained(&label, status, &log);
        }
    }
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
    //
    // `info`, not `ok`: a tally verifies nothing, and it sat in a list where any
    // non-`ok` entry failed the whole command.
    assert!(stdout.contains("extensions: info"), "{stdout}");
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
