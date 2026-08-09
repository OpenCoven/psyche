//! Payload-free failures at the Coven behavior boundary.

use std::fmt;

use psyche_core::contracts::ContractError;

/// A stable, redacted Coven boundary failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PortError {
    /// The requested behavior contract is not supported.
    #[error("Coven behavior contract is unsupported")]
    ContractUnsupported {},
    /// A required negotiated capability is unavailable.
    #[error("required Coven capability is unavailable")]
    CapabilityMissing {},
    /// A typed request failed boundary validation.
    #[error("Coven request is invalid")]
    InvalidRequest,
    /// The claimed request digest did not attest the complete typed request.
    #[error("Coven request digest does not match")]
    RequestDigestMismatch,
    /// A stable identity was reused for different intent.
    #[error("Coven request conflicts with durable intent")]
    IntentConflict,
    /// Returned evidence did not echo its complete correlation.
    #[error("Coven response correlation does not match")]
    CorrelationMismatch,
    /// The requested durable entity does not exist.
    #[error("Coven entity was not found")]
    NotFound,
    /// Coven authoritatively denied the operation.
    #[error("Coven policy denied the operation")]
    PolicyDenied,
    /// No authoritative outcome was available.
    #[error("Coven outcome is unavailable")]
    Unavailable,
    /// A deterministic test operation reached a scripted stall.
    #[error("Coven operation stalled")]
    Stalled,
    /// A deterministic fake received a call not present in its script.
    #[error("Coven call was not scripted")]
    UnexpectedCall,
    /// Coven returned an invalid typed result.
    #[error("Coven response is invalid")]
    InvalidResponse,
}

impl From<ContractError> for PortError {
    fn from(_error: ContractError) -> Self {
        Self::InvalidRequest
    }
}

/// Failure returned by a termination persistence implementation.
pub enum TerminationPersistenceFailure<E> {
    /// The candidate would fork, gap, or rewrite durable revision history.
    Conflict(E),
    /// Durability could not be established.
    Write(E),
}

impl<E> fmt::Debug for TerminationPersistenceFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(_) => formatter.write_str("TerminationPersistenceFailure::Conflict"),
            Self::Write(_) => formatter.write_str("TerminationPersistenceFailure::Write"),
        }
    }
}

/// Phase-specific failure from the persist-then-terminate coordinator.
pub enum TerminationDispatchError<E> {
    /// The candidate binding violated the owned execution contract.
    Contract(ContractError),
    /// The termination-request revision did not become durable.
    RequestPersistence(E),
    /// Persistence returned bytes other than the requested canonical bytes.
    PersistedBindingMismatch,
    /// The behavior port failed after request persistence.
    Port(PortError),
    /// Returned acknowledgement or unresolved evidence did not match.
    OutcomeEvidenceMismatch,
    /// Coven responded but outcome durability is indeterminate.
    OutcomePersistenceIndeterminate(E),
    /// A revision fork, gap, rewrite, or divergent replay was detected.
    RevisionConflict(E),
    /// Persistence returned bytes other than the outcome canonical bytes.
    PersistedOutcomeMismatch,
}

impl<E> fmt::Display for TerminationDispatchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Contract(_) => "termination request contract validation failed",
            Self::RequestPersistence(_) => "termination request persistence failed",
            Self::PersistedBindingMismatch => "persisted termination request bytes do not match",
            Self::Port(_) => "termination port call failed",
            Self::OutcomeEvidenceMismatch => "termination outcome evidence does not match",
            Self::OutcomePersistenceIndeterminate(_) => {
                "termination outcome persistence is indeterminate"
            }
            Self::RevisionConflict(_) => "termination revision conflicts with durable history",
            Self::PersistedOutcomeMismatch => "persisted termination outcome bytes do not match",
        })
    }
}

impl<E> fmt::Debug for TerminationDispatchError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "TerminationDispatchError({self})")
    }
}

impl<E> std::error::Error for TerminationDispatchError<E> {}
