use rusqlite::{TransactionBehavior, params};
use time::format_description::well_known::Rfc3339;

use crate::{Store, StoreError, execution_bindings, quarantine, records, transitions};

/// Counts returned by one conservative retention pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PruneReport {
    /// Fully resolved quarantine rows older than the exact cutoff.
    pub resolved_quarantine_deleted: u64,
    /// Always zero: automated retention never deletes unresolved quarantine.
    pub unresolved_quarantine_deleted: u64,
    /// Always zero: immutable execution-binding history is retained.
    pub execution_binding_revisions_deleted: u64,
    /// Always zero: immutable transition history is retained.
    pub transitions_deleted: u64,
    /// Always zero: opaque audit correlations are retained.
    pub audit_events_deleted: u64,
}

impl Store {
    /// Deletes only fully resolved quarantine rows strictly older than `cutoff`.
    pub fn prune(&mut self, cutoff: time::OffsetDateTime) -> Result<PruneReport, StoreError> {
        if cutoff.offset() != time::UtcOffset::UTC {
            return Err(StoreError::InvalidRetentionCutoff);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        records::validate_all(&transaction)?;
        execution_bindings::validate_all(&transaction)?;
        transitions::validate_all(&transaction)?;
        let quarantine = quarantine::all_records(&transaction)?;
        quarantine::audit_events_from_connection(&transaction)?;

        let mut resolved_quarantine_deleted = 0_u64;
        for record in quarantine {
            let (Some(resolved_at), Some(resolution_code), Some(resolution_digest)) = (
                record.resolved_at,
                record.resolution_code,
                record.resolution_digest,
            ) else {
                continue;
            };
            if resolved_at >= cutoff {
                continue;
            }
            let resolved_at = resolved_at
                .format(&Rfc3339)
                .map_err(|_| StoreError::DatabaseCorruption)?;
            let deleted = transaction.execute(
                "
                DELETE FROM quarantine_records
                WHERE quarantine_id = ?1
                  AND resolved_at = ?2
                  AND resolution_code = ?3
                  AND resolution_digest = ?4
                ",
                params![
                    record.quarantine_id.as_str(),
                    resolved_at,
                    resolution_code_key(resolution_code),
                    resolution_digest.as_str(),
                ],
            )?;
            if deleted != 1 {
                return Err(StoreError::DatabaseCorruption);
            }
            resolved_quarantine_deleted = resolved_quarantine_deleted
                .checked_add(u64::try_from(deleted).map_err(|_| StoreError::DatabaseOperation)?)
                .ok_or(StoreError::DatabaseOperation)?;
        }

        transaction.commit()?;
        Ok(PruneReport {
            resolved_quarantine_deleted,
            ..PruneReport::default()
        })
    }

    /// Forces a truncating WAL checkpoint without changing logical rows.
    pub fn checkpoint(&mut self) -> Result<(), StoreError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) =
            self.connection
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        if busy != 0 || log_frames < 0 || checkpointed_frames < 0 {
            return Err(StoreError::DatabaseOperation);
        }
        Ok(())
    }
}

fn resolution_code_key(code: crate::QuarantineResolutionCode) -> &'static str {
    match code {
        crate::QuarantineResolutionCode::SchemaNowSupported => "schema_now_supported",
        crate::QuarantineResolutionCode::ConfirmedInvalid => "confirmed_invalid",
        crate::QuarantineResolutionCode::DuplicatePayload => "duplicate_payload",
    }
}
