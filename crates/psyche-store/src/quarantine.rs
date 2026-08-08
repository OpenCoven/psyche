use std::fmt;

use psyche_core::contracts::{RejectedDocument, RejectionReason, SchemaKind};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use time::format_description::well_known::Rfc3339;

use crate::{Store, StoreError};

const QUARANTINE_PREFIX: &str = "qua_";
const ULID_LEN: usize = 26;
const MAX_BOUNDED_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_SCHEMA_VERSION_BYTES: usize = 128;
const QUARANTINE_RESOLVED_EVENT: &str = "quarantine_resolved";

/// A validated `qua_` identifier with one canonical uppercase ULID suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct QuarantineId(String);

impl QuarantineId {
    /// Generates a new quarantine identity.
    pub fn new() -> Self {
        Self(format!("{QUARANTINE_PREFIX}{}", ulid::Ulid::new()))
    }

    /// Strictly parses a canonical quarantine identity.
    pub fn parse(value: &str) -> Result<Self, StoreError> {
        let Some(suffix) = value.strip_prefix(QUARANTINE_PREFIX) else {
            return Err(StoreError::InvalidQuarantineId);
        };
        if suffix.len() != ULID_LEN {
            return Err(StoreError::InvalidQuarantineId);
        }
        let parsed = suffix
            .parse::<ulid::Ulid>()
            .map_err(|_| StoreError::InvalidQuarantineId)?;
        if parsed.to_string() != suffix {
            return Err(StoreError::InvalidQuarantineId);
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the complete validated identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for QuarantineId {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<String> for QuarantineId {
    type Error = StoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<QuarantineId> for String {
    fn from(value: QuarantineId) -> Self {
        value.0
    }
}

/// Stable payload-free classification persisted with a quarantine row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineReasonCode {
    /// The complete input exceeded the accepted document bound.
    TooLarge,
    /// The declared schema kind is unknown.
    UnknownSchema,
    /// The schema kind is known but its major is unsupported.
    UnsupportedMajor,
    /// A typed enum field used an unknown spelling.
    UnknownEnumValue,
    /// The typed document shape was invalid.
    InvalidShape,
}

impl QuarantineReasonCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::UnknownSchema => "unknown_schema",
            Self::UnsupportedMajor => "unsupported_major",
            Self::UnknownEnumValue => "unknown_enum_value",
            Self::InvalidShape => "invalid_shape",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "too_large" => Ok(Self::TooLarge),
            "unknown_schema" => Ok(Self::UnknownSchema),
            "unsupported_major" => Ok(Self::UnsupportedMajor),
            "unknown_enum_value" => Ok(Self::UnknownEnumValue),
            "invalid_shape" => Ok(Self::InvalidShape),
            _ => Err(StoreError::DatabaseCorruption),
        }
    }

    fn rejection_reason(self) -> RejectionReason {
        match self {
            Self::TooLarge => RejectionReason::TooLarge,
            Self::UnknownSchema => RejectionReason::UnknownSchema,
            Self::UnsupportedMajor => RejectionReason::UnsupportedMajor {
                found: 0,
                supported: 0,
            },
            Self::UnknownEnumValue => RejectionReason::UnknownEnumValue {
                schema: SchemaKind::Error,
                field: "persisted",
            },
            Self::InvalidShape => RejectionReason::InvalidShape {
                schema: SchemaKind::Error,
                field: "persisted",
            },
        }
    }
}

impl From<&RejectionReason> for QuarantineReasonCode {
    fn from(reason: &RejectionReason) -> Self {
        match reason {
            RejectionReason::TooLarge => Self::TooLarge,
            RejectionReason::UnknownSchema => Self::UnknownSchema,
            RejectionReason::UnsupportedMajor { .. } => Self::UnsupportedMajor,
            RejectionReason::UnknownEnumValue { .. } => Self::UnknownEnumValue,
            RejectionReason::InvalidShape { .. } => Self::InvalidShape,
        }
    }
}

/// Stable resolution classification for one quarantine row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineResolutionCode {
    /// A newer schema implementation can now decode the bytes.
    SchemaNowSupported,
    /// The bytes were confirmed to be invalid.
    ConfirmedInvalid,
    /// The bytes duplicate another durable payload.
    DuplicatePayload,
}

