use psyche_core::contracts::{
    CanonicalDocument, ContractError, RejectedDocument, SchemaKind, decode_document,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// A new canonical record was persisted.
    Inserted,
    /// The exact canonical record was already present.
    AlreadyPresent,
    /// Unsupported bytes were retained only in quarantine.
    Quarantined {
        /// Validated identity of the retained quarantine row.
        quarantine_id: crate::QuarantineId,
    },
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
                let quarantine_id = self.quarantine(rejected)?;
                Ok(IngestOutcome::Quarantined { quarantine_id })
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

pub(crate) fn parse_kind_key(value: &str) -> Result<SchemaKind, StoreError> {
    match value {
        "identity_snapshot" => Ok(SchemaKind::IdentitySnapshot),
        "intent" => Ok(SchemaKind::Intent),
        "surface_event" => Ok(SchemaKind::SurfaceEvent),
        "graph" => Ok(SchemaKind::Graph),
        "graph_node" => Ok(SchemaKind::GraphNode),
        "delegation" => Ok(SchemaKind::Delegation),
        "budget" => Ok(SchemaKind::Budget),
        "approval" => Ok(SchemaKind::Approval),
        "execution_binding" => Ok(SchemaKind::ExecutionBinding),
        "evidence" => Ok(SchemaKind::Evidence),
        "verdict" => Ok(SchemaKind::Verdict),
        "recovery" => Ok(SchemaKind::Recovery),
        "addon" => Ok(SchemaKind::Addon),
        "surface_effect" => Ok(SchemaKind::SurfaceEffect),
        "delivery" => Ok(SchemaKind::Delivery),
        "error" => Ok(SchemaKind::Error),
        _ => Err(StoreError::DatabaseCorruption),
    }
}

pub(crate) fn schema_kind_for_id(id: &RecordId) -> SchemaKind {
    use psyche_core::contracts::RecordKind;

    match id.kind() {
        RecordKind::IdentitySnapshot => SchemaKind::IdentitySnapshot,
        RecordKind::Intent => SchemaKind::Intent,
        RecordKind::Graph => SchemaKind::Graph,
        RecordKind::GraphNode => SchemaKind::GraphNode,
        RecordKind::Attempt => SchemaKind::ExecutionBinding,
        RecordKind::Delegation => SchemaKind::Delegation,
        RecordKind::Budget => SchemaKind::Budget,
        RecordKind::Approval => SchemaKind::Approval,
        RecordKind::Evidence => SchemaKind::Evidence,
        RecordKind::Verdict => SchemaKind::Verdict,
        RecordKind::Recovery => SchemaKind::Recovery,
        RecordKind::Addon => SchemaKind::Addon,
        RecordKind::SurfaceEvent => SchemaKind::SurfaceEvent,
        RecordKind::SurfaceEffect => SchemaKind::SurfaceEffect,
        RecordKind::Delivery => SchemaKind::Delivery,
    }
}

pub(crate) fn validate_all(connection: &rusqlite::Connection) -> Result<(), StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT kind, record_id, schema_version, digest, canonical_json
        FROM canonical_records
        ORDER BY kind, record_id
        ",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok(StoredCanonicalRecord {
                kind: row.get(0)?,
                record_id: row.get(1)?,
                schema_version: row.get(2)?,
                digest: row.get(3)?,
                canonical_json: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)?;
    for row in rows {
        let kind = parse_kind_key(&row.kind)?;
        if kind == SchemaKind::ExecutionBinding {
            return Err(StoreError::DatabaseCorruption);
        }
        let record_kind = kind.record_kind().ok_or(StoreError::DatabaseCorruption)?;
        let id = RecordId::parse(record_kind, &row.record_id)
            .map_err(|_| StoreError::DatabaseCorruption)?;
        validate_stored_canonical_record(&row, kind, &id)?;
    }
    Ok(())
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
