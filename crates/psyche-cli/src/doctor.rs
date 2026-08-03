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

use std::path::Path;

use psyche_config::Config;

/// One named check and the single line it contributes to `doctor` output.
#[derive(Debug)]
pub(crate) struct Check {
    /// Stable identifier an operator or a script can grep for.
    pub(crate) name: &'static str,
    /// Whether the check passed. Any `false` makes `doctor` exit non-zero.
    pub(crate) ok: bool,
    /// Operator-facing explanation. Built from named config fields only.
    pub(crate) detail: String,
}

/// Every check here is local and credential-free. Reaching the Coven socket or
/// a Telegram API is explicitly *not* done — those belong to later gates.
pub(crate) fn run(config: &Config) -> Vec<Check> {
    let mut checks = vec![Check {
        name: "config",
        ok: true,
        // `schema_version()` is a method returning a `&'static str` const, not a
        // value read back from the file: a validated `Config` cannot hold any
        // other version, so this reports the build's contract, not user input.
        detail: format!("schema {}", config.schema_version()),
    }];

    let data_dir: &Path = config.data_dir.as_path();
    let (ok, detail) = match std::fs::create_dir_all(data_dir) {
        Ok(()) => (true, format!("{} writable", data_dir.display())),
        Err(e) => (false, format!("{}: {e}", data_dir.display())),
    };
    checks.push(Check {
        name: "data_dir",
        ok,
        detail,
    });

    checks.push(Check {
        name: "coven_socket_path",
        ok: true,
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
        ok: true,
        detail: format!(
            "{} table(s) present; contents not read",
            config.extensions.len()
        ),
    });

    checks
}