impl QuarantineResolutionCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::SchemaNowSupported => "schema_now_supported",
            Self::ConfirmedInvalid => "confirmed_invalid",
            Self::DuplicatePayload => "duplicate_payload",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "schema_now_supported" => Ok(Self::SchemaNowSupported),
            "confirmed_invalid" => Ok(Self::ConfirmedInvalid),
            "duplicate_payload" => Ok(Self::DuplicatePayload),
            _ => Err(StoreError::DatabaseCorruption),
        }
    }
}

/// A requested terminal resolution for one quarantine row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineResolution {
    /// Stable terminal classification.
    pub code: QuarantineResolutionCode,
    /// UTC time at which the resolution became authoritative.
    #[serde(with = "time::serde::rfc3339")]
    pub resolved_at: time::OffsetDateTime,
}

/// Durable result of resolving one quarantine row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveQuarantineOutcome {
    /// This request durably established the first resolution.
    Resolved {
        /// Canonical digest of the durable resolution.
        resolution_digest: Sha256Digest,
    },
    /// The exact same resolution was already durable.
    AlreadyResolved {
        /// Canonical digest shared by the replay and stored resolution.
        resolution_digest: Sha256Digest,
    },
}

/// One validated persisted quarantine row.
#[derive(Clone, PartialEq, Eq)]
pub struct QuarantineRecord {
    /// Durable quarantine identity.
    pub quarantine_id: QuarantineId,
    /// Safely bounded schema text extracted from the raw input.
    pub schema_version: Option<String>,
    /// SHA-256 digest over the complete raw input.
    pub payload_digest: Sha256Digest,
    /// Complete raw-input length before the retained payload was bounded.
    pub original_payload_len: usize,
    /// SHA-256 digest over exactly the retained payload bytes.
    pub retained_payload_digest: Sha256Digest,
    /// At most 64 KiB retained from the beginning of the raw input.
    pub bounded_payload: Vec<u8>,
    /// Stable payload-free rejection classification.
    pub reason: QuarantineReasonCode,
    /// Canonical UTC discovery time.
    pub discovered_at: time::OffsetDateTime,
    /// Canonical UTC terminal resolution time, when resolved.
    pub resolved_at: Option<time::OffsetDateTime>,
    /// Stable terminal resolution classification, when resolved.
    pub resolution_code: Option<QuarantineResolutionCode>,
    /// Canonical terminal resolution digest, when resolved.
    pub resolution_digest: Option<Sha256Digest>,
}

impl fmt::Debug for QuarantineRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuarantineRecord")
            .field("quarantine_id", &self.quarantine_id)
            .field("schema_version", &self.schema_version)
            .field("payload_digest", &self.payload_digest)
            .field("original_payload_len", &self.original_payload_len)
            .field("retained_payload_digest", &self.retained_payload_digest)
            .field("bounded_payload_bytes", &self.bounded_payload.len())
            .field("reason", &self.reason)
            .field("discovered_at", &self.discovered_at)
            .field("resolved_at", &self.resolved_at)
            .field("resolution_code", &self.resolution_code)
            .field("resolution_digest", &self.resolution_digest)
            .finish()
    }
}

/// One validated redacted audit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// Monotonic database sequence.
    pub sequence: u64,
    /// Stable event classification.
    pub event_code: String,
    /// Validated payload-free event correlation identity.
    pub correlation_id: String,
    /// Canonical JSON containing only redacted public metadata.
    pub public_details_json: Vec<u8>,
    /// Canonical UTC event time.
    pub created_at: time::OffsetDateTime,
}

struct StoredQuarantineRecord {
    quarantine_id: String,
    schema_version: Option<String>,
    payload_digest: String,
    original_payload_len: i64,
    retained_payload_digest: String,
    bounded_payload: Vec<u8>,
    reason: String,
    discovered_at: String,
    resolved_at: Option<String>,
    resolution_code: Option<String>,
    resolution_digest: Option<String>,
}

struct StoredAuditEvent {
    sequence: i64,
    event_code: String,
    correlation_id: String,
    public_details_json: Vec<u8>,
    created_at: String,
}

