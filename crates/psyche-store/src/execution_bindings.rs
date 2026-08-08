use psyche_core::contracts::{CanonicalDocument, ContractError, ExecutionBinding, SchemaKind};
use psyche_core::digest::{canonical_bytes, digest};
use psyche_core::id::RecordId;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use time::format_description::well_known::Rfc3339;

use crate::records::InsertStatus;
use crate::{Store, StoreError, records};

struct StoredRevision {
    revision: u64,
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
    let canonical_json = canonical_bytes(binding)?;
    let revision_digest = digest(binding)?;
    let sql_revision = sql_revision(binding.revision)?;
    let created_at = binding
        .revision_created_at
        .format(&Rfc3339)
        .map_err(|_| StoreError::Contract(ContractError::CanonicalizationFailed))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let existing: Option<Vec<u8>> = transaction
        .query_row(
            "
            SELECT canonical_json
            FROM execution_binding_revisions
            WHERE attempt_id = ?1 AND revision = ?2
            ",
            params![binding.attempt_id.as_str(), sql_revision],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing == canonical_json {
            transaction.commit()?;
            return Ok(InsertStatus::AlreadyPresent);
        }
        return Err(revision_conflict(binding));
    }

    let latest = latest_revision(&transaction, &binding.attempt_id)?;
    match latest {
        None => {
            if binding.revision != 1 || binding.previous_revision_digest.is_some() {
                return Err(revision_conflict(binding));
            }
        }
        Some(latest) => {
            validate_next_revision(&transaction, binding, &latest)?;
        }
    }

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
    transaction.commit()?;
    Ok(InsertStatus::Inserted)
}

pub(crate) fn revisions(
    connection: &Connection,
    attempt_id: &RecordId,
) -> Result<Vec<ExecutionBinding>, StoreError> {
    records::validate_kind_id(SchemaKind::ExecutionBinding, attempt_id)?;
    let mut statement = connection.prepare(
        "
        SELECT canonical_json
        FROM execution_binding_revisions
        WHERE attempt_id = ?1
        ORDER BY revision
        ",
    )?;
    let canonical = statement
        .query_map([attempt_id.as_str()], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    canonical
        .into_iter()
        .map(|bytes| decode_binding(&bytes))
        .collect()
}

pub(crate) fn latest_canonical_bytes(
    connection: &Connection,
    attempt_id: &RecordId,
) -> Result<Option<Vec<u8>>, StoreError> {
    records::validate_kind_id(SchemaKind::ExecutionBinding, attempt_id)?;
    connection
        .query_row(
            "
            SELECT canonical_json
            FROM execution_binding_revisions
            WHERE attempt_id = ?1
            ORDER BY revision DESC
            LIMIT 1
            ",
            [attempt_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn validate_next_revision(
    transaction: &Transaction<'_>,
    binding: &ExecutionBinding,
    latest: &StoredRevision,
) -> Result<(), StoreError> {
    let expected_revision = latest
        .revision
        .checked_add(1)
        .ok_or_else(|| revision_conflict(binding))?;
    let latest_binding = decode_binding(&latest.canonical_json)?;
    let latest_digest = digest(&latest_binding)?;
    if binding.revision != expected_revision
        || binding.previous_revision_digest.as_ref() != Some(&latest_digest)
    {
        return Err(revision_conflict(binding));
    }

    let initial =
        first_revision(transaction, &binding.attempt_id)?.ok_or(StoreError::DatabaseOperation)?;
    if binding.revision_created_at <= latest_binding.revision_created_at
        || !frozen_execution_fields_match(&initial, binding)
        || !session_binding_is_append_only(&latest_binding, binding)
        || !termination_binding_is_append_only(&latest_binding, binding)
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
        && initial.request_created_at == candidate.request_created_at
        && initial.request_valid_until == candidate.request_valid_until
        && initial.coven_contract_version == candidate.coven_contract_version
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
            candidate.termination_request.as_ref() == Some(previous)
                && candidate.termination_reason_code == latest.termination_reason_code
        }
    }
}

fn latest_revision(
    transaction: &Transaction<'_>,
    attempt_id: &RecordId,
) -> Result<Option<StoredRevision>, StoreError> {
    let stored = transaction
        .query_row(
            "
            SELECT revision, canonical_json
            FROM execution_binding_revisions
            WHERE attempt_id = ?1
            ORDER BY revision DESC
            LIMIT 1
            ",
            [attempt_id.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    stored
        .map(|(revision, canonical_json)| {
            let revision = u64::try_from(revision).map_err(|_| StoreError::DatabaseOperation)?;
            Ok(StoredRevision {
                revision,
                canonical_json,
            })
        })
        .transpose()
}

fn first_revision(
    transaction: &Transaction<'_>,
    attempt_id: &RecordId,
) -> Result<Option<ExecutionBinding>, StoreError> {
    transaction
        .query_row(
            "
            SELECT canonical_json
            FROM execution_binding_revisions
            WHERE attempt_id = ?1 AND revision = 1
            ",
            [attempt_id.as_str()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .map(|bytes| decode_binding(&bytes))
        .transpose()
}

fn decode_binding(bytes: &[u8]) -> Result<ExecutionBinding, StoreError> {
    match psyche_core::contracts::decode_document(bytes)? {
        CanonicalDocument::ExecutionBinding(binding) => Ok(binding),
        document => Err(StoreError::Contract(ContractError::SchemaMismatch {
            expected: SchemaKind::ExecutionBinding,
            found: document.schema_version().kind,
        })),
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
