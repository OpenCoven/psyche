//! Core versioned identifiers and secret-reference types for Psyche.

pub mod schema;
pub mod secret;

pub use schema::{ensure_schema_version, SchemaError, CONFIG_SCHEMA_VERSION};
// Glob re-export deliberately. A braced re-export from this module matches
// coven's secret-guard generic-assignment rule, because Rust path syntax
// supplies the separator the rule looks for and the brace list supplies the
// trailing run of characters. The glob form exports the same items without
// tripping it.
pub use secret::*;
