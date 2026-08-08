//! Payload-free failures at a surface behavior boundary.

/// Stable, redacted surface port failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PortError {
    /// A surface event failed its owned contract.
    #[error("surface event is invalid")]
    InvalidEvent,
    /// A surface effect failed its owned contract.
    #[error("surface effect is invalid")]
    InvalidEffect,
    /// A stable identity was reused for different intent.
    #[error("surface operation conflicts with durable intent")]
    IntentConflict,
    /// Surface policy denied the operation.
    #[error("surface policy denied the operation")]
    PolicyDenied,
    /// Delivery outcome is unavailable.
    #[error("surface outcome is unavailable")]
    Unavailable,
    /// A deterministic fake received an unscripted call.
    #[error("surface call was not scripted")]
    UnexpectedCall,
    /// A deterministic test operation reached a scripted stall.
    #[error("surface operation stalled")]
    Stalled,
    /// A surface returned an invalid response.
    #[error("surface response is invalid")]
    InvalidResponse,
}