#[derive(serde::Serialize)]
struct ResolutionDigestInput<'a> {
    quarantine_id: &'a QuarantineId,
    payload_digest: &'a Sha256Digest,
    reason: QuarantineReasonCode,
    resolution_code: QuarantineResolutionCode,
    #[serde(with = "time::serde::rfc3339")]
    resolved_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QuarantineResolvedAuditDetails {
    quarantine_id: QuarantineId,
    payload_digest: Sha256Digest,
    reason: QuarantineReasonCode,
    resolution_code: QuarantineResolutionCode,
    #[serde(with = "time::serde::rfc3339")]
    resolved_at: time::OffsetDateTime,
    resolution_digest: Sha256Digest,
}

impl Store {
    /// Validates and durably retains one bounded rejected document.
    pub fn quarantine(&mut self, rejected: RejectedDocument) -> Result<QuarantineId, StoreError> {
        validate_rejected(&rejected)?;
        let reason = QuarantineReasonCode::from(&rejected.reason);
        let original_payload_len = rejected.original_payload_len();
        let stored_original_payload_len =
            i64::try_from(original_payload_len).map_err(|_| StoreError::InvalidQuarantineRecord)?;
        let retained_payload_digest = rejected.retained_payload_digest();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = stored_by_digest_and_reason(
            &transaction,
            rejected.payload_digest.as_str(),
            reason.as_str(),
        )?;
        if existing.len() > 1 {
            return Err(StoreError::DatabaseCorruption);
        }
        if let Some(stored) = existing.into_iter().next() {
            let record = validate_stored(stored)?;
            validate_record_audit(&transaction, &record)?;
            if record.schema_version == rejected.schema_version
                && record.payload_digest == rejected.payload_digest
                && record.original_payload_len == original_payload_len
                && record.retained_payload_digest == retained_payload_digest
                && record.bounded_payload == rejected.bounded_payload
                && record.reason == reason
            {
                transaction.commit()?;
                return Ok(record.quarantine_id);
            }
            return Err(StoreError::QuarantineConflict {
                payload_digest: rejected.payload_digest,
            });
        }

        let quarantine_id = QuarantineId::new();
        let discovered_at = time::OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| StoreError::InvalidQuarantineRecord)?;
        transaction.execute(
            "
            INSERT INTO quarantine_records (
                quarantine_id,
                schema_version,
                payload_digest,
                original_payload_len,
                retained_payload_digest,
                bounded_payload,
                reason,
                discovered_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ",
            params![
                quarantine_id.as_str(),
                rejected.schema_version.as_deref(),
                rejected.payload_digest.as_str(),
                stored_original_payload_len,
                retained_payload_digest.as_str(),
                rejected.bounded_payload,
                reason.as_str(),
                discovered_at,
            ],
        )?;
        let stored = stored_by_id(&transaction, quarantine_id.as_str())?
            .ok_or(StoreError::DatabaseCorruption)?;
        let persisted = validate_stored(stored)?;
        if persisted.quarantine_id != quarantine_id {
            return Err(StoreError::DatabaseCorruption);
        }
        transaction.commit()?;
        Ok(quarantine_id)
    }

    /// Loads and validates one quarantine row by exact identity.
    pub fn quarantine_record(
        &self,
        id: &QuarantineId,
    ) -> Result<Option<QuarantineRecord>, StoreError> {
        validate_typed_id(id)?;
        let transaction = self.connection.unchecked_transaction()?;
        let result = (|| {
            let Some(stored) = stored_by_id(&transaction, id.as_str())? else {
                return Ok(None);
            };
            let record = validate_stored(stored)?;
            validate_record_audit(&transaction, &record)?;
            Ok(Some(record))
        })();
        match result {
            Ok(record) => {
                transaction.commit()?;
                Ok(record)
            }
            Err(error) => {
                transaction.rollback()?;
                Err(error)
            }
        }
    }

