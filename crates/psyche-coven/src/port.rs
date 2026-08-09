//! Typed behavior-level Coven operations.

use std::collections::{BTreeSet, HashSet};

use psyche_core::contracts::execution::{
    CancellationAcknowledgementEvidence, CancellationAcknowledgementKind, CancellationState,
    CancellationUnresolvedEvidence, ExecutionBinding,
};
use psyche_core::contracts::{ContractError, RecordKind};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest as _, Sha256};

use crate::error::{PortError, TerminationDispatchError, TerminationPersistenceFailure};

const EXECUTION_REQUEST_SCHEMA: &str = "psyche.execution_request.v1";
const MAX_STRING_BYTES: usize = 255;
const MAX_ARTIFACTS: usize = 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_CONTENT_SIZE_BYTES: u64 = i64::MAX as u64;

/// A capability that a Coven implementation may advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Stable digest-bound request adoption.
    StableAdoption,
    /// Durable ambiguity reconciliation and fencing.
    AmbiguityFence,
    /// Ordered event pages with durable cursors.
    OrderedEvents,
    /// O5-authoritative termination evidence.
    AuthoritativeTermination,
    /// Content-addressed result and artifact references.
    ContentAddressedResults,
}

impl Capability {
    /// Stable wire spelling used during negotiation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableAdoption => "stable_adoption",
            Self::AmbiguityFence => "ambiguity_fence",
            Self::OrderedEvents => "ordered_events",
            Self::AuthoritativeTermination => "authoritative_termination",
            Self::ContentAddressedResults => "content_addressed_results",
        }
    }
}

/// Required Coven API version and behaviors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiateRequest {
    /// Exact behavior API version.
    pub required_api_version: String,
    /// Stable required capability spellings.
    pub required_capabilities: BTreeSet<String>,
}

impl NegotiateRequest {
    /// Builds a request for an exact API version with no optional capabilities.
    pub fn new(required_api_version: impl Into<String>) -> Self {
        Self {
            required_api_version: required_api_version.into(),
            required_capabilities: BTreeSet::new(),
        }
    }

    /// Adds one typed required capability.
    #[must_use]
    pub fn requiring(mut self, capability: Capability) -> Self {
        self.required_capabilities
            .insert(capability.as_str().to_owned());
        self
    }

    /// Validates bounded canonical contract and capability spellings.
    pub fn validate(&self) -> Result<(), PortError> {
        bounded(&self.required_api_version)?;
        if self.required_capabilities.len() > 64
            || self
                .required_capabilities
                .iter()
                .any(|value| !stable_token(value, MAX_STRING_BYTES))
        {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }
}

/// Negotiated Coven behavior profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityProfile {
    /// Exact supported behavior API version.
    pub api_version: String,
    /// Stable supported capability spellings.
    pub capabilities: BTreeSet<String>,
}

impl CapabilityProfile {
    /// Validates the bounded profile.
    pub fn validate(&self) -> Result<(), PortError> {
        NegotiateRequest {
            required_api_version: self.api_version.clone(),
            required_capabilities: self.capabilities.clone(),
        }
        .validate()
    }
}

/// One immutable content binding required by an execution request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionArtifactBinding {
    /// Stable artifact identity.
    pub artifact_id: String,
    /// Digest of the exact artifact bytes.
    pub digest: Sha256Digest,
    /// Strict lowercase media type.
    pub media_type: String,
    /// Exact payload length.
    pub size: u64,
}

