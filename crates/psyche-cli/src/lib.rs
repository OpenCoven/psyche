//! Everything `psyche` and `psyched` both do, in one place.
//!
//! The two binaries are two front doors onto one daemon. `psyche start`'s help
//! text says it starts the daemon in the foreground, and the only way for that
//! to keep being true is for it to run the code `psyched` runs — a second
//! implementation would drift, and the way it would drift is by quietly becoming
//! a stub that exits 0 having started nothing. The same argument covers
//! [`logging`]: two subscribers configured independently would eventually
//! disagree about the writer, and a daemon logging to stdout instead of stderr
//! is a corrupted `--json` pipeline.
//!
//! That rationale is why these modules are shared. It is not why they were once
//! shared with `#[path]` includes from two crate roots — that mechanism compiled
//! each file twice per build, kept doc-tests from ever running against them (a
//! binary target has none), and left them unreachable from `tests/`. A library
//! target costs nothing here: `publish = false` is set workspace-wide, so the
//! nominal public surface never reaches a registry.

pub mod daemon;
pub mod doctor;
pub mod logging;

// The exit-code space, defined in one place because both binaries owe an
// operator the same one.
//
// An exit code is the most expensive contract a CLI has: it gets baked into a
// systemd `SuccessExitStatus=`, a Kubernetes probe, and a shell `||`, none of
// which are visible from this repository once shipped. Defining the space before
// the first release is the only cheap moment.
//
// The codes are `u8` rather than `ExitCode` values because `ExitCode::from` is
// not a `const fn`; callers convert at the return site. Tests compare against
// these constants, and `assert_cmd`'s `code()` wants an integer anyway.
//
// | code | meaning                                                    |
// |------|------------------------------------------------------------|
// | 0    | the command did what it was asked                          |
// | 1    | unexpected — deliberately unassigned, so it stays a signal |
// | 2    | usage; owned by clap, never returned from this crate       |
// | 3    | configuration is missing, unreadable, or invalid           |
// | 4    | a daemon was needed and could not be reached               |
// | 5    | a check ran and failed                                     |

/// Success. The command did what it was asked to do.
pub const EXIT_OK: u8 = 0;

/// Usage error: unknown flag, missing argument, bad subcommand.
///
/// Owned by clap, which exits `2` itself before any code here runs. Declared so
/// the space is documented in one place and so nothing else claims `2`.
pub const EXIT_USAGE: u8 = 2;

/// Configuration is missing, unreadable, too large, or invalid.
///
/// The distinction `doctor` exists to draw: this code means the file is wrong,
/// as opposed to [`EXIT_CHECK_FAILED`], which means the file was fine and the
/// environment it describes was not.
pub const EXIT_CONFIG: u8 = 3;

/// A running daemon was required and could not be reached.
///
/// Also what a subcommand returns when the build has no way to reach one at all
/// — an operator scripting `psyche stop && deploy` needs that to be a failure,
/// not a `0` that reads as "the daemon is gone".
pub const EXIT_UNAVAILABLE: u8 = 4;

/// A check ran to completion and reported a failure.
///
/// The configuration was readable and valid; something it describes is not in
/// the state it needs to be.
pub const EXIT_CHECK_FAILED: u8 = 5;
