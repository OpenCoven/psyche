use psyche_core::contracts::{
    CanonicalDocument, ContractError, RejectedDocument, RejectionReason, SchemaKind,
    decode_document,
};
use psyche_core::digest::{canonical_bytes, digest};
use psyche_core::id::RecordId;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{Store, StoreError, execution_bindings};

struct StoredCanonicalRecord {
    kind: String,
    record_id: String,
    schema_version: String,
    digest: String,
    canonical_json: Vec<u8>,
}

/// Result of ingesting bytes at the store boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    /// A new canonical record was persisted.
    Inserted,
    /// The exact canonical record was already present.
    AlreadyPresent,
    /// Unsupported bytes were retained only in quarantine.
    Quarantined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertStatus {
    Inserted,
    AlreadyPresent,
}

impl Store {
    /// Strictly decodes and persists or quarantines one byte document.
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<IngestOutcome, StoreError> {
        match decode_document(bytes) {
            Ok(document) => match self.insert_with_status(&document)? {
                InsertStatus::Inserted => Ok(IngestOutcome::Inserted),
                InsertStatus::AlreadyPresent => Ok(IngestOutcome::AlreadyPresent),
            },
            Err(
                error @ (ContractError::UnknownSchema
                | ContractError::UnsupportedMajor { .. }
                | ContractError::UnknownEnumValue { .. }),
            ) => {
                let rejected = RejectedDocument::from_decode_error(bytes, error);
                self.quarantine_decode_rejection(&rejected)?;
                Ok(IngestOutcome::Quarantined)
            }
            Err(error) => Err(StoreError::Contract(error)),
        }
    }

    /// Validates and immutably persists one typed canonical document.
    pub fn insert(&mut self, document: &CanonicalDocument) -> Result<(), StoreError> {
        self.insert_with_status(document).map(|_| ())
    }

    /// Loads and validates one canonical record by exact kind and identity.
    pub fn load(
        &self,
        kind: SchemaKind,
        id: &RecordId,
    ) -> Result<Option<CanonicalDocument>, StoreError> {
        let Some(bytes) = self.load_canonical_bytes(kind, id)? else {
            return Ok(None);
        };
        let document = decode_document(&bytes).map_err(|_| StoreError::DatabaseCorruption)?;
        Ok(Some(document))
    }

    /// Loads immutable canonical bytes by exact kind and identity.
    pub fn load_canonical_bytes(
        &self,
        kind: SchemaKind,
        id: &RecordId,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        validate_kind_id(kind, id)?;
        if kind == SchemaKind::ExecutionBinding {
            return execution_bindings::latest_canonical_bytes(&self.connection, id);
        }
        let Some(stored) = stored_canonical_record(&self.connection, kind, id)? else {
            return Ok(None);
        };
        validate_stored_canonical_record(&stored, kind, id)?;
        Ok(Some(stored.canonical_json))
    }

    /// Counts persisted logical records of one schema kind.
    pub fn count_records(&self, kind: SchemaKind) -> Result<u64, StoreError> {
        if kind.record_kind().is_none() {
            return Err(StoreError::NonPersistableKind { kind });
        }
        let count: i64 = if kind == SchemaKind::ExecutionBinding {
            self.connection.query_row(
                "SELECT COUNT(DISTINCT attempt_id) FROM execution_binding_revisions",
                [],
                |row| row.get(0),
            )?
        } else {
            self.connection.query_row(
                "SELECT COUNT(*) FROM canonical_records WHERE kind = ?1",
                [kind_key(kind)],
                |row| row.get(0),
            )?
        };
        count.try_into().map_err(|_| StoreError::DatabaseOperation)
    }