    /// Atomically establishes or exactly replays one quarantine resolution.
    pub fn resolve_quarantine(
        &mut self,
        id: &QuarantineId,
        resolution: &QuarantineResolution,
    ) -> Result<ResolveQuarantineOutcome, StoreError> {
        validate_typed_id(id)?;
        if resolution.resolved_at.offset() != time::UtcOffset::UTC {
            return Err(StoreError::InvalidQuarantineResolution {
                quarantine_id: id.clone(),
            });
        }
        let resolved_at = resolution.resolved_at.format(&Rfc3339).map_err(|_| {
            StoreError::InvalidQuarantineResolution {
                quarantine_id: id.clone(),
            }
        })?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = stored_by_id(&transaction, id.as_str())?.ok_or_else(|| {
            StoreError::QuarantineNotFound {
                quarantine_id: id.clone(),
            }
        })?;
        let record = validate_stored(stored)?;
        validate_record_audit(&transaction, &record)?;
        if resolution.resolved_at < record.discovered_at {
            return Err(StoreError::InvalidQuarantineResolution {
                quarantine_id: id.clone(),
            });
        }
        let resolution_digest = compute_resolution_digest(
            id,
            &record.payload_digest,
            record.reason,
            resolution.code,
            resolution.resolved_at,
        )
        .map_err(StoreError::from)?;

        if record.resolved_at.is_some() {
            let outcome = replay_or_conflict(id, &record, resolution, &resolution_digest)?;
            transaction.commit()?;
            return Ok(outcome);
        }

        let updated = transaction.execute(
            "
            UPDATE quarantine_records
            SET resolved_at = ?1,
                resolution_code = ?2,
                resolution_digest = ?3
            WHERE quarantine_id = ?4
              AND resolved_at IS NULL
              AND resolution_code IS NULL
              AND resolution_digest IS NULL
            ",
            params![
                resolved_at,
                resolution.code.as_str(),
                resolution_digest.as_str(),
                id.as_str(),
            ],
        )?;
        if updated == 0 {
            let reloaded = stored_by_id(&transaction, id.as_str())?
                .ok_or(StoreError::DatabaseCorruption)
                .and_then(validate_stored)?;
            validate_record_audit(&transaction, &reloaded)?;
            let outcome = replay_or_conflict(id, &reloaded, resolution, &resolution_digest)?;
            transaction.commit()?;
            return Ok(outcome);
        }
        if updated != 1 {
            return Err(StoreError::DatabaseCorruption);
        }

        let details = QuarantineResolvedAuditDetails {
            quarantine_id: id.clone(),
            payload_digest: record.payload_digest,
            reason: record.reason,
            resolution_code: resolution.code,
            resolved_at: resolution.resolved_at,
            resolution_digest: resolution_digest.clone(),
        };
        let public_details_json = canonical_bytes(&details)?;
        transaction.execute(
            "
            INSERT INTO audit_events (
                event_code,
                correlation_id,
                public_details_json,
                created_at
            )
            VALUES (?1, ?2, ?3, ?4)
            ",
            params![
                QUARANTINE_RESOLVED_EVENT,
                id.as_str(),
                public_details_json,
                resolved_at,
            ],
        )?;
        let resolved = stored_by_id(&transaction, id.as_str())?
            .ok_or(StoreError::DatabaseCorruption)
            .and_then(validate_stored)?;
        validate_record_audit(&transaction, &resolved)?;
        transaction.commit()?;
        Ok(ResolveQuarantineOutcome::Resolved { resolution_digest })
    }

    /// Returns every validated redacted audit event in sequence order.
    pub fn audit_events(&self) -> Result<Vec<AuditEvent>, StoreError> {
        audit_events_from_connection(&self.connection)
    }
}

pub(crate) fn all_records(connection: &Connection) -> Result<Vec<QuarantineRecord>, StoreError> {
    let records = load_all_stored(connection)?
        .into_iter()
        .map(validate_stored)
        .collect::<Result<Vec<_>, _>>()?;
    let mut identities = std::collections::HashSet::with_capacity(records.len());
    let mut digest_reasons = std::collections::HashSet::with_capacity(records.len());
    for record in &records {
        if !identities.insert(record.quarantine_id.clone())
            || !digest_reasons.insert((record.payload_digest.clone(), record.reason))
        {
            return Err(StoreError::DatabaseCorruption);
        }
        validate_record_audit(connection, record)?;
    }
    Ok(records)
}

pub(crate) fn audit_events_from_connection(
    connection: &Connection,
) -> Result<Vec<AuditEvent>, StoreError> {
    load_all_audit_events(connection)?
        .into_iter()
        .map(validate_audit_event)
        .collect()
}

