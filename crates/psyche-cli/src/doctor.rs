//! Environment checks that must pass with no Telegram credentials present.
//!
//! Every string built here is printed verbatim to an operator's terminal and
//! routinely pasted into a bug report, so this module is the crate's output
//! surface. Two rules hold for everything it produces:
//!
//! 1. A [`Config`] is never rendered with `{:?}`. `Config`'s `Debug` redacts its
//!    extension table, so a dump would be safe today — but `doctor` names the
//!    fields it chose to print, so the guarantee does not depend on the `Debug`
//!    impl of a type this crate does not own.
//! 2. An extension value is never printed. Extension tables are untyped and a
//!    future one may hold a credential, so only the count is reported.
//!
//! Both are pinned by tests in `tests/cli.rs`, each shown to fail against a
//! `doctor` that violates it.
//!
//! A third rule joins them, and it is the reason this module takes a
//! `Result<&Config, &ConfigError>` rather than a `&Config`: **a check reports
//! what it observed, never what it assumes.** `doctor` used to call
//! `create_dir_all` and print "writable" on `Ok(())` — which that function
//! returns for a directory that already exists at mode 500 — so the one word an
//! operator ran the command to see was never verified. It also could not run at
//! all against a configuration that failed to load, which is the single case it
//! most exists for.

use std::path::Path;

use psyche_config::{Config, ConfigError};

/// What a check found.
///
/// Replaces a `bool`. Two of the entries below cannot fail — they report a path
/// and a table count — and living in a list where any `false` failed the command
/// made them look like assertions they were not. Splitting the outcomes also
/// gives the loader failure somewhere honest to put the checks it prevented from
/// running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Checked, and the thing checked is in the state it needs to be.
    Ok,
    /// Checked, usable, and worth an operator's attention anyway.
    Warn,
    /// Checked, and wrong. Any single one of these fails the command.
    Fail,
    /// Not a check. Reported because an operator reading a bug report wants it,
    /// and it can never fail.
    Info,
    /// Not run, because something it needed was unavailable.
    Skipped,
}

/// The wire spelling of a status: `ok`, `warn`, `fail`, `info`, `skipped`.
///
/// One spelling, used by both the text and the JSON rendering. The text form
/// previously shouted `FAIL` while the machine-readable form did not exist yet;
/// giving each its own vocabulary is how the two drift, and an operator who
/// greps for what they saw on their terminal then finds nothing in the JSON.
/// Visual salience is not worth a second definition of the same word.
impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Info => "info",
            Status::Skipped => "skipped",
        })
    }
}

/// One named check and the single line it contributes to `doctor` output.
#[derive(Debug)]
pub struct Check {
    /// Stable identifier an operator or a script can grep for.
    pub name: &'static str,
    /// What the check found. Any [`Status::Fail`] fails the command.
    pub status: Status,
    /// Operator-facing explanation. Built from named config fields only.
    pub detail: String,
}

/// Schema identifier on `doctor --json` output.
///
/// This repository versions its configuration (`psyche.config.v1`) and the Coven
/// API (`coven.daemon.v1`); its own machine-readable output is owed the same. The
/// alternative is the ad-hoc `name: status (detail)` line format, which someone
/// would `grep`, and which would then be frozen without anyone deciding to
/// freeze it.
pub const DOCTOR_SCHEMA: &str = "psyche.doctor.v1";

/// Exclusively creates and removes a temporary file inside `dir`, creating
/// `dir` if it is absent.
///
/// Returns whether `dir` already existed. `create_dir_all` alone proves nothing:
/// it returns `Ok(())` for a directory that exists at any mode, so a mode-500
/// `data_dir` reported "writable" and exited 0. The only way to learn that a
/// directory is writable is to write to it.
///
/// The distinction between created and pre-existing matters just as much: on a
/// typo'd path the old code silently *created* the directory and blessed it,
/// hiding the exact misconfiguration `doctor` exists to surface.
fn probe(dir: &Path) -> Result<bool, String> {
    let existed = psyche_store::prepare_data_dir(dir).map_err(|error| error.to_string())?;
    // `tempfile` uses exclusive creation with randomized names and bounded
    // collision retries, so this never opens or truncates a pre-existing
    // entry. Closing removes that randomized path, and reports cleanup failure.
    let probe = tempfile::Builder::new()
        .prefix(".psyche-doctor-probe-")
        .tempfile_in(dir)
        .map_err(|error| error.to_string())?;
    probe.close().map_err(|error| error.to_string())?;
    Ok(existed)
}

