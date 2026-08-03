//! `psyched` — the Psyche daemon, in the foreground.
//!
//! Runs until interrupted, then takes the one graceful shutdown path
//! `psyche_runtime::Runtime` offers. There is no forced exit: a caller wanting
//! one terminates the process.
//!
//! A thin wrapper: argument parsing, configuration loading, then the shared run
//! path in `daemon.rs`. `psyche start` calls the same function, so the two
//! cannot come to mean different things.

// Included rather than copied, for the reason `daemon.rs` gives: two subscribers
// configured independently would eventually disagree about the writer, and a
// daemon logging to stdout instead of stderr is a corrupted `--json` pipeline.
#[path = "../daemon.rs"]
mod daemon;
#[path = "../logging.rs"]
mod logging;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "psyched", version, about = "Psyche daemon")]
struct Cli {
    #[arg(long, default_value = "psyche.toml")]
    config: PathBuf,
    /// Start, then immediately shut down. Used by tests and smoke checks so the
    /// full lifecycle runs without needing a signal.
    #[arg(long)]
    shutdown_after_start: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    logging::install();

    let cli = Cli::parse();
    let config = match psyche_config::load_path(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            // Display, not `{:?}` — see the note on `psyche`'s `main`.
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    daemon::run(config, cli.shutdown_after_start).await
}
