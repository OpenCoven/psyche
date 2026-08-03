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
use psyche_cli::{
    EXIT_CHECK_FAILED, EXIT_CONFIG, EXIT_OK, EXIT_UNAVAILABLE, daemon, doctor, logging,
};
use psyche_runtime::LifecycleState;

#[derive(Debug, Parser)]
#[command(name = "psyche", version, about = "Psyche familiar runtime")]
struct Cli {
    /// Configuration file. Resolution order: --config, $PSYCHE_CONFIG,
    /// ./psyche.toml.
    ///
    /// `global`, so it is accepted either side of the subcommand:
    /// `psyche --config X status` reads as the natural order and used to be a
    /// usage error. It also collapses what were four identical declarations and
    /// a four-arm match that existed only to pull the value back out of them.
    ///
    /// The default is relative to the working directory, which a systemd system
    /// unit leaves at `/` — so a service without an explicit path was resolving
    /// `/psyche.toml`. `$PSYCHE_CONFIG` is the fix for the container case, where
    /// the path cannot be put on argv. Deliberately no XDG or `/etc` lookup:
    /// deferred, not rejected, and it belongs with the packaging work rather
    /// than here.
    #[arg(
        long,
        global = true,
        env = "PSYCHE_CONFIG",
        default_value = "psyche.toml"
    )]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the daemon in the foreground. Equivalent to running `psyched`.
    Start {
        /// Start, then immediately shut down. Used by tests and smoke checks so
        /// the full lifecycle runs without needing a signal.
        #[arg(long)]
        shutdown_after_start: bool,
    },
    /// Not implemented in this build: there is no daemon IPC yet.
    ///
    /// The help text says so because the command cannot do it. It used to
    /// promise a graceful shutdown and exit 0 having done nothing, which is what
    /// `psyche stop && deploy` reads as success.
    Stop,
    /// Report daemon state.
    Status {
        /// Emit a `psyche.status.v1` document on stdout instead of a line of
        /// prose.
        #[arg(long)]
        json: bool,
    },
    /// Run local, credential-free environment checks.
    Doctor {
        /// Emit a `psyche.doctor.v1` document on stdout instead of one line per
        /// check.
        ///
        /// The line format is what an operator would otherwise `grep`, and a
        /// format that gets grepped is frozen whether or not anyone decided to
        /// freeze it.
        #[arg(long)]
        json: bool,
    },
}

/// Loads the configuration, runs every check against whatever came back, prints
/// the report, and returns the code that describes it.
///
/// The load result is passed into [`doctor::run`] rather than unwrapped here:
/// a configuration that will not load is the case `doctor` most exists for, and
/// the command has to have something to say about it beyond one raw error.
///
/// Three outcomes, three codes. `EXIT_CONFIG` means the file is wrong;
/// `EXIT_CHECK_FAILED` means the file was fine and something it describes is
/// not. An operator scripting this could not previously tell them apart.
fn doctor_command(path: &std::path::Path, json: bool) -> ExitCode {
    let loaded = psyche_config::load_path(path);
    let checks = doctor::run(path, loaded.as_ref());

    // stdout: the report *is* this command's output, and `doctor > report.txt`
    // has to capture all of it. The failure reason travels inside the `config`
    // check rather than being duplicated onto stderr.
    print!(
        "{}",
        if json {
            doctor::render_json(&checks) + "\n"
        } else {
            doctor::render_text(&checks)
        }
    );

    if loaded.is_err() {
        ExitCode::from(EXIT_CONFIG)
    } else if doctor::failures(&checks) > 0 {
        ExitCode::from(EXIT_CHECK_FAILED)
    } else {
        ExitCode::from(EXIT_OK)
    }
}

/// `unwrap`/`expect` are denied outside tests, so every failure path here
/// returns a named exit code after rendering the error with `Display`. The codes
/// are defined and documented in [`psyche_cli`]; nothing here invents one.
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

    let path = cli.config;

    // `doctor` is dispatched *before* the load, and is the only subcommand that
    // is. Loading first short-circuited it on exactly the input it exists to
    // explain: `psyche doctor --config /nope.toml` printed one raw `ConfigError`
    // and exited with zero checks run, which also left the `config` check
    // vacuous — it could only ever report `ok`.
    //
    // Taken out of `cli.command` here rather than matched again below, so the
    // match that follows has no `Doctor` arm to write. A dead arm returning a
    // code nothing can observe is the same kind of defect as a command that
    // reports a state it did not measure.
    let command = match cli.command {
        Command::Doctor { json } => return doctor_command(&path, json),
        other => other,
    };

    let config = match psyche_config::load_path(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };

    // The path is argv, not configuration content. `?config` is deliberately not
    // recorded: `Config`'s `Debug` redacts today, but this crate does not print
    // structs it does not own.
    tracing::debug!(path = %path.display(), "configuration loaded");

    match command {
        // Returned by the match above, which is the only way `command` is
        // bound. Exhaustiveness still demands the arm; `unreachable!` is the
        // honest thing to put in it, and it names the reason so the next reader
        // does not try to write a test that reaches here.
        Command::Doctor { .. } => {
            unreachable!("doctor is dispatched before the configuration load")
        }
        Command::Status { json } => {
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
            ExitCode::from(EXIT_OK)
        }
        Command::Start {
            shutdown_after_start,
            ..
        } => daemon::run(config, shutdown_after_start).await,
        Command::Stop => {
            // Non-zero, because nothing was stopped. The previous form printed
            // "no running daemon to stop" and exited 0, which a rolling restart
            // scripted as `psyche stop && deploy` reads as "the old daemon is
            // gone". It is not gone; this build has no way to ask it.
            eprintln!("stop is not implemented in this build (no daemon IPC)");
            ExitCode::from(EXIT_UNAVAILABLE)
        }
    }
}