    /// Counts all persisted logical canonical and execution-binding records.
    pub fn total_record_count(&self) -> Result<u64, StoreError> {
        let canonical: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM canonical_records", [], |row| {
                    row.get(0)
                })?;
        let bindings: i64 = self.connection.query_row(
            "SELECT COUNT(DISTINCT attempt_id) FROM execution_binding_revisions",
            [],
            |row| row.get(0),
        )?;
        let total = canonical
            .checked_add(bindings)
            .ok_or(StoreError::DatabaseOperation)?;
        total.try_into().map_err(|_| StoreError::DatabaseOperation)
    }

    fn insert_with_status(
        &mut self,
        document: &CanonicalDocument,
    ) -> Result<InsertStatus, StoreError> {
        document.validate()?;
        let kind = document.schema_version().kind;
        let id = document
            .persistable_record_id()
            .ok_or(StoreError::NonPersistableKind { kind })?;
        if let CanonicalDocument::ExecutionBinding(binding) = document {
            return execution_bindings::insert(&mut self.connection, binding);
        }

        let bytes = canonical_bytes(document)?;
        let record_digest = digest(document)?;
        let schema_version = document.schema_version().to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(stored) = stored_canonical_record(&transaction, kind, id)? {
            validate_stored_canonical_record(&stored, kind, id)?;
            if stored.canonical_json == bytes {
                transaction.commit()?;
                return Ok(InsertStatus::AlreadyPresent);
            }
            return Err(StoreError::RecordConflict {
                kind,
                record_id: id.clone(),
            });
        }

        transaction.execute(
            "
            INSERT INTO canonical_records (
                kind,
                record_id,
                schema_version,
                digest,
                canonical_json,
                created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            ",
            params![
                kind_key(kind),
                id.as_str(),
                schema_version,
                record_digest.as_str(),
                bytes,
            ],
        )?;
        transaction.commit()?;
        Ok(InsertStatus::Inserted)
    }

    fn quarantine_decode_rejection(
        &mut self,
        rejected: &RejectedDocument,
    ) -> Result<(), StoreError> {
        let reason = match rejected.reason {
            RejectionReason::TooLarge => "too_large",
            RejectionReason::UnknownSchema => "unknown_schema",
            RejectionReason::UnsupportedMajor { .. } => "unsupported_major",
            RejectionReason::UnknownEnumValue { .. } => "unknown_enum_value",
            RejectionReason::InvalidShape { .. } => "invalid_shape",
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "
            INSERT INTO quarantine_records (
                quarantine_id,
                schema_version,
                payload_digest,
                bounded_payload,
                reason,
                discovered_at
            )
            VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            )
            ON CONFLICT(quarantine_id) DO NOTHING
            ",
            params![
                rejected.payload_digest.as_str(),
                rejected.schema_version.as_deref(),
                rejected.payload_digest.as_str(),
                &rejected.bounded_payload,
                reason,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn stored_canonical_record(
    connection: &rusqlite::Connection,
    kind: SchemaKind,
    id: &RecordId,
) -> Result<Option<StoredCanonicalRecord>, StoreError> {
    connection
        .query_row(
            "
            SELECT kind, record_id, schema_version, digest, canonical_json
            FROM canonical_records
            WHERE kind = ?1 AND record_id = ?2
            ",
            params![kind_key(kind), id.as_str()],
            |row| {
                Ok(StoredCanonicalRecord {
                    kind: row.get(0)?,
                    record_id: row.get(1)?,
                    schema_version: row.get(2)?,
                    digest: row.get(3)?,
                    canonical_json: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn validate_stored_canonical_record(
    stored: &StoredCanonicalRecord,
    expected_kind: SchemaKind,
    expected_id: &RecordId,
) -> Result<(), StoreError> {
    let document =
        decode_document(&stored.canonical_json).map_err(|_| StoreError::DatabaseCorruption)?;
    let canonical = canonical_bytes(&document).map_err(|_| StoreError::DatabaseCorruption)?;
    let recomputed_digest = digest(&document).map_err(|_| StoreError::DatabaseCorruption)?;
    let schema_version = document.schema_version();
    if canonical != stored.canonical_json
        || stored.kind != kind_key(expected_kind)
        || stored.record_id != expected_id.as_str()
        || schema_version.kind != expected_kind
        || stored.schema_version != schema_version.to_string()
        || stored.digest != recomputed_digest.as_str()
        || document.persistable_record_id() != Some(expected_id)
    {
        return Err(StoreError::DatabaseCorruption);
    }
    Ok(())
}

pub(crate) fn kind_key(kind: SchemaKind) -> &'static str {
    match kind {
        SchemaKind::IdentitySnapshot => "identity_snapshot",
        SchemaKind::Intent => "intent",
        SchemaKind::SurfaceEvent => "surface_event",
        SchemaKind::Graph => "graph",
        SchemaKind::GraphNode => "graph_node",
        SchemaKind::Delegation => "delegation",
        SchemaKind::Budget => "budget",
        SchemaKind::Approval => "approval",
        SchemaKind::ExecutionBinding => "execution_binding",
        SchemaKind::Evidence => "evidence",
        SchemaKind::Verdict => "verdict",
        SchemaKind::Recovery => "recovery",
        SchemaKind::Addon => "addon",
        SchemaKind::SurfaceEffect => "surface_effect",
        SchemaKind::Delivery => "delivery",
        SchemaKind::Error => "error",
    }
}

pub(crate) fn validate_kind_id(kind: SchemaKind, id: &RecordId) -> Result<(), StoreError> {
    let expected = kind
        .record_kind()
        .ok_or(StoreError::NonPersistableKind { kind })?;
    if id.kind() != expected {
        return Err(StoreError::Contract(ContractError::WrongRecordKind {
            schema: kind,
            field: "record_id",
            expected,
            found: id.kind(),
        }));
    }
    Ok(())
}