impl ExecutionArtifactBinding {
    fn validate(&self) -> Result<(), PortError> {
        bounded(&self.artifact_id)?;
        validate_media_type(&self.media_type)?;
        if self.size == 0 || self.size > MAX_SAFE_INTEGER {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }
}

/// Canonical digest input for launch and input adoption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionRequestInput {
    /// Launches a new supervised session.
    Launch {
        /// Exact schema string.
        schema_version: String,
        /// Stable operation identity.
        request_id: RequestId,
        /// Owning graph.
        graph_id: RecordId,
        /// Owning graph node.
        node_id: RecordId,
        /// Owning execution attempt.
        attempt_id: RecordId,
        /// Principal whose authority admitted the request.
        principal_id: String,
        /// Pinned familiar identity snapshot.
        familiar_snapshot_id: RecordId,
        /// Stable project identity.
        project_id: String,
        /// Canonical absolute project root.
        project_root: String,
        /// Canonical absolute working directory within the project root.
        cwd: String,
        /// Supported harness spelling.
        harness: String,
        /// Context manifest digest.
        context_manifest_digest: Sha256Digest,
        /// Optional delegation digest.
        delegation_digest: Option<Sha256Digest>,
        /// Budget digest.
        budget_digest: Sha256Digest,
        /// Ordered, unique required artifacts.
        required_artifact_bindings: Vec<ExecutionArtifactBinding>,
        /// Digest of the typed execution payload.
        payload_digest: Sha256Digest,
        /// Correlation creation time.
        #[serde(with = "time::serde::rfc3339")]
        created_at: time::OffsetDateTime,
        /// Correlation deadline.
        #[serde(with = "time::serde::rfc3339")]
        valid_until: time::OffsetDateTime,
    },
    /// Sends input to an already adopted session.
    Input {
        /// Exact schema string.
        schema_version: String,
        /// Stable operation identity.
        request_id: RequestId,
        /// Owning graph.
        graph_id: RecordId,
        /// Owning graph node.
        node_id: RecordId,
        /// Owning execution attempt.
        attempt_id: RecordId,
        /// Principal whose authority admitted the request.
        principal_id: String,
        /// Pinned familiar identity snapshot.
        familiar_snapshot_id: RecordId,
        /// Stable project identity.
        project_id: String,
        /// Adopted Coven session.
        session_id: String,
        /// Digest of the exact input.
        input_digest: Sha256Digest,
        /// Context manifest digest.
        context_manifest_digest: Sha256Digest,
        /// Ordered, unique required artifacts.
        required_artifact_bindings: Vec<ExecutionArtifactBinding>,
        /// Digest of the typed execution payload.
        payload_digest: Sha256Digest,
        /// Correlation creation time.
        #[serde(with = "time::serde::rfc3339")]
        created_at: time::OffsetDateTime,
        /// Correlation deadline.
        #[serde(with = "time::serde::rfc3339")]
        valid_until: time::OffsetDateTime,
    },
}

impl ExecutionRequestInput {
    /// Validates every typed request field before digesting or dispatch.
    pub fn validate(&self) -> Result<(), PortError> {
        let (
            schema,
            request_id,
            graph_id,
            node_id,
            attempt_id,
            principal_id,
            familiar_snapshot_id,
            project_id,
            artifacts,
            created_at,
            valid_until,
        ) = match self {
            Self::Launch {
                schema_version,
                request_id,
                graph_id,
                node_id,
                attempt_id,
                principal_id,
                familiar_snapshot_id,
                project_id,
                project_root,
                cwd,
                harness,
                required_artifact_bindings,
                created_at,
                valid_until,
                ..
            } => {
                validate_absolute_path(project_root)?;
                validate_absolute_path(cwd)?;
                if !path_is_within(project_root, cwd) || harness != "codex" {
                    return Err(PortError::InvalidRequest);
                }
                (
                    schema_version,
                    request_id,
                    graph_id,
                    node_id,
                    attempt_id,
                    principal_id,
                    familiar_snapshot_id,
                    project_id,
                    required_artifact_bindings,
                    created_at,
                    valid_until,
                )
            }
            Self::Input {
                schema_version,
                request_id,
                graph_id,
                node_id,
                attempt_id,
                principal_id,
                familiar_snapshot_id,
                project_id,
                session_id,
                required_artifact_bindings,
                created_at,
                valid_until,
                ..
            } => {
                bounded(session_id)?;
                (
                    schema_version,
                    request_id,
                    graph_id,
                    node_id,
                    attempt_id,
                    principal_id,
                    familiar_snapshot_id,
                    project_id,
                    required_artifact_bindings,
                    created_at,
                    valid_until,
                )
            }
        };

        if schema != EXECUTION_REQUEST_SCHEMA
            || request_id.as_str().is_empty()
            || graph_id.kind() != RecordKind::Graph
            || node_id.kind() != RecordKind::GraphNode
            || attempt_id.kind() != RecordKind::Attempt
            || familiar_snapshot_id.kind() != RecordKind::IdentitySnapshot
        {
            return Err(PortError::InvalidRequest);
        }
        bounded(principal_id)?;
        bounded(project_id)?;
        validate_window(*created_at, *valid_until)?;
        validate_artifact_bindings(artifacts)
    }

    fn request_id(&self) -> &RequestId {
        match self {
            Self::Launch { request_id, .. } | Self::Input { request_id, .. } => request_id,
        }
    }

    fn correlation_fields(
        &self,
    ) -> (
        &RecordId,
        &RecordId,
        &RecordId,
        &RecordId,
        &str,
        time::OffsetDateTime,
        time::OffsetDateTime,
    ) {
        match self {
            Self::Launch {
                graph_id,
                node_id,
                attempt_id,
                familiar_snapshot_id,
                project_id,
                created_at,
                valid_until,
                ..
            }
            | Self::Input {
                graph_id,
                node_id,
                attempt_id,
                familiar_snapshot_id,
                project_id,
                created_at,
                valid_until,
                ..
            } => (
                graph_id,
                node_id,
                attempt_id,
                familiar_snapshot_id,
                project_id,
                *created_at,
                *valid_until,
            ),
        }
    }
}