/// Every check here is local and credential-free. Reaching the Coven socket or
/// a Telegram API is explicitly *not* done — those belong to later gates.
///
/// Takes the load *result*, not a `Config`. `doctor` is dispatched before the
/// configuration is loaded precisely so it can report on a configuration that
/// does not load: the caller used to short-circuit on a bad file and print one
/// raw error with zero checks run, which made the `config` check below vacuous —
/// it could only ever report `ok`, because an invalid configuration never
/// reached it.
///
/// `path` is passed alongside because a `ConfigError` may or may not carry one,
/// and the operator's question is always "which file did you read".
pub fn run(path: &Path, config: Result<&Config, &ConfigError>) -> Vec<Check> {
    let config = match config {
        Ok(config) => config,
        Err(e) => return skipped_because_of(path, e),
    };

    let mut checks = vec![Check {
        name: "config",
        status: Status::Ok,
        // `schema_version()` is a method returning a `&'static str` const, not a
        // value read back from the file: a validated `Config` cannot hold any
        // other version, so this reports the build's contract, not user input.
        detail: format!("{} is schema {}", path.display(), config.schema_version()),
    }];

    let data_dir: &Path = config.data_dir.as_path();
    let (status, detail) = match probe(data_dir) {
        Ok(true) => (
            Status::Ok,
            format!("{} exists and is writable", data_dir.display()),
        ),
        // Created, and said so. `doctor` performing a side effect is defensible
        // — a first run on a fresh host should not fail for a directory it can
        // make — but doing it silently turns a typo'd path into a green line.
        Ok(false) => (
            Status::Warn,
            format!("{} created (did not exist)", data_dir.display()),
        ),
        Err(e) => (Status::Fail, format!("{}: {e}", data_dir.display())),
    };
    checks.push(Check {
        name: "data_dir",
        status,
        detail,
    });

    checks.push(Check {
        name: "coven_socket_path",
        // Info, not Ok: nothing was contacted, so there is nothing this could
        // have found. It was hardcoded `ok: true` in a list where any `!ok`
        // failed the command, which is a claim of verification it never did.
        status: Status::Info,
        detail: format!(
            "{} (not contacted at this gate)",
            config.coven.socket.display()
        ),
    });

    // Count only, and deliberately not `Extensions`' `Debug` — that renders
    // "<N key(s) redacted>", which reads to an operator as though something was
    // withheld from them rather than as a plain tally. The table names would
    // also be safe to print (they are operator-authored structure, which is why
    // psyche-config names them in its errors), but `Extensions` exposes no
    // iterator over its keys and this crate does not reach into another to add
    // one. Values are never in scope here under any API.
    checks.push(Check {
        name: "extensions",
        // Info for the same reason as the socket path: a tally is not a verdict.
        status: Status::Info,
        detail: format!(
            "{} table(s) present; contents not read",
            config.extensions.len()
        ),
    });

    checks
}

/// The report for a configuration that would not load: one real failure, and
/// every dependent check marked as not run.
///
/// Skipped rather than omitted. A shorter list would read as though those checks
/// had passed, and the operator would not learn that `doctor` still has nothing
/// to say about their `data_dir`.
fn skipped_because_of(path: &Path, error: &ConfigError) -> Vec<Check> {
    // `Display`, never `{:?}`: `ConfigError` reduces every deserializer error to
    // a payload-free message at one place inside psyche-config and holds no
    // `toml::de::Error`, whose own `Debug` would carry the entire configuration
    // file, secrets included. This rule is load-bearing across the project.
    let mut checks = vec![Check {
        name: "config",
        status: Status::Fail,
        detail: format!("{}: {error}", path.display()),
    }];
    for name in ["data_dir", "coven_socket_path", "extensions"] {
        checks.push(Check {
            name,
            status: Status::Skipped,
            detail: "configuration did not load".to_owned(),
        });
    }
    checks
}

