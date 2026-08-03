//! Structured JSON logs. Secret-bearing values never reach here as strings —
//! `psyche_core::secret::SecretRef` renders `<redacted>` through both `Debug`
//! and `Display`, so a field holding one cannot leak by accident.
//!
//! Logs go to stderr, never stdout. `psyche status --json` writes a document to
//! stdout that an operator is expected to pipe into a parser; interleaving log
//! lines there would corrupt it.
//!
//! Shared by both binaries — `psyched` includes this file with `#[path]` rather
//! than carrying its own copy, so the writer and format cannot drift apart.

use tracing_subscriber::EnvFilter;

/// Installs the process-wide subscriber, if one is not already installed.
///
/// `try_init`'s error is discarded on purpose: a second install attempt is not
/// a reason to refuse to run, and the failure mode — no logs — is visible.
pub(crate) fn install() {
    let filter = EnvFilter::try_from_env("PSYCHE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_current_span(false)
        .with_writer(std::io::stderr)
        .try_init();
}