fn validate_typed_id(id: &QuarantineId) -> Result<(), StoreError> {
    if matches!(QuarantineId::parse(id.as_str()), Ok(parsed) if parsed == *id) {
        Ok(())
    } else {
        Err(StoreError::InvalidQuarantineId)
    }
}

fn validate_rejected(rejected: &RejectedDocument) -> Result<(), StoreError> {
    if !rejected.is_authentic()
        || rejected.bounded_payload.len() > MAX_BOUNDED_PAYLOAD_BYTES
        || !rejected
            .schema_version
            .as_deref()
            .is_none_or(schema_version_is_safe)
        || Sha256Digest::parse(rejected.payload_digest.as_str()).is_err()
    {
        return Err(StoreError::InvalidQuarantineRecord);
    }

    if rejected.bounded_payload.len() < MAX_BOUNDED_PAYLOAD_BYTES {
        let reconstructed =
            RejectedDocument::from_bytes(&rejected.bounded_payload, rejected.reason.clone());
        if reconstructed.payload_digest != rejected.payload_digest
            || reconstructed.schema_version != rejected.schema_version
        {
            return Err(StoreError::InvalidQuarantineRecord);
        }
    }
    Ok(())
}

fn schema_version_is_safe(value: &str) -> bool {
    value.len() <= MAX_SCHEMA_VERSION_BYTES
        && value.starts_with("psyche.")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_')
        })
}

fn stored_by_id(
    connection: &Connection,
    id: &str,
) -> Result<Option<StoredQuarantineRecord>, StoreError> {
    connection
        .query_row(
            "
            SELECT
                quarantine_id,
                schema_version,
                payload_digest,
                original_payload_len,
                retained_payload_digest,
                bounded_payload,
                reason,
                discovered_at,
                resolved_at,
                resolution_code,
                resolution_digest
            FROM quarantine_records
            WHERE quarantine_id = ?1
            ",
            [id],
            stored_quarantine_from_row,
        )
        .optional()
        .map_err(|_| StoreError::DatabaseCorruption)
}

fn stored_by_digest_and_reason(
    connection: &Connection,
    payload_digest: &str,
    reason: &str,
) -> Result<Vec<StoredQuarantineRecord>, StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT
            quarantine_id,
            schema_version,
            payload_digest,
            original_payload_len,
            retained_payload_digest,
            bounded_payload,
            reason,
            discovered_at,
            resolved_at,
            resolution_code,
            resolution_digest
        FROM quarantine_records
        WHERE payload_digest = ?1 AND reason = ?2
        ORDER BY quarantine_id
        ",
    )?;
    statement
        .query_map(params![payload_digest, reason], stored_quarantine_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)
}

fn load_all_stored(connection: &Connection) -> Result<Vec<StoredQuarantineRecord>, StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT
            quarantine_id,
            schema_version,
            payload_digest,
            original_payload_len,
            retained_payload_digest,
            bounded_payload,
            reason,
            discovered_at,
            resolved_at,
            resolution_code,
            resolution_digest
        FROM quarantine_records
        ORDER BY quarantine_id
        ",
    )?;
    statement
        .query_map([], stored_quarantine_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)
}

fn stored_quarantine_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredQuarantineRecord> {
    Ok(StoredQuarantineRecord {
        quarantine_id: row.get(0)?,
        schema_version: row.get(1)?,
        payload_digest: row.get(2)?,
        original_payload_len: row.get(3)?,
        retained_payload_digest: row.get(4)?,
        bounded_payload: row.get(5)?,
        reason: row.get(6)?,
        discovered_at: row.get(7)?,
        resolved_at: row.get(8)?,
        resolution_code: row.get(9)?,
        resolution_digest: row.get(10)?,
    })
}

