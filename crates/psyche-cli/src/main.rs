//! `psyche` — the operator-facing command line.
//!
//! Nothing here reaches the network or reads a credential: every subcommand in
//! this slice is local, so `psyche doctor` is usable on a machine that has never
//! been given a Telegram token. See [`doctor`] for the rules governing what may
//! be printed.

mod doctor;
mod logging;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
fn main() -> ExitCode {
    logging::install();
    let cli = Cli::parse();

    let path = match &cli.command {
        Command::Start { config }
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
            // Always "stopped" in this slice, and honestly so: `status` is a
            // separate process from `psyched` and there is no IPC yet, so it
            // cannot observe a running daemon's LifecycleState. The follow-on
            // G2 plan adds the socket that makes this a real query. Reporting a
            // guess would be worse than reporting the one state we can know.
            if json {
                // A one-field object rather than a bare string, so the follow-on
                // query can add fields without breaking a parsing consumer.
                let state = serde_json::json!({ "state": "stopped" });
                println!("{state}");
            } else {
                println!("state: stopped");
            }
            ExitCode::SUCCESS
        }
        Command::Start { .. } => {
            eprintln!("run `psyched` to start the daemon in the foreground");
            ExitCode::SUCCESS
        }
        Command::Stop { .. } => {
            eprintln!("no running daemon to stop");
            ExitCode::SUCCESS
        }
    }
}
