//! Execution binding and cancellation evidence contracts.
#![allow(missing_docs)]

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::contracts::{
    ContractError, RecordKind, SchemaKind, SchemaVersion, VersionedRecord, bounded,
    optional_bounded, reason_code, require_id, require_schema, safe_integer,
};
use crate::digest::Sha256Digest;
use crate::id::{RecordId, RequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionState {
    NotSubmitted,
    Submitting,
    Adopted,
    ProvenNotAdopted,
    AdoptionUnknown,
    Fenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationState {
    NotRequested,
    TerminationRequested,
    AcknowledgedTerminated,
    AcknowledgedAlreadyTerminal,
    TerminationUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationAcknowledgementKind {
    Terminated,
    AlreadyAuthoritativelyTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancellationAcknowledgementEvidence {
    pub acknowledgement_id: String,
    pub termination_request_id: RequestId,
    pub session_id: String,
    pub execution_request_id: RequestId,
    pub execution_request_digest: Sha256Digest,
    pub kind: CancellationAcknowledgementKind,
    pub authority_evidence_digest: Sha256Digest,
    #[serde(with = "time::serde::rfc3339")]
    pub acknowledged_at: time::OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcknowledgementWire {
    acknowledgement_id: String,
    termination_request_id: RequestId,
    session_id: String,
    execution_request_id: RequestId,
    execution_request_digest: Sha256Digest,
    kind: CancellationAcknowledgementKind,
    authority_evidence_digest: Sha256Digest,
    #[serde(with = "time::serde::rfc3339")]
    acknowledged_at: time::OffsetDateTime,
}

impl TryFrom<AcknowledgementWire> for CancellationAcknowledgementEvidence {
    type Error = ContractError;
    fn try_from(w: AcknowledgementWire) -> Result<Self, Self::Error> {
        let value = Self {
            acknowledgement_id: w.acknowledgement_id,
            termination_request_id: w.termination_request_id,
            session_id: w.session_id,
            execution_request_id: w.execution_request_id,
            execution_request_digest: w.execution_request_digest,
            kind: w.kind,
            authority_evidence_digest: w.authority_evidence_digest,
            acknowledged_at: w.acknowledged_at,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CancellationAcknowledgementEvidence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        AcknowledgementWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl CancellationAcknowledgementEvidence {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::ExecutionBinding;
        bounded(&self.acknowledgement_id, 255, s, "acknowledgement_id")?;
        bounded(&self.session_id, 255, s, "session_id")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancellationUnresolvedEvidence {
    pub disposition_id: String,
    pub termination_request_id: RequestId,
    pub session_id: String,
    pub execution_request_id: RequestId,
    pub execution_request_digest: Sha256Digest,
    pub reason_code: String,
    #[serde(with = "time::serde::rfc3339")]
    pub recorded_at: time::OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnresolvedWire {
    disposition_id: String,
    termination_request_id: RequestId,
    session_id: String,
    execution_request_id: RequestId,
    execution_request_digest: Sha256Digest,
    reason_code: String,
    #[serde(with = "time::serde::rfc3339")]
    recorded_at: time::OffsetDateTime,
}

impl TryFrom<UnresolvedWire> for CancellationUnresolvedEvidence {
    type Error = ContractError;
    fn try_from(w: UnresolvedWire) -> Result<Self, Self::Error> {
        let value = Self {
            disposition_id: w.disposition_id,
            termination_request_id: w.termination_request_id,
            session_id: w.session_id,
            execution_request_id: w.execution_request_id,
            execution_request_digest: w.execution_request_digest,
            reason_code: w.reason_code,
            recorded_at: w.recorded_at,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CancellationUnresolvedEvidence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        UnresolvedWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl CancellationUnresolvedEvidence {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::ExecutionBinding;
        bounded(&self.disposition_id, 255, s, "disposition_id")?;
        bounded(&self.session_id, 255, s, "session_id")?;
        reason_code(&self.reason_code, s, "reason_code")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminationRequestCorrelation {
    pub termination_request_id: RequestId,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub valid_until: time::OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminationWire {
    termination_request_id: RequestId,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    valid_until: time::OffsetDateTime,
}

impl TryFrom<TerminationWire> for TerminationRequestCorrelation {
    type Error = ContractError;
    fn try_from(w: TerminationWire) -> Result<Self, Self::Error> {
        let value = Self {
            termination_request_id: w.termination_request_id,
            created_at: w.created_at,
            valid_until: w.valid_until,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for TerminationRequestCorrelation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TerminationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl TerminationRequestCorrelation {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::ExecutionBinding;
        if self.valid_until <= self.created_at {
            return Err(super::invalid(s, "termination_request.valid_until"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionBinding {
    pub schema_version: SchemaVersion,
    pub attempt_id: RecordId,
    pub revision: u64,
    pub previous_revision_digest: Option<Sha256Digest>,
    #[serde(with = "time::serde::rfc3339")]
    pub revision_created_at: time::OffsetDateTime,
    pub familiar_snapshot_id: RecordId,
    pub project_id: String,
    pub request_id: RequestId,
    pub request_digest: Sha256Digest,
    #[serde(with = "time::serde::rfc3339")]
    pub request_created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub request_valid_until: time::OffsetDateTime,
    pub coven_contract_version: String,
    pub coven_session_id: Option<String>,
    pub adoption_state: AdoptionState,
    pub event_cursor: Option<String>,
    pub cancellation_state: CancellationState,
    pub termination_request: Option<TerminationRequestCorrelation>,
    pub termination_reason_code: Option<String>,
    pub cancellation_acknowledgement: Option<CancellationAcknowledgementEvidence>,
    pub cancellation_unresolved: Option<CancellationUnresolvedEvidence>,
    pub terminal_state: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionWire {
    schema_version: SchemaVersion,
    attempt_id: RecordId,
    revision: u64,
    previous_revision_digest: Option<Sha256Digest>,
    #[serde(with = "time::serde::rfc3339")]
    revision_created_at: time::OffsetDateTime,
    familiar_snapshot_id: RecordId,
    project_id: String,
    request_id: RequestId,
    request_digest: Sha256Digest,
    #[serde(with = "time::serde::rfc3339")]
    request_created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    request_valid_until: time::OffsetDateTime,
    coven_contract_version: String,
    coven_session_id: Option<String>,
    adoption_state: AdoptionState,
    event_cursor: Option<String>,
    cancellation_state: CancellationState,
    termination_request: Option<Value>,
    termination_reason_code: Option<Value>,
    cancellation_acknowledgement: Option<Value>,
    cancellation_unresolved: Option<Value>,
    terminal_state: Option<String>,
}

impl TryFrom<ExecutionWire> for ExecutionBinding {
    type Error = ContractError;

    fn try_from(w: ExecutionWire) -> Result<Self, Self::Error> {
        let termination_request = cancellation_value(w.termination_request)?;
        let termination_reason_code = match w.termination_reason_code {
            Some(Value::String(reason)) => Some(reason),
            Some(_) => return Err(ContractError::CancellationEvidenceMismatch),
            None => None,
        };
        let cancellation_acknowledgement = cancellation_value(w.cancellation_acknowledgement)?;
        let cancellation_unresolved = cancellation_value(w.cancellation_unresolved)?;
        let value = Self {
            schema_version: w.schema_version,
            attempt_id: w.attempt_id,
            revision: w.revision,
            previous_revision_digest: w.previous_revision_digest,
            revision_created_at: w.revision_created_at,
            familiar_snapshot_id: w.familiar_snapshot_id,
            project_id: w.project_id,
            request_id: w.request_id,
            request_digest: w.request_digest,
            request_created_at: w.request_created_at,
            request_valid_until: w.request_valid_until,
            coven_contract_version: w.coven_contract_version,
            coven_session_id: w.coven_session_id,
            adoption_state: w.adoption_state,
            event_cursor: w.event_cursor,
            cancellation_state: w.cancellation_state,
            termination_request,
            termination_reason_code,
            cancellation_acknowledgement,
            cancellation_unresolved,
            terminal_state: w.terminal_state,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ExecutionBinding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ExecutionWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl ExecutionBinding {
    pub(crate) fn decode(value: Value) -> Result<Self, ContractError> {
        let wire: ExecutionWire = serde_json::from_value(value)
            .map_err(|_| super::invalid(SchemaKind::ExecutionBinding, "document"))?;
        wire.try_into()
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::ExecutionBinding;
        require_schema(self.schema_version, s)?;
        require_id(&self.attempt_id, RecordKind::Attempt, s, "attempt_id")?;
        require_id(
            &self.familiar_snapshot_id,
            RecordKind::IdentitySnapshot,
            s,
            "familiar_snapshot_id",
        )?;
        if self.revision == 0 || (self.revision == 1) != self.previous_revision_digest.is_none() {
            return Err(super::invalid(s, "revision"));
        }
        safe_integer(self.revision, s, "revision")?;
        if self.revision_created_at.offset() != time::UtcOffset::UTC {
            return Err(super::invalid(s, "revision_created_at"));
        }
        bounded(&self.project_id, 255, s, "project_id")?;
        bounded(
            &self.coven_contract_version,
            255,
            s,
            "coven_contract_version",
        )?;
        if let Err(error) = optional_bounded(&self.coven_session_id, 255, s, "coven_session_id") {
            return if self.cancellation_state == CancellationState::NotRequested {
                Err(error)
            } else {
                Err(ContractError::CancellationEvidenceMismatch)
            };
        }
        optional_bounded(&self.event_cursor, 255, s, "event_cursor")?;
        optional_bounded(&self.terminal_state, 255, s, "terminal_state")?;
        if self.request_valid_until <= self.request_created_at {
            return Err(super::invalid(s, "request_valid_until"));
        }
        self.validate_cancellation(self.request_created_at)
    }

    fn validate_cancellation(
        &self,
        request_created: time::OffsetDateTime,
    ) -> Result<(), ContractError> {
        let empty = self.termination_request.is_none()
            && self.termination_reason_code.is_none()
            && self.cancellation_acknowledgement.is_none()
            && self.cancellation_unresolved.is_none();
        if self.cancellation_state == CancellationState::NotRequested {
            return if empty {
                Ok(())
            } else {
                Err(ContractError::CancellationEvidenceMismatch)
            };
        }
        let correlation = self
            .termination_request
            .as_ref()
            .ok_or(ContractError::CancellationEvidenceMismatch)?;
        cancellation_result(correlation.validate())?;
        let reason = self
            .termination_reason_code
            .as_deref()
            .ok_or(ContractError::CancellationEvidenceMismatch)?;
        cancellation_result(reason_code(
            reason,
            SchemaKind::ExecutionBinding,
            "termination_reason_code",
        ))?;
        let created = correlation.created_at;
        let valid_until = correlation.valid_until;
        if created < request_created || correlation.termination_request_id == self.request_id {
            return Err(ContractError::CancellationEvidenceMismatch);
        }
        match self.cancellation_state {
            CancellationState::TerminationRequested => {
                if self.cancellation_acknowledgement.is_none()
                    && self.cancellation_unresolved.is_none()
                {
                    Ok(())
                } else {
                    Err(ContractError::CancellationEvidenceMismatch)
                }
            }
            CancellationState::AcknowledgedTerminated => self.validate_ack(
                CancellationAcknowledgementKind::Terminated,
                created,
                valid_until,
            ),
            CancellationState::AcknowledgedAlreadyTerminal => self.validate_ack(
                CancellationAcknowledgementKind::AlreadyAuthoritativelyTerminal,
                created,
                valid_until,
            ),
            CancellationState::TerminationUnknown => {
                if self.cancellation_acknowledgement.is_some() {
                    return Err(ContractError::CancellationEvidenceMismatch);
                }
                let evidence = self
                    .cancellation_unresolved
                    .as_ref()
                    .ok_or(ContractError::CancellationEvidenceMismatch)?;
                cancellation_result(evidence.validate())?;
                self.validate_evidence_bindings(
                    &evidence.termination_request_id,
                    &evidence.session_id,
                    &evidence.execution_request_id,
                    &evidence.execution_request_digest,
                )?;
                in_window(evidence.recorded_at, created, valid_until)
            }
            CancellationState::NotRequested => unreachable!(),
        }
    }

    fn validate_ack(
        &self,
        kind: CancellationAcknowledgementKind,
        created: time::OffsetDateTime,
        valid_until: time::OffsetDateTime,
    ) -> Result<(), ContractError> {
        if self.cancellation_unresolved.is_some() {
            return Err(ContractError::CancellationEvidenceMismatch);
        }
        let evidence = self
            .cancellation_acknowledgement
            .as_ref()
            .ok_or(ContractError::CancellationEvidenceMismatch)?;
        cancellation_result(evidence.validate())?;
        if evidence.kind != kind {
            return Err(ContractError::CancellationEvidenceMismatch);
        }
        self.validate_evidence_bindings(
            &evidence.termination_request_id,
            &evidence.session_id,
            &evidence.execution_request_id,
            &evidence.execution_request_digest,
        )?;
        in_window(evidence.acknowledged_at, created, valid_until)
    }

    fn validate_evidence_bindings(
        &self,
        termination: &RequestId,
        session: &str,
        execution: &RequestId,
        digest: &Sha256Digest,
    ) -> Result<(), ContractError> {
        let expected_termination = self
            .termination_request
            .as_ref()
            .map(|v| &v.termination_request_id);
        if expected_termination != Some(termination)
            || self.coven_session_id.as_deref() != Some(session)
            || &self.request_id != execution
            || &self.request_digest != digest
        {
            Err(ContractError::CancellationEvidenceMismatch)
        } else {
            Ok(())
        }
    }
}

fn cancellation_value<T: serde::de::DeserializeOwned>(
    value: Option<Value>,
) -> Result<Option<T>, ContractError> {
    value
        .map(|value| {
            serde_json::from_value(value).map_err(|_| ContractError::CancellationEvidenceMismatch)
        })
        .transpose()
}

fn cancellation_result<T>(result: Result<T, ContractError>) -> Result<T, ContractError> {
    result.map_err(|_| ContractError::CancellationEvidenceMismatch)
}

fn in_window(
    at: time::OffsetDateTime,
    created: time::OffsetDateTime,
    valid_until: time::OffsetDateTime,
) -> Result<(), ContractError> {
    if at < created || at > valid_until {
        Err(ContractError::CancellationEvidenceMismatch)
    } else {
        Ok(())
    }
}

impl VersionedRecord for ExecutionBinding {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.attempt_id
    }
}