fn validate_stored(stored: StoredQuarantineRecord) -> Result<QuarantineRecord, StoreError> {
    let quarantine_id =
        QuarantineId::parse(&stored.quarantine_id).map_err(|_| StoreError::DatabaseCorruption)?;
    let original_payload_len =
        usize::try_from(stored.original_payload_len).map_err(|_| StoreError::DatabaseCorruption)?;
    if !stored
        .schema_version
        .as_deref()
        .is_none_or(schema_version_is_safe)
        || stored.bounded_payload.len() > MAX_BOUNDED_PAYLOAD_BYTES
        || stored.bounded_payload.len() != original_payload_len.min(MAX_BOUNDED_PAYLOAD_BYTES)
    {
        return Err(StoreError::DatabaseCorruption);
    }
    let payload_digest =
        Sha256Digest::parse(&stored.payload_digest).map_err(|_| StoreError::DatabaseCorruption)?;
    let retained_payload_digest = Sha256Digest::parse(&stored.retained_payload_digest)
        .map_err(|_| StoreError::DatabaseCorruption)?;
    let reason = QuarantineReasonCode::parse(&stored.reason)?;
    let reconstructed =
        RejectedDocument::from_bytes(&stored.bounded_payload, reason.rejection_reason());
    if reconstructed.retained_payload_digest() != retained_payload_digest {
        return Err(StoreError::DatabaseCorruption);
    }
    if original_payload_len <= MAX_BOUNDED_PAYLOAD_BYTES
        && (reconstructed.payload_digest != payload_digest
            || reconstructed.schema_version != stored.schema_version)
    {
        return Err(StoreError::DatabaseCorruption);
    }
    let discovered_at = parse_canonical_utc(&stored.discovered_at)?;

    let resolution_columns = (
        stored.resolved_at.as_deref(),
        stored.resolution_code.as_deref(),
        stored.resolution_digest.as_deref(),
    );
    let (resolved_at, resolution_code, resolution_digest) = match resolution_columns {
        (None, None, None) => (None, None, None),
        (Some(resolved_at), Some(resolution_code), Some(resolution_digest)) => {
            let resolved_at = parse_canonical_utc(resolved_at)?;
            if resolved_at < discovered_at {
                return Err(StoreError::DatabaseCorruption);
            }
            let resolution_code = QuarantineResolutionCode::parse(resolution_code)?;
            let resolution_digest = Sha256Digest::parse(resolution_digest)
                .map_err(|_| StoreError::DatabaseCorruption)?;
            let recomputed = compute_resolution_digest(
                &quarantine_id,
                &payload_digest,
                reason,
                resolution_code,
                resolved_at,
            )
            .map_err(|_| StoreError::DatabaseCorruption)?;
            if recomputed != resolution_digest {
                return Err(StoreError::DatabaseCorruption);
            }
            (
                Some(resolved_at),
                Some(resolution_code),
                Some(resolution_digest),
            )
        }
        _ => return Err(StoreError::DatabaseCorruption),
    };

    Ok(QuarantineRecord {
        quarantine_id,
        schema_version: stored.schema_version,
        payload_digest,
        original_payload_len,
        retained_payload_digest,
        bounded_payload: stored.bounded_payload,
        reason,
        discovered_at,
        resolved_at,
        resolution_code,
        resolution_digest,
    })
}

fn compute_resolution_digest(
    quarantine_id: &QuarantineId,
    payload_digest: &Sha256Digest,
    reason: QuarantineReasonCode,
    resolution_code: QuarantineResolutionCode,
    resolved_at: time::OffsetDateTime,
) -> Result<Sha256Digest, psyche_core::contracts::ContractError> {
    digest(&ResolutionDigestInput {
        quarantine_id,
        payload_digest,
        reason,
        resolution_code,
        resolved_at,
    })
}

fn replay_or_conflict(
    id: &QuarantineId,
    stored: &QuarantineRecord,
    requested: &QuarantineResolution,
    requested_digest: &Sha256Digest,
) -> Result<ResolveQuarantineOutcome, StoreError> {
    if stored.resolved_at == Some(requested.resolved_at)
        && stored.resolution_code == Some(requested.code)
        && stored.resolution_digest.as_ref() == Some(requested_digest)
    {
        return Ok(ResolveQuarantineOutcome::AlreadyResolved {
            resolution_digest: requested_digest.clone(),
        });
    }
    Err(StoreError::QuarantineResolutionConflict {
        quarantine_id: id.clone(),
        resolution_digest: requested_digest.clone(),
    })
}

