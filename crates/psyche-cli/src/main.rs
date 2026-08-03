//! `psyche` — the operator-facing command line.
//!
//! Argument parsing and dispatch only; the work lives in the `psyche_cli`
//! library, which `psyched` links too.
//!
//! Nothing here reaches the network or reads a credential: every subcommand in
//! this slice is local, so `psyche doctor` is usable on a machine that has never
//! been given a Telegram token. See [`psyche_cli::doctor`] for the rules
//! governing what may be printed.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
// From the `psyche_cli` library target, which `psyched` also links. See that
// crate root for why the daemon path and the log subscriber are shared rather
// than reimplemented per binary.
use psyche_cli::{daemon, doctor, logging};
use psyche_runtime::LifecycleState;

#[derive(Debug, Parser)]
#[command(name = "psyche", version, about = "Psyche familiar runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the daemon in the foreground.
    Start {
        #[arg(long, default_value = "psyche.toml")]
        config: PathBuf,
        /// Start, then immediately shut down. Used by tests and smoke checks so
        /// the full lifecycle runs without needing a signal.
        #[arg(long)]
        shutdown_after_start: bool,
    },
    /// Ask a running daemon to shut down gracefully.
    Stop {
        #[arg(long, default_value = "psyche.toml")]
        config: PathBuf,
    },
    /// Report daemon state.
    Status {
        #[arg(long, default_value = "psyche.toml")]
        config: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run local, credential-free environment checks.
    Doctor {
        #[arg(long, default_value = "psyche.toml")]
        config: PathBuf,
    },
}

/// `unwrap`/`expect` are denied outside tests, so every failure path here
/// returns [`ExitCode::FAILURE`] after rendering the error with `Display`.
///
/// `Display`, never `{:?}`: `psyche_config::ConfigError` reduces every
/// deserializer error to a payload-free message at one place inside that crate,
/// and holds no `toml::de::Error` — whose own `Debug` would carry the entire
/// configuration file, secrets included.
///
/// `#[tokio::main]` on the whole binary rather than an executor built inside the
/// `Start` arm, so `psyche start` and `psyched` run the daemon on an identically
/// configured one. Two independently built executors are the same drift risk
/// that makes `daemon.rs` and `logging.rs` shared files. The cost is that
/// `doctor` and `status` construct an executor they never use.
#[tokio::main]
async fn main() -> ExitCode {
    logging::install();
    let cli = Cli::parse();

    let path = match &cli.command {
        Command::Start { config, .. }
        | Command::Stop { config }
        | Command::Status { config, .. }
        | Command::Doctor { config } => config.clone(),
    };

    let config = match psyche_config::load_path(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    // The path is argv, not configuration content. `?config` is deliberately not
    // recorded: `Config`'s `Debug` redacts today, but this crate does not print
    // structs it does not own.
    tracing::debug!(path = %path.display(), "configuration loaded");

    match cli.command {
        Command::Doctor { .. } => {
            let checks = doctor::run(&config);
            let failed = checks.iter().filter(|c| !c.ok).count();
            for check in &checks {
                let status = if check.ok { "ok" } else { "FAIL" };
                println!("{}: {status} ({})", check.name, check.detail);
            }
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Status { json, .. } => {
            // `stopped`, and marked as not observed. `status` is a separate
            // process from the daemon and there is no IPC in this build, so it
            // cannot see a running `psyched`; on a host where one *is* running,
            // this answer is wrong. Carrying the caveat as a field rather than
            // as a comment in this file is the point — a consumer that learns to
            // trust a bare `state` and is told about the caveat in a later
            // release has already written the code that ignores it.
            //
            // The spelling comes from `LifecycleState`'s `Display`, not from a
            // literal here, so the wire word has exactly one definition.
            let state = LifecycleState::Stopped;
            if json {
                let document = serde_json::json!({
                    "state": state.to_string(),
                    "observed": false,
                });
                println!("{document}");
            } else {
                println!("state: {state} (not observed: no daemon IPC in this build)");
            }
            ExitCode::SUCCESS
        }
        Command::Start {
            shutdown_after_start,
            ..
        } => daemon::run(config, shutdown_after_start).await,
        Command::Stop { .. } => {
            eprintln!("no running daemon to stop");
            ExitCode::SUCCESS
        }
    }
}
