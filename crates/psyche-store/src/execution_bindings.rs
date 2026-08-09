use psyche_core::contracts::execution::{
    CancellationAcknowledgementEvidence, CancellationState, CancellationUnresolvedEvidence,
    TerminationRequestCorrelation,
};
use psyche_core::contracts::{
    CanonicalDocument, ContractError, ExecutionBinding, RecordKind, SchemaKind,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::RecordId;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use time::format_description::well_known::Rfc3339;

use crate::records::InsertStatus;
use crate::{Store, StoreError, records};

struct StoredRevision {
    attempt_id: String,
    revision: i64,
    schema_version: String,
    digest: String,
    previous_revision_digest: Option<String>,
    canonical_json: Vec<u8>,
    created_at: String,
}

struct ValidatedRevision {
    binding: ExecutionBinding,
    digest: Sha256Digest,
    canonical_json: Vec<u8>,
}

impl Store {
    /// Returns every immutable revision for an execution attempt in order.
    pub fn execution_binding_revisions(
        &self,
        attempt_id: &RecordId,
    ) -> Result<Vec<ExecutionBinding>, StoreError> {
        revisions(&self.connection, attempt_id)
    }
}

pub(crate) fn insert(
    connection: &mut Connection,
    binding: &ExecutionBinding,
) -> Result<InsertStatus, StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let status = insert_in_transaction(&transaction, binding)?;
    transaction.commit()?;
    Ok(status)
}

/// Package-private production primitive for appending within an immediate transaction.
///
/// The caller owns begin/commit; validation and ledger semantics are shared with `insert`.
pub(crate) fn insert_in_transaction(
    transaction: &Transaction<'_>,
    binding: &ExecutionBinding,
) -> Result<InsertStatus, StoreError> {
    binding.validate()?;
    let canonical_json = canonical_bytes(binding)?;
    let revision_digest = digest(binding)?;
    let sql_revision = sql_revision(binding.revision)?;
    let created_at = binding
        .revision_created_at
        .format(&Rfc3339)
        .map_err(|_| StoreError::Contract(ContractError::CanonicalizationFailed))?;
    let stored = load_stored_revisions(transaction, &binding.attempt_id)?;
    let history = validate_revision_chain(stored, &binding.attempt_id)?;

    if let Some(existing) = history
        .iter()
        .find(|revision| revision.binding.revision == binding.revision)
    {
        if existing.canonical_json == canonical_json {
            return Ok(InsertStatus::AlreadyPresent);
        }
        return Err(revision_conflict(binding));
    }

    validate_next_revision(binding, &history)?;
    transaction.execute(
        "
        INSERT INTO execution_binding_revisions (
            attempt_id,
            revision,
            schema_version,
            digest,
            previous_revision_digest,
            canonical_json,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            binding.attempt_id.as_str(),
            sql_revision,
            binding.schema_version.to_string(),
            revision_digest.as_str(),
            binding
                .previous_revision_digest
                .as_ref()
                .map(|value| value.as_str()),
            canonical_json,
            created_at,
        ],
    )?;
    Ok(InsertStatus::Inserted)
}

pub(crate) fn revisions(
    connection: &Connection,
    attempt_id: &RecordId,
) -> Result<Vec<ExecutionBinding>, StoreError> {
    records::validate_kind_id(SchemaKind::ExecutionBinding, attempt_id)?;
    let stored = load_stored_revisions(connection, attempt_id)?;
    Ok(validate_revision_chain(stored, attempt_id)?
        .into_iter()
        .map(|revision| revision.binding)
        .collect())
}

pub(crate) fn latest_canonical_bytes(
    connection: &Connection,
    attempt_id: &RecordId,
) -> Result<Option<Vec<u8>>, StoreError> {
    records::validate_kind_id(SchemaKind::ExecutionBinding, attempt_id)?;
    let stored = load_stored_revisions(connection, attempt_id)?;
    Ok(validate_revision_chain(stored, attempt_id)?
        .pop()
        .map(|revision| revision.canonical_json))
}

pub(crate) fn validate_all(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT attempt_id
        FROM execution_binding_revisions
        ORDER BY attempt_id
        ",
    )?;
    let attempt_ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)?;
    for attempt_id in attempt_ids {
        let attempt_id = RecordId::parse(RecordKind::Attempt, &attempt_id)
            .map_err(|_| StoreError::DatabaseCorruption)?;
        let stored = load_stored_revisions(connection, &attempt_id)?;
        validate_revision_chain(stored, &attempt_id)?;
    }
    Ok(())
}