/// A digest-attested execution adoption request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionRequest {
    input: ExecutionRequestInput,
    request_digest: Sha256Digest,
}

impl AdoptionRequest {
    /// Validates and digests a complete execution request.
    pub fn new(input: ExecutionRequestInput) -> Result<Self, PortError> {
        input.validate()?;
        let request_digest = digest(&input)?;
        Ok(Self {
            input,
            request_digest,
        })
    }

    /// Complete canonical digest input.
    pub fn input(&self) -> &ExecutionRequestInput {
        &self.input
    }

    /// Claimed digest carried by the wire envelope.
    pub fn request_digest(&self) -> &Sha256Digest {
        &self.request_digest
    }

    /// Correlation derived from the same request and digest.
    pub fn correlation(&self) -> ExecutionCorrelation {
        let (
            graph_id,
            node_id,
            attempt_id,
            familiar_snapshot_id,
            project_id,
            created_at,
            valid_until,
        ) = self.input.correlation_fields();
        ExecutionCorrelation {
            request_id: self.input.request_id().clone(),
            request_digest: self.request_digest.clone(),
            familiar_snapshot_id: familiar_snapshot_id.clone(),
            project_id: project_id.to_owned(),
            graph_id: graph_id.clone(),
            node_id: node_id.clone(),
            attempt_id: attempt_id.clone(),
            created_at,
            valid_until,
        }
    }

    /// Recomputes the digest from the complete typed input.
    pub fn recompute_digest(&self) -> Result<Sha256Digest, PortError> {
        digest(&self.input).map_err(Into::into)
    }

    /// Validates the claimed digest with a constant-time comparison.
    pub fn validate_digest(&self) -> Result<(), PortError> {
        let recomputed = self.recompute_digest()?;
        if !constant_time_equal(
            recomputed.as_str().as_bytes(),
            self.request_digest.as_str().as_bytes(),
        ) {
            return Err(PortError::RequestDigestMismatch);
        }
        self.input.validate()
    }
}

/// Complete immutable request correlation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionCorrelation {
    /// Stable execution request identity.
    pub request_id: RequestId,
    /// Digest of the complete typed execution request.
    pub request_digest: Sha256Digest,
    /// Pinned familiar identity.
    pub familiar_snapshot_id: RecordId,
    /// Stable project identity.
    pub project_id: String,
    /// Owning graph.
    pub graph_id: RecordId,
    /// Owning graph node.
    pub node_id: RecordId,
    /// Owning attempt.
    pub attempt_id: RecordId,
    /// Correlation creation time.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    /// Correlation deadline.
    #[serde(with = "time::serde::rfc3339")]
    pub valid_until: time::OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionCorrelationWire {
    request_id: RequestId,
    request_digest: Sha256Digest,
    familiar_snapshot_id: RecordId,
    project_id: String,
    graph_id: RecordId,
    node_id: RecordId,
    attempt_id: RecordId,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    valid_until: time::OffsetDateTime,
}

impl TryFrom<ExecutionCorrelationWire> for ExecutionCorrelation {
    type Error = PortError;

