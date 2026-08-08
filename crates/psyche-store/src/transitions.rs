use psyche_core::contracts::{ContractError, SchemaKind};
use psyche_core::digest::{Sha256Digest, digest};
use psyche_core::id::RecordId;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use time::format_description::well_known::Rfc3339;

use crate::{Store, StoreError, records};

/// One immutable state transition for a persisted record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Schema kind of the transitioned record.
    pub kind: SchemaKind,
    /// Durable identity of the transitioned record.
    pub record_id: RecordId,
    /// One-based version in this record's transition history.
    pub record_version: u64,
    /// State immediately before this transition.
    pub from_state: Option<String>,
    /// State established by this transition.
    pub to_state: String,
    /// Canonical digest of every other transition field.
    pub transition_digest: Sha256Digest,
    /// UTC creation time.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct TransitionDigestInput<'a> {
    kind: SchemaKind,
    record_id: &'a RecordId,
    record_version: u64,
    from_state: &'a Option<String>,
    to_state: &'a str,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

struct StoredTransition {
    kind: String,
    record_id: String,
    from_state: Option<String>,
    to_state: String,
    record_version: i64,
    transition_digest: String,
    created_at: String,
}

impl Transition {
    /// Builds a transition and binds its canonical digest.
    pub fn new(
        kind: SchemaKind,
        record_id: RecordId,
        record_version: u64,
        from_state: Option<String>,
        to_state: String,
        created_at: time::OffsetDateTime,
    ) -> Result<Self, ContractError> {
        let transition_digest = digest(&TransitionDigestInput {
            kind,
            record_id: &record_id,
            record_version,
            from_state: &from_state,
            to_state: &to_state,
            created_at,
        })?;
        let transition = Self {
            kind,
            record_id,
            record_version,
            from_state,
            to_state,
            transition_digest,
            created_at,
        };
        transition.validate_shape()?;
        Ok(transition)
    }

    /// Revalidates the transition shape, identity, and canonical digest.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.validate_shape()?;
        if self.transition_digest != self.computed_digest()? {
            return Err(ContractError::DigestMismatch {
                schema: self.kind,
                field: "transition_digest",
            });
        }
        Ok(())
    }

    fn computed_digest(&self) -> Result<Sha256Digest, ContractError> {
        digest(&TransitionDigestInput {
            kind: self.kind,
            record_id: &self.record_id,
            record_version: self.record_version,
            from_state: &self.from_state,
            to_state: &self.to_state,
            created_at: self.created_at,
        })
    }

    fn validate_shape(&self) -> Result<(), ContractError> {
        let Some(expected) = self.kind.record_kind() else {
            return Err(ContractError::InvalidShape {
                schema: self.kind,
                field: "kind",
            });
        };
        if self.record_id.kind() != expected {
            return Err(ContractError::WrongRecordKind {
                schema: self.kind,
                field: "record_id",
                expected,
                found: self.record_id.kind(),
            });
        }
        if self.record_version == 0 {
            return Err(invalid(self.kind, "record_version"));
        }
        if self.record_version > 1 && self.from_state.is_none() {
            return Err(invalid(self.kind, "from_state"));
        }
        if let Some(from_state) = &self.from_state {
            validate_state(from_state, self.kind, "from_state")?;
            if from_state == &self.to_state {
                return Err(invalid(self.kind, "to_state"));
            }
        }

        validate_state(&self.to_state, self.kind, "to_state")?;
        if self.created_at.offset() != time::UtcOffset::UTC {
            return Err(invalid(self.kind, "created_at"));
        }
        Ok(())
    }
}

impl Store {
    /// Validates and appends one immutable transition.
    pub fn append_transition(&mut self, transition: &Transition) -> Result<(), StoreError> {
        transition.validate()?;
        let sql_version = i64::try_from(transition.record_version)
            .map_err(|_| StoreError::Contract(invalid(transition.kind, "record_version")))?;
        let created_at = transition
            .created_at
            .format(&Rfc3339)
            .map_err(|_| StoreError::Contract(ContractError::CanonicalizationFailed))?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<StoredTransition> = transaction
            .query_row(
                "
                    SELECT
                        kind,
                        record_id,
                        from_state,
                        to_state,
                        record_version,
                        transition_digest,
                        created_at
                    FROM transitions
                    WHERE kind = ?1 AND record_id = ?2 AND record_version = ?3
                    ",
                params![
                    records::kind_key(transition.kind),
                    transition.record_id.as_str(),
                    sql_version,
                ],
                |row| {
                    Ok(StoredTransition {
                        kind: row.get(0)?,
                        record_id: row.get(1)?,
                        from_state: row.get(2)?,
                        to_state: row.get(3)?,
                        record_version: row.get(4)?,
                        transition_digest: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        if let Some(stored) = existing {
            if stored.kind == records::kind_key(transition.kind)
                && stored.record_id == transition.record_id.as_str()
                && stored.from_state == transition.from_state
                && stored.to_state == transition.to_state
                && stored.record_version == sql_version
                && stored.transition_digest == transition.transition_digest.as_str()
                && stored.created_at == created_at
            {
                transaction.commit()?;
                return Ok(());
            }
            return Err(transition_conflict(transition));
        }

        let latest: Option<(i64, String)> = transaction
            .query_row(
                "
                SELECT record_version, to_state
                FROM transitions
                WHERE kind = ?1 AND record_id = ?2
                ORDER BY record_version DESC
                LIMIT 1
                ",
                params![
                    records::kind_key(transition.kind),
                    transition.record_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let valid_position = match latest {
            None => transition.record_version == 1,
            Some((previous_version, previous_state)) => {
                let previous_version =
                    u64::try_from(previous_version).map_err(|_| StoreError::DatabaseCorruption)?;
                previous_version
                    .checked_add(1)
                    .is_some_and(|next| next == transition.record_version)
                    && transition.from_state.as_deref() == Some(previous_state.as_str())
            }
        };
        if !valid_position {
            return Err(transition_conflict(transition));
        }

        transaction.execute(
            "
            INSERT INTO transitions (
                kind,
                record_id,
                from_state,
                to_state,
                record_version,
                transition_digest,
                created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                records::kind_key(transition.kind),
                transition.record_id.as_str(),
                transition.from_state.as_deref(),
                &transition.to_state,
                sql_version,
                transition.transition_digest.as_str(),
                created_at,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Counts all immutable transition rows.
    pub fn count_transitions(&self) -> Result<u64, StoreError> {
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM transitions", [], |row| row.get(0))?;
        count.try_into().map_err(|_| StoreError::DatabaseOperation)
    }
}

fn validate_state(
    value: &str,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    let bytes = value.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(invalid(schema, field))
    }
}

fn invalid(schema: SchemaKind, field: &'static str) -> ContractError {
    ContractError::InvalidShape { schema, field }
}

fn transition_conflict(transition: &Transition) -> StoreError {
    StoreError::TransitionConflict {
        kind: transition.kind,
        record_id: transition.record_id.clone(),
        record_version: transition.record_version,
    }
}
