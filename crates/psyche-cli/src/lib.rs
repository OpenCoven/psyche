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