/// How many checks failed. Anything above zero fails the command.
///
/// [`Status::Warn`] deliberately does not count: a `data_dir` this run created
/// is worth saying out loud and is not a reason for a health probe to go red.
#[must_use]
pub fn failures(checks: &[Check]) -> usize {
    checks.iter().filter(|c| c.status == Status::Fail).count()
}

/// The human rendering: one `name: status (detail)` line per check.
///
/// Returns a `String` rather than printing, so it can be asserted without
/// spawning a process.
#[must_use]
pub fn render_text(checks: &[Check]) -> String {
    let mut out = String::new();
    for check in checks {
        out.push_str(&format!(
            "{}: {} ({})\n",
            check.name, check.status, check.detail
        ));
    }
    out
}

/// The machine rendering: a versioned `psyche.doctor.v1` document.
///
/// `failed` is carried in the document as well as in the exit code, so a
/// consumer that already has the JSON does not have to also thread the status
/// through its shell.
#[must_use]
pub fn render_json(checks: &[Check]) -> String {
    let checks: Vec<serde_json::Value> = checks
        .iter()
        .map(|check| {
            serde_json::json!({
                "name": check.name,
                "status": check.status.to_string(),
                "detail": check.detail,
            })
        })
        .collect();
    serde_json::json!({
        "schema": DOCTOR_SCHEMA,
        "checks": checks,
        "failed": failures_in(&checks),
    })
    .to_string()
}

