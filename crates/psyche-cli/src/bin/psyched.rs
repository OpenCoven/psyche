//! `psyched` — the Psyche daemon, in the foreground.
//!
//! Runs until interrupted, then takes the one graceful shutdown path
//! `psyche_runtime::Runtime` offers. There is no forced exit: a caller wanting
//! one terminates the process.
//!
//! A thin wrapper: argument parsing, configuration loading, then the shared run
//! path in [`psyche_cli::daemon`]. `psyche start` calls the same function, so the
//! two cannot come to mean different things.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
// Linked from the `psyche_cli` library rather than copied or `#[path]`-included:
// two subscribers configured independently would eventually disagree about the
// writer, and a daemon logging to stdout instead of stderr is a corrupted
// `--json` pipeline.
use psyche_cli::{EXIT_CONFIG, daemon, logging};

#[derive(Debug, Parser)]
#[command(
    name = "psyched",
    version,
    about = "Psyche daemon, in the foreground. Equivalent to `psyche start`."
)]
struct Cli {
    /// Configuration file. Resolution order: --config, $PSYCHE_CONFIG,
    /// ./psyche.toml.
    ///
    /// The default is relative to the working directory, which a systemd system
    /// unit leaves at `/`. Set $PSYCHE_CONFIG or pass --config in a unit file or
    /// a container, or the path resolves to /psyche.toml.
    //
    // Must match `psyche`'s, flag for flag — an operator who learns one and
    // writes the other into a unit file is entitled to have it work. Asserted by
    // `psyche_start_and_psyched_accept_the_same_flags`.
    #[arg(long, env = "PSYCHE_CONFIG", default_value = "psyche.toml")]
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
            // Display, not `{:?}` — see the note on `psyche`'s `main`. The code
            // is the same one `psyche` returns for the same file: an operator's
            // unit file must not have to know which binary it invoked.
            eprintln!("{e}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };

    daemon::run(config, cli.shutdown_after_start).await
}
