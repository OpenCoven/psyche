//! The daemon run path, shared by `psyched` and `psyche start`.
//!
//! One function, called from two binaries, for the reason `logging.rs` is also
//! shared: `psyche start`'s help text says it starts the daemon in the
//! foreground, and the only way for that to keep being true is for it to run the
//! same code `psyched` runs. A second implementation would drift, and the way it
//! would drift is by quietly becoming a stub that exits 0 having started
//! nothing.

use std::io;
use std::process::ExitCode;

use psyche_config::Config;
use psyche_runtime::Runtime;

/// The signals that mean "shut down", with their handlers already installed.
///
/// Holding them in a value is the point: on Unix a handler exists from the
/// moment [`Signals::install`] returns, not from the first await. That is what
/// lets [`run`] install *before* [`Runtime::start`], so a signal arriving during
/// startup is queued rather than killing the process at its default
/// disposition — which for SIGTERM means no drain and, once `start` opens the
/// data directory and binds the Coven socket, a leaked socket and lease.
#[cfg(unix)]
struct Signals {
    term: tokio::signal::unix::Signal,
    int: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    /// Installs handlers for SIGTERM and SIGINT.
    ///
    /// Both, not just SIGINT: `tokio::signal::ctrl_c` covers SIGINT alone, so a
    /// daemon relying on it leaves SIGTERM at its default disposition — and
    /// SIGTERM is what `systemctl stop`, `docker stop`, a Kubernetes eviction
    /// and a bare `kill` all send. The drain was unreachable in production
    /// before this existed.
    fn install() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    /// Resolves when either signal is delivered.
    ///
    /// Which one it was is deliberately not reported: there is exactly one
    /// shutdown path, and a caller cannot act on the distinction.
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.term.recv() => {}
            _ = self.int.recv() => {}
        }
    }
}

/// Windows shim. Task 8 ships Windows binaries through npm, and this file is on
/// that build's path even though a Windows service is not a supported
/// deployment yet.
///
/// Unlike the Unix form this cannot install anything eagerly — `ctrl_c`
/// registers its handler at first await — so `install` succeeding here means
/// only "nothing to do", not "a handler is armed". The startup window that the
/// Unix ordering closes therefore stays open on Windows; it is called out here
/// rather than papered over.
#[cfg(not(unix))]
struct Signals {
    _private: (),
}

#[cfg(not(unix))]
impl Signals {
    /// Infallible: there is nothing to install until the first await.
    fn install() -> io::Result<Self> {
        Ok(Self { _private: () })
    }

    /// Resolves on Ctrl-C, or immediately if the handler cannot be installed.
    ///
    /// An install failure here is treated as a shutdown request rather than as
    /// a hang: the alternative is a daemon that can never be stopped except by
    /// killing it, which is strictly worse than one that exits early and says
    /// nothing was wrong with the drain.
    async fn wait(&mut self) {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("failed to install signal handler: {e}");
        }
    }
}

/// Brings a runtime up, waits for a shutdown signal, then takes the graceful
/// path down.
///
/// Takes an already-loaded [`Config`] rather than a path: the caller has one in
/// hand, and re-reading the file here would both duplicate the error rendering
/// and leave a window in which the daemon runs a different configuration from
/// the one its caller validated.
///
/// Ordering is load-bearing. Handlers are installed first, the runtime is
/// started second, and only then is the signal awaited. Installing lazily at the
/// await — which is what `ctrl_c().await` does — leaves every signal delivered
/// during startup at its default disposition.
///
/// When installation fails there is deliberately no runtime yet, so the honest
/// answer is to report the failure and exit without starting anything. The
/// previous form installed after `start` and returned failure while a `Running`
/// runtime went out of scope undrained.
pub async fn run(config: Config, shutdown_after_start: bool) -> ExitCode {
    // `--shutdown-after-start` skips installation entirely: the flag exists so
    // the full lifecycle can run without a signal, and arming handlers that
    // nothing will ever wait on would only change what the process does to
    // signals it was not asked to handle.
    let signals = if shutdown_after_start {
        None
    } else {
        match Signals::install() {
            Ok(signals) => Some(signals),
            Err(e) => {
                eprintln!("failed to install signal handlers: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    let runtime = match Runtime::start(config).await {
        Ok(runtime) => runtime,
        // Unreachable, and permanently so: `RuntimeError` has no variants, so
        // this arm cannot be constructed and cannot be covered by a test. It is
        // written out anyway because the signature is what absorbs the first
        // real startup failure — see the type's docs in psyche-runtime.
        Err(e) => {
            // Display, not `{:?}` — see the note on `psyche`'s `main`.
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    // Read back through `config()` because `start` consumed the `Config`. The
    // data directory is the field operators most often have pointed somewhere
    // they did not mean, and it is a path, never a value from an extension
    // table.
    tracing::info!(
        data_dir = %runtime.config().data_dir.display(),
        "psyche daemon ready"
    );

    if let Some(mut signals) = signals {
        signals.wait().await;
    }

    match runtime.shutdown().await {
        Ok(()) => ExitCode::SUCCESS,
        // Unreachable for the same reason as the arm above.
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
