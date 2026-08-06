//! Core versioned identifiers and secret-reference types for Psyche.

// One public path per item. Flat re-exports alongside public modules would give
// every type two spellings for downstream crates to drift between, and a glob
// re-export would silently promote anything later added to `secret.rs` into the
// public API. Callers write `psyche_core::schema::ensure_schema_version`.
pub mod schema;
pub mod secret;
