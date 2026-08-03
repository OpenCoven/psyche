//! The daemon run path, shared by `psyched` and `psyche start`.
//!
//! One function, called from two binaries, for the reason `logging.rs` is also
//! shared: `psyche start`'s help text says it starts the daemon in the
//! foreground, and the only way for that to keep being true is for it to run the
//! same code `psyched` runs. A second implementation would drift, and the way it
//! would drift is by quietly becoming a stub that exits 0 having started
//! nothing.

use std::process::ExitCode;

use psyche_config::Config;
use psyche_runtime::Runtime;

/// Brings a runtime up, waits for a shutdown trigger, then takes the graceful
/// path down.
///
/// Takes an already-loaded [`Config`] rather than a path: the caller has one in
/// hand, and re-reading the file here would both duplicate the error rendering
/// and leave a window in which the daemon runs a different configuration from
/// the one its caller validated.
pub(crate) async fn run(config: Config, shutdown_after_start: bool) -> ExitCode {
    let runtime = match Runtime::start(config).await {
        Ok(runtime) => runtime,
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

    // Short-circuiting rather than nested: with `--shutdown-after-start` the
    // signal handler is never installed, which is the point of the flag.
    if !shutdown_after_start && tokio::signal::ctrl_c().await.is_err() {
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