/// `failed` over already-rendered values, so [`render_json`] does not have to
/// hold both forms at once.
fn failures_in(checks: &[serde_json::Value]) -> usize {
    checks
        .iter()
        .filter(|c| c["status"] == serde_json::json!(Status::Fail.to_string()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the `Result` rather than unwrapping: `clippy.toml` allows
    /// `unwrap` only in frames reachable from a `#[test]` fn, and a free helper
    /// is not one. The caller unwrapping also names the failing test.
    /// Renders a path as a TOML basic string, escaping what TOML gives meaning to.
    ///
    /// A Windows temp directory is `C:\Users\RUNNER~1\AppData\Local\Temp\...`,
    /// and interpolating that raw makes `\U` a unicode escape: the loader fails
    /// with "too few unicode value digits" and the test reads as though the
    /// configuration loader were broken. Nothing platform-specific here — the
    /// escaping is simply what writing a path into TOML has always required.
    fn toml_str(path: &Path) -> String {
        format!(
            "\"{}\"",
            path.display()
                .to_string()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        )
    }

    fn config_for(data_dir: &Path) -> Result<Config, ConfigError> {
        psyche_config::load_str(&format!(
            r#"
schema_version = "psyche.config.v1"
data_dir = {}

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
            toml_str(data_dir)
        ))
    }

    fn prepared_data_dir(root: &Path) -> Result<std::path::PathBuf, psyche_store::StoreError> {
        let data_dir = root.join("data");
        psyche_store::prepare_data_dir(&data_dir)?;
        Ok(data_dir)
    }

    fn check<'a>(checks: &'a [Check], name: &str) -> Option<&'a Check> {
        checks.iter().find(|c| c.name == name)
    }

    /// A Windows path survives being written into a TOML fixture.
    ///
    /// Runs on every platform on purpose: the bug this pins is not conditional
    /// code, it is a string that means something different to the TOML parser,
    /// so a literal Windows path exercises it from macOS just as well. Without
    /// this, five `doctor` tests passed everywhere developers work and failed on
    /// `windows-latest` with "too few unicode value digits" — an error naming the
    /// config loader, which is not where the defect was.
    #[test]
    fn a_windows_style_path_round_trips_through_a_toml_fixture() {
        let windows = Path::new(r"C:\Users\RUNNER~1\AppData\Local\Temp\psyche");
        let rendered = toml_str(windows);
        assert_eq!(
            rendered, r#""C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\psyche""#,
            "every backslash must be escaped or TOML reads \\U as a unicode escape"
        );

        let config = psyche_config::load_str(&format!(
            r#"
schema_version = "psyche.config.v1"
data_dir = {rendered}

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#
        ))
        .expect("a path with backslashes must survive the fixture");
        assert_eq!(config.data_dir, windows);
    }

    /// The whole of `doctor`'s coverage used to be process spawns, because these
    /// modules were reachable only through two binary crate roots. Calling `run`
    /// directly is what the library target bought.
    #[test]
    fn every_check_is_named_and_explained() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = prepared_data_dir(tmp.path()).unwrap();
        let config = config_for(&data_dir).unwrap();
        let checks = run(Path::new("psyche.toml"), Ok(&config));

        let names: Vec<&str> = checks.iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            ["config", "data_dir", "coven_socket_path", "extensions"],
            "the check list is the command's contract with a scripting operator"
        );
        for check in &checks {
            assert!(!check.detail.is_empty(), "{} has no detail", check.name);
        }
        assert_eq!(failures(&checks), 0);
    }

    /// An existing directory is reported as existing, and the word "writable" is
    /// earned by a write.
    #[test]
    fn an_existing_writable_data_dir_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let prepared = prepared_data_dir(tmp.path()).unwrap();
        let config = config_for(&prepared).unwrap();
        let checks = run(Path::new("psyche.toml"), Ok(&config));
        let data_dir = check(&checks, "data_dir").unwrap();

        assert_eq!(data_dir.status, Status::Ok, "{}", data_dir.detail);
        assert!(
            data_dir.detail.contains("exists and is writable"),
            "{}",
            data_dir.detail
        );
        // The probe cleans up after itself; a leftover file in an operator's
        // data directory is litter, and one that persisted would also make the
        // "did not exist" branch below unreachable on a second run.
        assert!(std::fs::read_dir(prepared).unwrap().next().is_none());
    }

    /// A typo'd path used to be silently created and blessed with a green line.
    /// It is still created — failing a fresh host for a directory `doctor` can
    /// make would be worse — but the report says so.
    #[test]
    fn a_data_dir_that_did_not_exist_is_reported_as_created() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("typo").join("psyche");
        let config = config_for(&missing).unwrap();
        let checks = run(Path::new("psyche.toml"), Ok(&config));
        let data_dir = check(&checks, "data_dir").unwrap();

        assert_eq!(data_dir.status, Status::Warn, "{}", data_dir.detail);
        assert!(
            data_dir.detail.contains("did not exist"),
            "{}",
            data_dir.detail
        );
        // A warning is not a failure: a first run on a clean host must not exit
        // non-zero for a directory it successfully prepared.
        assert_eq!(failures(&checks), 0);
    }

    #[test]
    fn probe_does_not_touch_a_preexisting_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = prepared_data_dir(tmp.path()).unwrap();
        let existing_probe = data_dir.join(".psyche-doctor-probe");
        std::fs::write(&existing_probe, b"operator-owned").unwrap();
        let config = config_for(&data_dir).unwrap();

        let checks = run(Path::new("psyche.toml"), Ok(&config));

        assert_eq!(check(&checks, "data_dir").unwrap().status, Status::Ok);
        assert_eq!(std::fs::read(existing_probe).unwrap(), b"operator-owned");
        assert_eq!(std::fs::read_dir(data_dir).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn probe_does_not_follow_or_remove_a_preexisting_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let data_dir = prepared_data_dir(tmp.path()).unwrap();
        let target = tmp.path().join("operator-owned");
        std::fs::write(&target, b"preserve").unwrap();
        let existing_probe = data_dir.join(".psyche-doctor-probe");
        symlink(&target, &existing_probe).unwrap();
        let config = config_for(&data_dir).unwrap();

        let checks = run(Path::new("psyche.toml"), Ok(&config));

        assert_eq!(check(&checks, "data_dir").unwrap().status, Status::Ok);
        assert!(
            std::fs::symlink_metadata(&existing_probe)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
        assert_eq!(std::fs::read_dir(data_dir).unwrap().count(), 1);
    }

    /// Mode 500: readable and traversable, not writable. `create_dir_all`
    /// returns `Ok(())` for it, which is exactly how "writable" came to be
    /// printed about a directory that is not.
    #[cfg(unix)]
    #[test]
    fn an_unwritable_data_dir_fails() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let blocked = tmp.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o500)).unwrap();

        // Determined by trying, not by asking for a uid: reading the process uid
        // needs libc, and `unsafe_code` is forbidden workspace-wide. Root
        // ignores mode bits, so without this the test would pass by reporting
        // the opposite of what it claims to check.
        if std::fs::write(blocked.join(".root-check"), b"").is_ok() {
            eprintln!(
                "skipping an_unwritable_data_dir_fails: this process writes through mode 0o500, \
                 so it is root or on a filesystem that ignores mode bits"
            );
            return;
        }

        let config = config_for(&blocked).unwrap();
        let checks = run(Path::new("psyche.toml"), Ok(&config));
        let data_dir = check(&checks, "data_dir").unwrap();

        assert_eq!(data_dir.status, Status::Fail, "{}", data_dir.detail);
        assert!(data_dir.detail.contains(&blocked.display().to_string()));
        assert_eq!(failures(&checks), 1);

        // Restore, or the tempdir cannot be removed on drop.
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// The case `doctor` most exists for. It used to be the case `doctor`
    /// refused to run in.
    #[test]
    fn a_config_that_will_not_load_fails_that_check_and_skips_the_rest() {
        let error =
            psyche_config::load_str("schema_version = \"psyche.config.v99\"\n").unwrap_err();
        let checks = run(Path::new("/etc/psyche/psyche.toml"), Err(&error));

        let config = check(&checks, "config").unwrap();
        assert_eq!(config.status, Status::Fail);
        assert!(
            config.detail.contains("/etc/psyche/psyche.toml"),
            "{}",
            config.detail
        );
        assert!(
            config.detail.contains("psyche.config.v99"),
            "{}",
            config.detail
        );
        assert_eq!(failures(&checks), 1);

        // Skipped, not omitted: a shorter list reads as though these passed.
        for name in ["data_dir", "coven_socket_path", "extensions"] {
            assert_eq!(
                check(&checks, name).unwrap().status,
                Status::Skipped,
                "{name}"
            );
        }
    }

    /// The checks that cannot fail must not sit in the list looking like
    /// assertions. `coven_socket_path` was hardcoded `ok: true`.
    #[test]
    fn checks_that_verify_nothing_are_marked_info() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = prepared_data_dir(tmp.path()).unwrap();
        let config = config_for(&data_dir).unwrap();
        let checks = run(Path::new("psyche.toml"), Ok(&config));

        for name in ["coven_socket_path", "extensions"] {
            assert_eq!(check(&checks, name).unwrap().status, Status::Info, "{name}");
        }
    }

    #[test]
    fn the_json_document_is_versioned_and_counts_failures() {
        let error =
            psyche_config::load_str("schema_version = \"psyche.config.v99\"\n").unwrap_err();
        let checks = run(Path::new("psyche.toml"), Err(&error));
        let document: serde_json::Value = serde_json::from_str(&render_json(&checks)).unwrap();

        assert_eq!(document["schema"], serde_json::json!(DOCTOR_SCHEMA));
        assert_eq!(document["failed"], serde_json::json!(1));
        assert_eq!(document["checks"].as_array().map(Vec::len), Some(4));
        assert_eq!(document["checks"][0]["name"], serde_json::json!("config"));
        assert_eq!(document["checks"][0]["status"], serde_json::json!("fail"));
        assert_eq!(
            document["checks"][1]["status"],
            serde_json::json!("skipped")
        );
    }

    /// One spelling per status, shared by both renderings. Pinned against the
    /// literals so renaming a variant cannot silently rename a word an operator
    /// greps for.
    #[test]
    fn statuses_render_as_their_wire_spellings() {
        assert_eq!(Status::Ok.to_string(), "ok");
        assert_eq!(Status::Warn.to_string(), "warn");
        assert_eq!(Status::Fail.to_string(), "fail");
        assert_eq!(Status::Info.to_string(), "info");
        assert_eq!(Status::Skipped.to_string(), "skipped");
    }

    #[test]
    fn the_text_rendering_names_every_check_and_its_status() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = prepared_data_dir(tmp.path()).unwrap();
        let config = config_for(&data_dir).unwrap();
        let rendered = render_text(&run(Path::new("psyche.toml"), Ok(&config)));

        assert!(rendered.contains("config: ok ("), "{rendered}");
        assert!(rendered.contains("data_dir: ok ("), "{rendered}");
        assert!(rendered.contains("coven_socket_path: info ("), "{rendered}");
        assert_eq!(rendered.lines().count(), 4, "{rendered}");
    }
}