    fn try_from(wire: ExecutionCorrelationWire) -> Result<Self, Self::Error> {
        let value = Self {
            request_id: wire.request_id,
            request_digest: wire.request_digest,
            familiar_snapshot_id: wire.familiar_snapshot_id,
            project_id: wire.project_id,
            graph_id: wire.graph_id,
            node_id: wire.node_id,
            attempt_id: wire.attempt_id,
            created_at: wire.created_at,
            valid_until: wire.valid_until,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ExecutionCorrelation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ExecutionCorrelationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl ExecutionCorrelation {
    /// Validates all field kinds, bounds, and the canonical UTC lifetime.
    pub fn validate(&self) -> Result<(), PortError> {
        if self.graph_id.kind() != RecordKind::Graph
            || self.node_id.kind() != RecordKind::GraphNode
            || self.attempt_id.kind() != RecordKind::Attempt
            || self.familiar_snapshot_id.kind() != RecordKind::IdentitySnapshot
        {
            return Err(PortError::InvalidRequest);
        }
        bounded(&self.project_id)?;
        validate_window(self.created_at, self.valid_until)
    }
}

/// Result of stable adoption or lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptionDisposition {
    /// One durable Coven session owns the request.
    Adopted {
        /// Stable opaque session identity.
        session_id: String,
    },
    /// Coven durably proved that no adoption occurred.
    ProvenNotAdopted,
    /// Adoption remains ambiguous.
    Unknown,
}

impl AdoptionDisposition {
    /// Validates response bounds.
    pub fn validate(&self) -> Result<(), PortError> {
        if let Self::Adopted { session_id } = self {
            bounded(session_id).map_err(|_| PortError::InvalidResponse)?;
        }
        Ok(())
    }
}

/// Correlation-bound ambiguity reconciliation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationRequest {
    /// Complete immutable execution correlation.
    pub correlation: ExecutionCorrelation,
    /// Digest of the durable ambiguity evidence.
    pub ambiguity_digest: Sha256Digest,
    /// Stable bounded reason.
    pub reason_code: String,
}

impl ReconciliationRequest {
    /// Validates correlation and reason.
    pub fn validate(&self) -> Result<(), PortError> {
        self.correlation.validate()?;
        if !reason_code(&self.reason_code) {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }
}

/// Durable reconciliation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationDisposition {
    /// The adopted execution was authoritatively found.
    Returned {
        /// Durable disposition identity.
        disposition_id: String,
        /// Existing session.
        session_id: String,
        /// Exact request correlation.
        correlation: ExecutionCorrelation,
        /// Exact ambiguity digest.
        ambiguity_digest: Sha256Digest,
        /// Durable disposition time.
        recorded_at: time::OffsetDateTime,
    },
    /// Every resource capable of satisfying the correlation was fenced.
    Fenced {
        /// Durable disposition identity.
        disposition_id: String,
        /// Opaque fence token.
        fence_token: String,
        /// Exact request correlation.
        correlation: ExecutionCorrelation,
        /// Exact ambiguity digest.
        ambiguity_digest: Sha256Digest,
        /// Durable disposition time.
        recorded_at: time::OffsetDateTime,
    },
    /// No authoritative reconciliation outcome exists.
    Unresolved,
}

impl ReconciliationDisposition {
    /// Validates an outcome against its exact request.
    pub fn validate_for(&self, request: &ReconciliationRequest) -> Result<(), PortError> {
        let (disposition_id, opaque, correlation, ambiguity_digest, recorded_at) = match self {
            Self::Returned {
                disposition_id,
                session_id,
                correlation,
                ambiguity_digest,
                recorded_at,
            } => (
                disposition_id,
                session_id,
                correlation,
                ambiguity_digest,
                recorded_at,
            ),
            Self::Fenced {
                disposition_id,
                fence_token,
                correlation,
                ambiguity_digest,
                recorded_at,
            } => (
                disposition_id,
                fence_token,
                correlation,
                ambiguity_digest,
                recorded_at,
            ),
            Self::Unresolved => return Ok(()),
        };
        bounded(disposition_id).map_err(|_| PortError::InvalidResponse)?;
        bounded(opaque).map_err(|_| PortError::InvalidResponse)?;
        if correlation != &request.correlation
            || ambiguity_digest != &request.ambiguity_digest
            || !utc(*recorded_at)
            || *recorded_at < request.correlation.created_at
        {
            return Err(PortError::CorrelationMismatch);
        }
        Ok(())
    }
}

/// Current session state with complete execution correlation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// Stable opaque session identity.
    pub session_id: String,
    /// Exact adoption correlation.
    pub correlation: ExecutionCorrelation,
    /// Optional terminal ledger state, never cancellation evidence.
    pub terminal_state: Option<String>,
}

impl SessionSnapshot {
    /// Validates bounded snapshot metadata.
    pub fn validate(&self) -> Result<(), PortError> {
        bounded(&self.session_id).map_err(|_| PortError::InvalidResponse)?;
        self.correlation
            .validate()
            .map_err(|_| PortError::InvalidResponse)?;
        if self
            .terminal_state
            .as_deref()
            .is_some_and(|value| bounded(value).is_err())
        {
            return Err(PortError::InvalidResponse);
        }
        Ok(())
    }
}

/// Cursor for ordered session events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCursor {
    /// Stable opaque session identity.
    pub session_id: String,
    /// Last durably consumed sequence.
    pub after_sequence: u64,
}

impl EventCursor {
    /// Validates session and safe-integer bounds.
    pub fn validate(&self) -> Result<(), PortError> {
        bounded(&self.session_id)?;
        if self.after_sequence > MAX_SAFE_INTEGER {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }
}

/// One ordered Coven event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CovenEvent {
    /// Monotonic sequence.
    pub sequence: u64,
    /// Digest of the complete event.
    pub event_digest: Sha256Digest,
    /// Optional raw terminal ledger state.
    pub terminal_state: Option<String>,
}

