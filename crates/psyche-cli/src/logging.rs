//! Structured JSON logs. Secret-bearing values never reach here as strings —
//! `psyche_core::secret::SecretRef` renders `<redacted>` through both `Debug`
//! and `Display`, so a field holding one cannot leak by accident.
//!
//! Logs go to stderr, never stdout. `psyche status --json` writes a document to
//! stdout that an operator is expected to pipe into a parser; interleaving log
//! lines there would corrupt it.
//!
//! Shared by both binaries — they link this module from the `psyche_cli`
//! library rather than carrying a copy each, so the writer and the format cannot
//! drift apart.

use tracing_subscriber::EnvFilter;

/// Environment variable holding the log filter directives.
pub const LOG_ENV: &str = "PSYCHE_LOG";

/// Filter used when [`LOG_ENV`] is absent or unusable.
pub const DEFAULT_FILTER: &str = "info";

/// Installs the process-wide subscriber, if one is not already installed.
///
/// `try_init`'s error is discarded on purpose: a second install attempt is not
/// a reason to refuse to run, and the failure mode — no logs — is visible.
///
/// A *malformed* filter is not the same thing as an absent one, and this used to
/// treat them identically: `try_from_env(..).unwrap_or_else(|_| info)` meant
/// `PSYCHE_LOG=trce` ran at info in silence, and the operator concluded that
/// trace logging was broken rather than that they had mistyped it. An absent
/// variable still falls back without comment — that is the ordinary case, not a
/// mistake.
///
/// The variable is read here rather than through `EnvFilter::try_from_env`
/// because that collapses "not present" and "does not parse" into one opaque
/// error type whose cause it does not expose.
///
/// This catches syntax, and only syntax. `PSYCHE_LOG=trce` — the obvious typo
/// for `trace` — *is* valid: a bare word is a target directive, so it enables a
/// target named `trce` and nothing else, and the process runs in total silence.
/// Measured, not assumed. Nothing here can distinguish that from a deliberate
/// target filter, and guessing which crate names an operator meant would be a
/// worse error than the one it prevented.
///
/// `eprintln!`, not `tracing::warn!`: the subscriber this function installs is
/// not up yet, so a `tracing` event at this point goes nowhere. stderr also
/// keeps it off the stdout stream that `--json` consumers parse.
pub fn install() {
    let filter = match std::env::var(LOG_ENV) {
        Ok(directives) => EnvFilter::try_new(&directives).unwrap_or_else(|e| {
            // The directives are operator-authored filter syntax, never
            // configuration content, so echoing them back is safe and is the
            // only way the operator learns which part they got wrong.
            eprintln!("{LOG_ENV} is not a valid filter ({e}); using {DEFAULT_FILTER:?}");
            EnvFilter::new(DEFAULT_FILTER)
        }),
        Err(std::env::VarError::NotPresent) => EnvFilter::new(DEFAULT_FILTER),
        // Set, but not readable as UTF-8. Distinct from absent for the same
        // reason a parse failure is: the operator meant something by it.
        Err(e) => {
            eprintln!("{LOG_ENV} could not be read ({e}); using {DEFAULT_FILTER:?}");
            EnvFilter::new(DEFAULT_FILTER)
        }
    };
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_current_span(false)
        .with_writer(std::io::stderr)
        .try_init();
}