fn load_all_audit_events(connection: &Connection) -> Result<Vec<StoredAuditEvent>, StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT sequence, event_code, correlation_id, public_details_json, created_at
        FROM audit_events
        ORDER BY sequence
        ",
    )?;
    statement
        .query_map([], |row| {
            Ok(StoredAuditEvent {
                sequence: row.get(0)?,
                event_code: row.get(1)?,
                correlation_id: row.get(2)?,
                public_details_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)
}

fn validate_audit_event(stored: StoredAuditEvent) -> Result<AuditEvent, StoreError> {
    validate_audit_event_with_details(stored).map(|(event, _details)| event)
}

fn validate_audit_event_with_details(
    stored: StoredAuditEvent,
) -> Result<(AuditEvent, QuarantineResolvedAuditDetails), StoreError> {
    let sequence = u64::try_from(stored.sequence).map_err(|_| StoreError::DatabaseCorruption)?;
    if sequence == 0 || stored.event_code != QUARANTINE_RESOLVED_EVENT {
        return Err(StoreError::DatabaseCorruption);
    }
    let correlation_id =
        QuarantineId::parse(&stored.correlation_id).map_err(|_| StoreError::DatabaseCorruption)?;
    let details: QuarantineResolvedAuditDetails =
        serde_json::from_slice(&stored.public_details_json)
            .map_err(|_| StoreError::DatabaseCorruption)?;
    let canonical_details =
        canonical_bytes(&details).map_err(|_| StoreError::DatabaseCorruption)?;
    let created_at = parse_canonical_utc(&stored.created_at)?;
    let recomputed = compute_resolution_digest(
        &details.quarantine_id,
        &details.payload_digest,
        details.reason,
        details.resolution_code,
        details.resolved_at,
    )
    .map_err(|_| StoreError::DatabaseCorruption)?;
    if canonical_details != stored.public_details_json
        || details.quarantine_id != correlation_id
        || details.resolution_digest != recomputed
        || details.resolved_at != created_at
        || details.resolved_at.offset() != time::UtcOffset::UTC
    {
        return Err(StoreError::DatabaseCorruption);
    }

    Ok((
        AuditEvent {
            sequence,
            event_code: stored.event_code,
            correlation_id: stored.correlation_id,
            public_details_json: stored.public_details_json,
            created_at,
        },
        details,
    ))
}

fn validate_record_audit(
    connection: &Connection,
    record: &QuarantineRecord,
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(
        "
        SELECT sequence, event_code, correlation_id, public_details_json, created_at
        FROM audit_events
        WHERE correlation_id = ?1
        ORDER BY sequence
        ",
    )?;
    let stored = statement
        .query_map([record.quarantine_id.as_str()], |row| {
            Ok(StoredAuditEvent {
                sequence: row.get(0)?,
                event_code: row.get(1)?,
                correlation_id: row.get(2)?,
                public_details_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| StoreError::DatabaseCorruption)?;
    let validated = stored
        .into_iter()
        .map(validate_audit_event_with_details)
        .collect::<Result<Vec<_>, _>>()?;

    match (
        record.resolved_at,
        record.resolution_code,
        record.resolution_digest.as_ref(),
    ) {
        (None, None, None) if validated.is_empty() => Ok(()),
        (Some(resolved_at), Some(resolution_code), Some(resolution_digest))
            if validated.len() == 1 =>
        {
            let details = &validated[0].1;
            if details.quarantine_id == record.quarantine_id
                && details.payload_digest == record.payload_digest
                && details.reason == record.reason
                && details.resolution_code == resolution_code
                && details.resolved_at == resolved_at
                && &details.resolution_digest == resolution_digest
            {
                Ok(())
            } else {
                Err(StoreError::DatabaseCorruption)
            }
        }
        _ => Err(StoreError::DatabaseCorruption),
    }
}

fn parse_canonical_utc(value: &str) -> Result<time::OffsetDateTime, StoreError> {
    let timestamp =
        time::OffsetDateTime::parse(value, &Rfc3339).map_err(|_| StoreError::DatabaseCorruption)?;
    let canonical = timestamp
        .format(&Rfc3339)
        .map_err(|_| StoreError::DatabaseCorruption)?;
    if timestamp.offset() != time::UtcOffset::UTC || canonical != value {
        return Err(StoreError::DatabaseCorruption);
    }
    Ok(timestamp)
}
