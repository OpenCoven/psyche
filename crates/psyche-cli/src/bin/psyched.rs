//! `psyched` — the Psyche daemon, in the foreground.
//!
//! Runs until interrupted, then takes the one graceful shutdown path
//! `psyche_runtime::Runtime` offers. There is no forced exit: a caller wanting
//! one terminates the process.

// The same installer `psyche` uses, included rather than copied. Two subscribers
// configured independently would eventually disagree about the writer, and a
// daemon logging to stdout instead of stderr is a corrupted `--json` pipeline.
#[path = "../logging.rs"]
mod logging;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use psyche_runtime::Runtime;

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

    let runtime = Runtime::start(config).await;

    // Short-circuiting rather than nested: with `--shutdown-after-start` the
    // signal handler is never installed, which is the point of the flag.
    if !cli.shutdown_after_start && tokio::signal::ctrl_c().await.is_err() {
        eprintln!("failed to install signal handler");
        return ExitCode::FAILURE;
    }

    match runtime.shutdown().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
