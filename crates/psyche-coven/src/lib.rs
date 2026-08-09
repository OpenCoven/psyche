//! Behavior-level Coven execution boundary.

pub mod error;
pub mod port;

pub use error::{PortError, TerminationDispatchError, TerminationPersistenceFailure};
pub use port::{
    AdoptionDisposition, AdoptionRequest, ArtifactReference, Capability, CapabilityProfile,
    ContentAddressedReference, CovenEvent, CovenPort, EventCursor, EventPage,
    ExecutionArtifactBinding, ExecutionCorrelation, ExecutionRequestInput, NegotiateRequest,
    ReconciliationDisposition, ReconciliationRequest, ResultBundle, SessionSnapshot,
    TerminationDisposition, TerminationPersistence, TerminationRequest,
    derive_termination_outcome_revision, persist_then_terminate,
};