/// One ordered event page and its continuation cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPage {
    /// Strictly ordered events after the input cursor.
    pub events: Vec<CovenEvent>,
    /// Cursor after the returned page.
    pub next_cursor: EventCursor,
}

impl EventPage {
    /// Validates ordering and cursor consistency.
    pub fn validate_for(&self, cursor: &EventCursor) -> Result<(), PortError> {
        self.next_cursor
            .validate()
            .map_err(|_| PortError::InvalidResponse)?;
        if self.next_cursor.session_id != cursor.session_id || self.events.len() > MAX_ARTIFACTS {
            return Err(PortError::CorrelationMismatch);
        }
        let mut previous = cursor.after_sequence;
        for event in &self.events {
            if event.sequence <= previous || event.sequence > MAX_SAFE_INTEGER {
                return Err(PortError::InvalidResponse);
            }
            if event
                .terminal_state
                .as_deref()
                .is_some_and(|value| bounded(value).is_err())
            {
                return Err(PortError::InvalidResponse);
            }
            previous = event.sequence;
        }
        if self.next_cursor.after_sequence != previous {
            return Err(PortError::CorrelationMismatch);
        }
        Ok(())
    }
}

/// Content-addressed payload metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentAddressedReference {
    /// SHA-256 digest of exact bytes.
    pub digest: Sha256Digest,
    /// Strict lowercase media type.
    pub media_type: String,
    /// Exact nonzero payload size.
    pub size_bytes: u64,
    /// Last instant at which content may be retrieved.
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: time::OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentAddressedReferenceWire {
    digest: Sha256Digest,
    media_type: String,
    size_bytes: u64,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
}

impl TryFrom<ContentAddressedReferenceWire> for ContentAddressedReference {
    type Error = PortError;