fn load_stored_revisions(
    connection: &Connection,
    attempt_id: &RecordId,
) -> Result<Vec<StoredRevision>, StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT
            attempt_id,
            revision,
            schema_version,
            digest,
            previous_revision_digest,
            canonical_json,
            created_at
        FROM execution_binding_revisions
        WHERE attempt_id = ?1
        ORDER BY revision
        ",
    )?;
    statement
        .query_map([attempt_id.as_str()], |row| {
            Ok(StoredRevision {
                attempt_id: row.get(0)?,
                revision: row.get(1)?,
                schema_version: row.get(2)?,
                digest: row.get(3)?,
                previous_revision_digest: row.get(4)?,
                canonical_json: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn validate_revision_chain(
    stored: Vec<StoredRevision>,
    expected_attempt_id: &RecordId,
) -> Result<Vec<ValidatedRevision>, StoreError> {
    let mut validated = Vec::with_capacity(stored.len());
    for row in stored {
        let binding = decode_stored_binding(&row.canonical_json)?;
        let canonical_json =
            canonical_bytes(&binding).map_err(|_| StoreError::DatabaseCorruption)?;
        let revision_digest = digest(&binding).map_err(|_| StoreError::DatabaseCorruption)?;
        let revision = u64::try_from(row.revision).map_err(|_| StoreError::DatabaseCorruption)?;
        let created_at = binding
            .revision_created_at
            .format(&Rfc3339)
            .map_err(|_| StoreError::DatabaseCorruption)?;
        if row.attempt_id != expected_attempt_id.as_str()
            || binding.attempt_id != *expected_attempt_id
            || revision != binding.revision
            || row.schema_version != binding.schema_version.to_string()
            || row.digest != revision_digest.as_str()
            || row.previous_revision_digest.as_deref()
                != binding
                    .previous_revision_digest
                    .as_ref()
                    .map(Sha256Digest::as_str)
            || row.canonical_json != canonical_json
            || row.created_at != created_at
        {
            return Err(StoreError::DatabaseCorruption);
        }
        validated.push(ValidatedRevision {
            binding,
            digest: revision_digest,
            canonical_json,
        });
    }

    let Some(initial) = validated.first() else {
        return Ok(validated);
    };
    for (index, current) in validated.iter().enumerate() {
        let expected_revision = u64::try_from(index)
            .map_err(|_| StoreError::DatabaseCorruption)?
            .checked_add(1)
            .ok_or(StoreError::DatabaseCorruption)?;
        if current.binding.revision != expected_revision {
            return Err(StoreError::DatabaseCorruption);
        }
        if index == 0 {
            if current.binding.previous_revision_digest.is_some() {
                return Err(StoreError::DatabaseCorruption);
            }
            continue;
        }

        let previous = &validated[index - 1];
        if current.binding.previous_revision_digest.as_ref() != Some(&previous.digest)
            || current.binding.revision_created_at <= previous.binding.revision_created_at
            || !frozen_execution_fields_match(&initial.binding, &current.binding)
            || !session_binding_is_append_only(&previous.binding, &current.binding)
            || !termination_binding_is_append_only(&previous.binding, &current.binding)
            || !cancellation_binding_is_append_only(&previous.binding, &current.binding)
        {
            return Err(StoreError::DatabaseCorruption);
        }
    }
    Ok(validated)
}

fn validate_next_revision(
    binding: &ExecutionBinding,
    history: &[ValidatedRevision],
) -> Result<(), StoreError> {
    let Some(latest) = history.last() else {
        if binding.revision == 1 && binding.previous_revision_digest.is_none() {
            return Ok(());
        }
        return Err(revision_conflict(binding));
    };
    let expected_revision = latest
        .binding
        .revision
        .checked_add(1)
        .ok_or_else(|| revision_conflict(binding))?;
    let initial = &history[0].binding;
    if binding.revision != expected_revision
        || binding.previous_revision_digest.as_ref() != Some(&latest.digest)
        || binding.revision_created_at <= latest.binding.revision_created_at
        || !frozen_execution_fields_match(initial, binding)
        || !session_binding_is_append_only(&latest.binding, binding)
        || !termination_binding_is_append_only(&latest.binding, binding)
        || !cancellation_binding_is_append_only(&latest.binding, binding)
    {
        return Err(revision_conflict(binding));
    }
    Ok(())
}

fn frozen_execution_fields_match(initial: &ExecutionBinding, candidate: &ExecutionBinding) -> bool {
    initial.attempt_id == candidate.attempt_id
        && initial.familiar_snapshot_id == candidate.familiar_snapshot_id
        && initial.project_id == candidate.project_id
        && initial.request_id == candidate.request_id
        && initial.request_digest == candidate.request_digest
        && canonical_timestamps_match(initial.request_created_at, candidate.request_created_at)
        && canonical_timestamps_match(initial.request_valid_until, candidate.request_valid_until)
        && initial.coven_contract_version == candidate.coven_contract_version
}

fn canonical_timestamps_match(
    previous: time::OffsetDateTime,
    candidate: time::OffsetDateTime,
) -> bool {
    previous == candidate && previous.offset() == candidate.offset()
}

fn session_binding_is_append_only(latest: &ExecutionBinding, candidate: &ExecutionBinding) -> bool {
    match (&latest.coven_session_id, &candidate.coven_session_id) {
        (None, _) => true,
        (Some(previous), Some(candidate)) => previous == candidate,
        (Some(_), None) => false,
    }
}

fn termination_binding_is_append_only(
    latest: &ExecutionBinding,
    candidate: &ExecutionBinding,
) -> bool {
    match &latest.termination_request {
        None => true,
        Some(previous) => {
            candidate
                .termination_request
                .as_ref()
                .is_some_and(|candidate| termination_requests_match(previous, candidate))
                && candidate.termination_reason_code == latest.termination_reason_code
        }
    }
}

fn termination_requests_match(
    previous: &TerminationRequestCorrelation,
    candidate: &TerminationRequestCorrelation,
) -> bool {
    previous == candidate
        && canonical_timestamps_match(previous.created_at, candidate.created_at)
        && canonical_timestamps_match(previous.valid_until, candidate.valid_until)
}

fn cancellation_binding_is_append_only(
    latest: &ExecutionBinding,
    candidate: &ExecutionBinding,
) -> bool {
    match latest.cancellation_state {
        CancellationState::NotRequested => matches!(
            candidate.cancellation_state,
            CancellationState::NotRequested | CancellationState::TerminationRequested
        ),
        CancellationState::TerminationRequested => matches!(
            candidate.cancellation_state,
            CancellationState::TerminationRequested
                | CancellationState::AcknowledgedTerminated
                | CancellationState::AcknowledgedAlreadyTerminal
                | CancellationState::TerminationUnknown
        ),
        CancellationState::AcknowledgedTerminated
        | CancellationState::AcknowledgedAlreadyTerminal
        | CancellationState::TerminationUnknown => {
            candidate.cancellation_state == latest.cancellation_state
                && cancellation_acknowledgements_match(
                    latest.cancellation_acknowledgement.as_ref(),
                    candidate.cancellation_acknowledgement.as_ref(),
                )
                && cancellation_unresolved_evidence_matches(
                    latest.cancellation_unresolved.as_ref(),
                    candidate.cancellation_unresolved.as_ref(),
                )
        }
    }
}

fn cancellation_acknowledgements_match(
    previous: Option<&CancellationAcknowledgementEvidence>,
    candidate: Option<&CancellationAcknowledgementEvidence>,
) -> bool {
    match (previous, candidate) {
        (None, None) => true,
        (Some(previous), Some(candidate)) => {
            previous == candidate
                && canonical_timestamps_match(previous.acknowledged_at, candidate.acknowledged_at)
        }
        _ => false,
    }
}

fn cancellation_unresolved_evidence_matches(
    previous: Option<&CancellationUnresolvedEvidence>,
    candidate: Option<&CancellationUnresolvedEvidence>,
) -> bool {
    match (previous, candidate) {
        (None, None) => true,
        (Some(previous), Some(candidate)) => {
            previous == candidate
                && canonical_timestamps_match(previous.recorded_at, candidate.recorded_at)
        }
        _ => false,
    }
}

fn decode_stored_binding(bytes: &[u8]) -> Result<ExecutionBinding, StoreError> {
    match psyche_core::contracts::decode_document(bytes)
        .map_err(|_| StoreError::DatabaseCorruption)?
    {
        CanonicalDocument::ExecutionBinding(binding) => Ok(binding),
        _ => Err(StoreError::DatabaseCorruption),
    }
}

fn sql_revision(revision: u64) -> Result<i64, StoreError> {
    i64::try_from(revision).map_err(|_| {
        StoreError::Contract(ContractError::InvalidShape {
            schema: SchemaKind::ExecutionBinding,
            field: "revision",
        })
    })
}

fn revision_conflict(binding: &ExecutionBinding) -> StoreError {
    StoreError::ExecutionBindingRevisionConflict {
        attempt_id: binding.attempt_id.clone(),
        revision: binding.revision,
    }
}