    fn try_from(wire: ContentAddressedReferenceWire) -> Result<Self, Self::Error> {
        let value = Self {
            digest: wire.digest,
            media_type: wire.media_type,
            size_bytes: wire.size_bytes,
            expires_at: wire.expires_at,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ContentAddressedReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ContentAddressedReferenceWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl ContentAddressedReference {
    /// Constructs metadata from exact payload bytes.
    pub fn for_bytes(
        media_type: impl Into<String>,
        bytes: &[u8],
        expires_at: time::OffsetDateTime,
    ) -> Result<Self, PortError> {
        let size_bytes = u64::try_from(bytes.len()).map_err(|_| PortError::InvalidRequest)?;
        let value = Self {
            digest: raw_digest(bytes),
            media_type: media_type.into(),
            size_bytes,
            expires_at,
        };
        value.validate()?;
        Ok(value)
    }

    /// Validates metadata only; this does not attest payload bytes.
    pub fn validate(&self) -> Result<(), PortError> {
        validate_media_type(&self.media_type)?;
        if self.size_bytes == 0 || self.size_bytes > MAX_CONTENT_SIZE_BYTES || !utc(self.expires_at)
        {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }

    /// Attests exact payload length and digest.
    pub fn validate_payload(&self, bytes: &[u8]) -> Result<(), PortError> {
        self.validate()?;
        if usize::try_from(self.size_bytes).ok() != Some(bytes.len())
            || !constant_time_equal(
                raw_digest(bytes).as_str().as_bytes(),
                self.digest.as_str().as_bytes(),
            )
        {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }

    /// Attests payload bytes and rejects retrieval after expiry.
    pub fn validate_payload_at(
        &self,
        bytes: &[u8],
        at: time::OffsetDateTime,
    ) -> Result<(), PortError> {
        self.validate_payload(bytes)?;
        if !utc(at) || at > self.expires_at {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }
}

/// One result artifact bound to a session and execution correlation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactReference {
    /// Unique bounded artifact identity.
    pub artifact_id: String,
    /// Exact result session.
    pub session_id: String,
    /// Exact adoption correlation.
    pub correlation: ExecutionCorrelation,
    /// Content-addressed bytes.
    pub content: ContentAddressedReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactReferenceWire {
    artifact_id: String,
    session_id: String,
    correlation: ExecutionCorrelation,
    content: ContentAddressedReference,
}

impl TryFrom<ArtifactReferenceWire> for ArtifactReference {
    type Error = PortError;

    fn try_from(wire: ArtifactReferenceWire) -> Result<Self, Self::Error> {
        let value = Self {
            artifact_id: wire.artifact_id,
            session_id: wire.session_id,
            correlation: wire.correlation,
            content: wire.content,
        };
        bounded(&value.artifact_id)?;
        bounded(&value.session_id)?;
        value.correlation.validate()?;
        value.content.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ArtifactReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ArtifactReferenceWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Complete content-addressed result and artifact references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultBundle {
    /// Exact Coven session.
    pub session_id: String,
    /// Exact adoption correlation.
    pub correlation: ExecutionCorrelation,
    /// Primary result bytes.
    pub result: ContentAddressedReference,
    /// Ordered unique artifact references.
    pub artifacts: Vec<ArtifactReference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultBundleWire {
    session_id: String,
    correlation: ExecutionCorrelation,
    result: ContentAddressedReference,
    artifacts: Vec<ArtifactReference>,
}

impl TryFrom<ResultBundleWire> for ResultBundle {
    type Error = PortError;

    fn try_from(wire: ResultBundleWire) -> Result<Self, Self::Error> {
        let value = Self {
            session_id: wire.session_id,
            correlation: wire.correlation,
            result: wire.result,
            artifacts: wire.artifacts,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ResultBundle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ResultBundleWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl ResultBundle {
    /// Validates complete correlation, association, uniqueness, and lifetimes.
    pub fn validate(&self) -> Result<(), PortError> {
        bounded(&self.session_id)?;
        self.correlation.validate()?;
        self.result.validate()?;
        if self.result.expires_at <= self.correlation.created_at
            || self.result.expires_at > self.correlation.valid_until
            || self.artifacts.len() > MAX_ARTIFACTS
        {
            return Err(PortError::InvalidRequest);
        }
        let mut artifact_ids = HashSet::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            bounded(&artifact.artifact_id)?;
            if !artifact_ids.insert(artifact.artifact_id.as_str()) {
                return Err(PortError::InvalidRequest);
            }
            if artifact.session_id != self.session_id
                || artifact.correlation != self.correlation
                || artifact.content.expires_at <= self.correlation.created_at
                || artifact.content.expires_at > self.result.expires_at
                || artifact.content.expires_at > self.correlation.valid_until
            {
                return Err(PortError::CorrelationMismatch);
            }
            artifact.content.validate()?;
        }
        Ok(())
    }
}

/// Construction-closed termination request proven durable by the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationRequest {
    persisted_binding: ExecutionBinding,
    reason_code: String,
}

impl TerminationRequest {
    fn from_persisted_binding(persisted_binding: ExecutionBinding) -> Result<Self, ContractError> {
        validate_termination_requested_candidate(&persisted_binding)?;
        let reason_code = persisted_binding
            .termination_reason_code
            .clone()
            .ok_or(ContractError::CancellationEvidenceMismatch)?;
        Ok(Self {
            persisted_binding,
            reason_code,
        })
    }

    /// Exact persisted termination-requested binding.
    pub fn binding(&self) -> &ExecutionBinding {
        &self.persisted_binding
    }

    /// Validated stable termination reason.
    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }
}

/// Validated authoritative termination response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationDisposition {
    /// O5 authority acknowledged termination or prior authoritative completion.
    Acknowledged {
        /// Core-owned authority evidence.
        evidence: CancellationAcknowledgementEvidence,
    },
    /// O5 could not provide authoritative acknowledgement.
    Unresolved {
        /// Core-owned durable unresolved evidence.
        evidence: CancellationUnresolvedEvidence,
    },
}

/// Narrow durable boundary used by termination coordination.
pub trait TerminationPersistence {
    /// Adapter-owned payload-free failure.
    type Error;

    /// Durably persists or exactly replays the requested revision.
    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>>;

    /// Durably persists or exactly replays the validated outcome revision.
    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>>;
}

/// Persists a request before dispatch and its validated outcome before success.
pub async fn persist_then_terminate<S, P>(
    persistence: &mut S,
    port: &P,
    requested: ExecutionBinding,
) -> Result<TerminationDisposition, TerminationDispatchError<S::Error>>
where
    S: TerminationPersistence,
    P: CovenPort + ?Sized,
{
    validate_termination_requested_candidate(&requested)
        .map_err(TerminationDispatchError::Contract)?;
    let expected_bytes = canonical_bytes(&requested).map_err(TerminationDispatchError::Contract)?;
    let persisted_bytes = persistence
        .persist_requested(requested.clone())
        .map_err(|failure| match failure {
            TerminationPersistenceFailure::Conflict(error) => {
                TerminationDispatchError::RevisionConflict(error)
            }
            TerminationPersistenceFailure::Write(error) => {
                TerminationDispatchError::RequestPersistence(error)
            }
        })?;
    if persisted_bytes != expected_bytes {
        return Err(TerminationDispatchError::PersistedBindingMismatch);
    }
    let request = TerminationRequest::from_persisted_binding(requested.clone())
        .map_err(TerminationDispatchError::Contract)?;
    let disposition = port
        .terminate(request)
        .await
        .map_err(TerminationDispatchError::Port)?;
    let outcome =
        derive_termination_outcome_revision(&requested, &disposition).map_err(|error| {
            if error == ContractError::CancellationEvidenceMismatch {
                TerminationDispatchError::OutcomeEvidenceMismatch
            } else {
                TerminationDispatchError::Contract(error)
            }
        })?;
    let expected_outcome_bytes =
        canonical_bytes(&outcome).map_err(TerminationDispatchError::Contract)?;
    let persisted_outcome_bytes =
        persistence
            .persist_outcome(outcome)
            .map_err(|failure| match failure {
                TerminationPersistenceFailure::Conflict(error) => {
                    TerminationDispatchError::RevisionConflict(error)
                }
                TerminationPersistenceFailure::Write(error) => {
                    TerminationDispatchError::OutcomePersistenceIndeterminate(error)
                }
            })?;
    if persisted_outcome_bytes != expected_outcome_bytes {
        return Err(TerminationDispatchError::PersistedOutcomeMismatch);
    }
    Ok(disposition)
}

/// Validates response evidence and derives the sole legal next outcome revision.
pub fn derive_termination_outcome_revision(
    persisted_requested: &ExecutionBinding,
    disposition: &TerminationDisposition,
) -> Result<ExecutionBinding, ContractError> {
    validate_termination_requested_candidate(persisted_requested)?;
    let termination = persisted_requested
        .termination_request
        .as_ref()
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    let session = persisted_requested
        .coven_session_id
        .as_deref()
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    let mut outcome = persisted_requested.clone();
    outcome.revision = outcome
        .revision
        .checked_add(1)
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    outcome.previous_revision_digest = Some(digest(persisted_requested)?);
    let evidence_at = match disposition {
        TerminationDisposition::Acknowledged { evidence } => {
            evidence
                .validate()
                .map_err(|_| ContractError::CancellationEvidenceMismatch)?;
            if evidence.termination_request_id != termination.termination_request_id
                || evidence.session_id != session
                || evidence.execution_request_id != persisted_requested.request_id
                || evidence.execution_request_digest != persisted_requested.request_digest
                || digest_is_zero(&evidence.authority_evidence_digest)
                || !utc(evidence.acknowledged_at)
                || evidence.acknowledged_at < termination.created_at
                || evidence.acknowledged_at > termination.valid_until
            {
                return Err(ContractError::CancellationEvidenceMismatch);
            }
            outcome.cancellation_state = match evidence.kind {
                CancellationAcknowledgementKind::Terminated => {
                    CancellationState::AcknowledgedTerminated
                }
                CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal => {
                    CancellationState::AcknowledgedAlreadyTerminal
                }
            };
            outcome.cancellation_acknowledgement = Some(evidence.clone());
            outcome.cancellation_unresolved = None;
            evidence.acknowledged_at
        }
        TerminationDisposition::Unresolved { evidence } => {
            evidence
                .validate()
                .map_err(|_| ContractError::CancellationEvidenceMismatch)?;
            if evidence.termination_request_id != termination.termination_request_id
                || evidence.session_id != session
                || evidence.execution_request_id != persisted_requested.request_id
                || evidence.execution_request_digest != persisted_requested.request_digest
                || !utc(evidence.recorded_at)
                || evidence.recorded_at < termination.created_at
                || evidence.recorded_at > termination.valid_until
            {
                return Err(ContractError::CancellationEvidenceMismatch);
            }
            outcome.cancellation_state = CancellationState::TerminationUnknown;
            outcome.cancellation_acknowledgement = None;
            outcome.cancellation_unresolved = Some(evidence.clone());
            evidence.recorded_at
        }
    };
    let minimum_revision_time = persisted_requested
        .revision_created_at
        .checked_add(time::Duration::nanoseconds(1))
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    outcome.revision_created_at = evidence_at.max(minimum_revision_time);
    outcome
        .validate()
        .map_err(|_| ContractError::CancellationEvidenceMismatch)?;
    Ok(outcome)
}

fn validate_termination_requested_candidate(
    requested: &ExecutionBinding,
) -> Result<(), ContractError> {
    requested.validate()?;
    let termination = requested
        .termination_request
        .as_ref()
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    let session = requested
        .coven_session_id
        .as_deref()
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    let reason = requested
        .termination_reason_code
        .as_deref()
        .ok_or(ContractError::CancellationEvidenceMismatch)?;
    if requested.revision < 2
        || requested.previous_revision_digest.is_none()
        || requested.cancellation_state != CancellationState::TerminationRequested
        || !bounded_string(session, MAX_STRING_BYTES)
        || !reason_code(reason)
        || termination.termination_request_id == requested.request_id
        || termination.created_at < requested.request_created_at
        || !utc(requested.revision_created_at)
        || !utc(requested.request_created_at)
        || !utc(requested.request_valid_until)
        || !utc(termination.created_at)
        || !utc(termination.valid_until)
    {
        return Err(ContractError::CancellationEvidenceMismatch);
    }
    Ok(())
}

/// Behavior-level Coven execution boundary.
#[async_trait::async_trait]
pub trait CovenPort: Send + Sync {
    /// Negotiates an exact behavior contract.
    async fn negotiate(&self, request: NegotiateRequest) -> Result<CapabilityProfile, PortError>;
    /// Stably adopts a complete digest-attested request.
    async fn adopt(&self, request: AdoptionRequest) -> Result<AdoptionDisposition, PortError>;
    /// Looks up the durable disposition for a stable request identity.
    async fn lookup(&self, request_id: &RequestId) -> Result<AdoptionDisposition, PortError>;
    /// Reconciles and fences ambiguous adoption.
    async fn reconcile(
        &self,
        request: ReconciliationRequest,
    ) -> Result<ReconciliationDisposition, PortError>;
    /// Inspects a session snapshot.
    async fn inspect(&self, session_id: &str) -> Result<SessionSnapshot, PortError>;
    /// Reads one ordered event page.
    async fn events(&self, cursor: EventCursor) -> Result<EventPage, PortError>;
    /// Reads content-addressed result metadata.
    async fn result(&self, session_id: &str) -> Result<ResultBundle, PortError>;
    /// Requests termination using a construction-closed durable request.
    async fn terminate(
        &self,
        request: TerminationRequest,
    ) -> Result<TerminationDisposition, PortError>;
}

fn validate_artifact_bindings(bindings: &[ExecutionArtifactBinding]) -> Result<(), PortError> {
    if bindings.len() > MAX_ARTIFACTS {
        return Err(PortError::InvalidRequest);
    }
    let mut ids = HashSet::with_capacity(bindings.len());
    for binding in bindings {
        binding.validate()?;
        if !ids.insert(binding.artifact_id.as_str()) {
            return Err(PortError::InvalidRequest);
        }
    }
    Ok(())
}

fn validate_window(
    created_at: time::OffsetDateTime,
    valid_until: time::OffsetDateTime,
) -> Result<(), PortError> {
    if !utc(created_at) || !utc(valid_until) || valid_until <= created_at {
        Err(PortError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn validate_absolute_path(path: &str) -> Result<(), PortError> {
    if path == "/" {
        return Ok(());
    }
    if !bounded_string(path, 4096)
        || !path.starts_with('/')
        || path.contains('\0')
        || path.contains("//")
        || (path.len() > 1 && path.ends_with('/'))
        || path
            .split('/')
            .skip(1)
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        Err(PortError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn path_is_within(root: &str, candidate: &str) -> bool {
    root == "/"
        || candidate == root
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_media_type(media_type: &str) -> Result<(), PortError> {
    let mut parts = media_type.split('/');
    let Some(major) = parts.next() else {
        return Err(PortError::InvalidRequest);
    };
    let Some(minor) = parts.next() else {
        return Err(PortError::InvalidRequest);
    };
    let valid = |component: &str| {
        !component.is_empty()
            && component.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&^_.+-".contains(&byte)
            })
    };
    if media_type.len() > MAX_STRING_BYTES
        || parts.next().is_some()
        || !valid(major)
        || !valid(minor)
    {
        Err(PortError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn bounded(value: &str) -> Result<(), PortError> {
    if bounded_string(value, MAX_STRING_BYTES) {
        Ok(())
    } else {
        Err(PortError::InvalidRequest)
    }
}

fn bounded_string(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum
}

fn stable_token(value: &str, maximum: usize) -> bool {
    bounded_string(value, maximum)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn reason_code(value: &str) -> bool {
    value.len() <= 128
        && value.split('_').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
}

fn utc(value: time::OffsetDateTime) -> bool {
    value.offset() == time::UtcOffset::UTC
}

fn raw_digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let value = format!("sha256:{digest:x}");
    match Sha256Digest::parse(&value) {
        Ok(value) => value,
        Err(_) => unreachable!("SHA-256 formatting is canonical"),
    }
}

fn digest_is_zero(digest: &Sha256Digest) -> bool {
    digest.as_str().as_bytes()[7..]
        .iter()
        .all(|byte| *byte == b'0')
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}
