//! Local, opt-in ledger for the Build receipt verifier. These handles and observations are not protocol record kinds.

mod history;

use std::fmt;

use psyche_core::{
    contracts::build_receipt::BuildReference,
    digest::{Sha256Digest, digest},
    id::RequestId,
};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;

use crate::{Store, StoreError};

const SCHEMA: &str = include_str!("schema.sql");
/// Maximum byte count for each independently snapshotted verifier input.
pub const INPUT_LIMIT: usize = 65_536;

// Check SQLite's borrowed bytes before copying, even if the database was damaged outside this writer.
struct BoundedBlob(Vec<u8>);

impl FromSql for BoundedBlob {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let bytes = value.as_blob()?;
        if bytes.len() > INPUT_LIMIT {
            return Err(FromSqlError::InvalidBlobSize {
                expected_size: INPUT_LIMIT,
                blob_size: bytes.len(),
            });
        }
        Ok(Self(bytes.to_vec()))
    }
}

/// Immutable submission from the operator's trusted request, native context and signer policy.
#[derive(Clone, PartialEq, Eq)]
pub struct VerificationInput {
    /// Original canonical execution request.
    pub request: Vec<u8>,
    /// Independently obtained Build journal context.
    pub context: Vec<u8>,
    /// Operator-provisioned OpenSSH allowed signers file.
    pub allowed_signers: Vec<u8>,
}

impl fmt::Debug for VerificationInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerificationInput").finish_non_exhaustive()
    }
}

/// Snapshots passed to the pinned verifier. Absence is distinct from a present, empty file.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct VerificationEvidence {
    /// Exact native journal receipt.
    pub receipt: Option<Vec<u8>>,
    /// Proposed canonical signed message.
    pub binding: Option<Vec<u8>>,
    /// Detached signature over the proposed message.
    pub signature: Option<Vec<u8>>,
}

impl fmt::Debug for VerificationEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerificationEvidence")
            .finish_non_exhaustive()
    }
}

/// Closed decision vocabulary for this verifier profile. No decision authorizes dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationDecision {
    /// The binding and terminal journal bytes agree.
    BindingVerified,
    /// Required evidence is missing.
    RecoveryRequired,
    /// Authenticated evidence correlates, but its native outcome is ambiguous.
    ReconciliationRequired,
    /// Authenticated correlation, receipt, or original outcome disagrees.
    ConflictingEvidence,
    /// Unreadable or malformed native context.
    InvalidContext,
    /// Unreadable or malformed binding.
    InvalidBinding,
    /// Unreadable or malformed canonical request.
    InvalidRequest,
    /// Unreadable or oversized receipt.
    InvalidReceipt,
    /// The declared version is outside the negotiated profile.
    UnsupportedVersion,
    /// The issuer or signature failed the independently retained policy.
    UntrustedIssuer,
    /// The verifier could not complete.
    VerifierUnavailable,
}

impl VerificationDecision {
    /// Profile disposition emitted by the verifier.
    pub fn result(self) -> &'static str {
        match self {
            Self::BindingVerified => "verified",
            Self::RecoveryRequired | Self::ReconciliationRequired => "unknown",
            Self::ConflictingEvidence => "quarantine",
            _ => "denied",
        }
    }

    /// Stable, payload-free explanation for this disposition.
    pub fn code(self) -> &'static str {
        match self {
            Self::BindingVerified => "binding_verified",
            Self::RecoveryRequired => "recovery_required",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::ConflictingEvidence => "conflicting_evidence",
            Self::InvalidContext => "invalid_context",
            Self::InvalidBinding => "invalid_binding",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidReceipt => "invalid_receipt",
            Self::UnsupportedVersion => "unsupported_version",
            Self::UntrustedIssuer => "untrusted_issuer",
            Self::VerifierUnavailable => "verifier_unavailable",
        }
    }

    fn is_original(self) -> bool {
        matches!(self, Self::BindingVerified | Self::ReconciliationRequired)
    }
}

/// Bounded status projection. Original evidence stays private and immutable in the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationSummary {
    /// Local handle scoped to a Psyche request and its native Build references.
    pub verification_id: String,
    /// Most recent effective observation, or recovery required before any observation.
    pub decision: VerificationDecision,
    /// Names of evidence missing from the latest observation. An empty native receipt requires recovery.
    pub missing_evidence: Vec<&'static str>,
    /// Number of durable observations.
    pub observation_count: u64,
    /// SHA-256 of the first authenticated, correlated receipt, including an ambiguous receipt.
    pub original_receipt_digest: Option<Sha256Digest>,
    /// Disposition when the original correlated receipt was first authenticated.
    pub original_decision: Option<VerificationDecision>,
    /// Number of durable observations adjudicated as conflicts.
    pub quarantine_count: u64,
}

#[derive(Deserialize)]
struct RequestKey {
    request_id: RequestId,
}

#[derive(Deserialize)]
struct NativeKey {
    task_id: BuildReference,
    #[serde(default, deserialize_with = "BuildReference::present")]
    action_id: Option<BuildReference>,
    #[serde(default, deserialize_with = "BuildReference::present")]
    idempotency_key: Option<BuildReference>,
}

struct Registration {
    request_id: RequestId,
    request_digest: Sha256Digest,
    id: String,
    seal: Sha256Digest,
}

impl VerificationInput {
    // The runtime validates the full pinned request and context shapes. For the lookup identity
    // the store uses only typed references read from these original bytes, never from a binding.
    fn registration(&self) -> Result<Registration, StoreError> {
        if self.request.len() > INPUT_LIMIT
            || self.context.len() > INPUT_LIMIT
            || self.allowed_signers.len() > INPUT_LIMIT
        {
            return Err(StoreError::InvalidVerification);
        }
        let request: RequestKey =
            serde_json::from_slice(&self.request).map_err(|_| StoreError::InvalidVerification)?;
        let context: NativeKey =
            serde_json::from_slice(&self.context).map_err(|_| StoreError::InvalidVerification)?;
        if context.action_id.is_none() && context.idempotency_key.is_none() {
            return Err(StoreError::InvalidVerification);
        }
        let lookup = digest(&(
            "psyche-local-build-verification-v1",
            &request.request_id,
            context.task_id,
            context.action_id,
            context.idempotency_key,
        ))?;
        let id = format!("vfy_{}", &lookup.as_str()[Sha256Digest::PREFIX.len()..]);
        let request_digest = Sha256Digest::from_raw_bytes(&self.request);
        let seal = digest(&(
            &id,
            &request_digest,
            Sha256Digest::from_raw_bytes(&self.context),
            Sha256Digest::from_raw_bytes(&self.allowed_signers),
        ))?;
        Ok(Registration {
            request_id: request.request_id,
            request_digest,
            id,
            seal,
        })
    }
}

// Older readers allow inert additional tables and preserve them through prune. New readers
// require all three definitions, or none, and reject unknown adjunct versions and objects.
pub(super) fn schema_present(db: &Connection) -> Result<bool, StoreError> {
    let mut stmt = db.prepare(
        "SELECT name, sql FROM sqlite_schema
         WHERE (name GLOB 'receipt_verification_*' OR tbl_name GLOB 'receipt_verification_*')
           AND NOT (type = 'index' AND sql IS NULL)",
    )?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let sql: String = row.get(1)?;
        if !SCHEMA
            .split_terminator(';')
            .map(str::trim)
            .any(|s| s.starts_with(&format!("CREATE TABLE {name} (")) && s == sql)
        {
            return Err(StoreError::DatabaseCorruption);
        }
        count += 1;
    }
    match count {
        0 => Ok(false),
        3 => Ok(true),
        _ => Err(StoreError::DatabaseCorruption),
    }
}

impl Store {
    /// Registers a request with independently obtained context and policy. An identical retry returns the existing status.
    pub fn register_verification(
        &mut self,
        input: &VerificationInput,
    ) -> Result<VerificationSummary, StoreError> {
        let reg = input.registration()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !schema_present(&tx)? {
            tx.execute_batch(SCHEMA)?;
        }
        tx.execute(
            "INSERT INTO receipt_verification_requests_v1(request_id, request, request_digest)
             VALUES (?1, ?2, ?3) ON CONFLICT(request_id) DO NOTHING",
            params![
                reg.request_id.as_str(),
                input.request,
                reg.request_digest.as_str()
            ],
        )?;
        let (stored_request, stored_digest): (Vec<u8>, String) = tx.query_row(
            "SELECT request, request_digest FROM receipt_verification_requests_v1 WHERE request_id = ?1",
            [reg.request_id.as_str()],
            |r| Ok((r.get::<_, BoundedBlob>(0)?.0, r.get(1)?)),
        )?;
        if Sha256Digest::from_raw_bytes(&stored_request).as_str() != stored_digest {
            return Err(StoreError::DatabaseCorruption);
        }
        if stored_request != input.request {
            return Err(StoreError::VerificationRegistrationConflict);
        }

        tx.execute(
            "INSERT INTO receipt_verification_registrations_v1
             (verification_id, request_id, context, allowed_signers, registration_digest, head_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5) ON CONFLICT(verification_id) DO NOTHING",
            params![
                reg.id,
                reg.request_id.as_str(),
                input.context,
                input.allowed_signers,
                reg.seal.as_str(),
            ],
        )?;
        if read_input(&tx, &reg.id)? != *input {
            return Err(StoreError::VerificationRegistrationConflict);
        }
        let summary = history::read(&tx, &reg.id, reg.seal)?.summary;
        tx.commit()?;
        Ok(summary)
    }

    /// Reads the original trusted inputs for verification, rechecking identity and snapshot digests.
    pub fn verification_input(&self, id: &str) -> Result<VerificationInput, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let input = read_input(&tx, id)?;
        tx.commit()?;
        Ok(input)
    }

    /// Reads the current outcome from a single consistent snapshot, including after process restart.
    pub fn inspect_verification(&self, id: &str) -> Result<VerificationSummary, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let (_input, reg) = read_registration(&tx, id)?;
        let summary = history::read(&tx, id, reg.seal)?.summary;
        tx.commit()?;
        Ok(summary)
    }

    /// Appends the pinned verifier's decision for exactly these already-snapshotted bytes. Checks original-receipt conflicts in the same writer transaction.
    pub fn append_verification(
        &mut self,
        id: &str,
        evidence: &VerificationEvidence,
        decision: VerificationDecision,
    ) -> Result<VerificationSummary, StoreError> {
        evidence.validate(decision)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (input, reg) = read_registration(&tx, id)?;
        if decision.is_original() && input.allowed_signers.is_empty() {
            return Err(StoreError::InvalidVerification);
        }
        let mut history = history::read(&tx, id, reg.seal)?;
        history.append(&tx, evidence, decision)?;
        tx.commit()?;
        Ok(history.summary)
    }
}

fn read_input(db: &Connection, id: &str) -> Result<VerificationInput, StoreError> {
    read_registration(db, id).map(|(input, _)| input)
}

fn read_registration(
    db: &Connection,
    id: &str,
) -> Result<(VerificationInput, Registration), StoreError> {
    if !id.strip_prefix("vfy_").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(StoreError::InvalidVerification);
    }
    if !schema_present(db)? {
        return Err(StoreError::VerificationNotFound);
    }
    let (input, request_id, request_digest, seal): (VerificationInput, String, String, String) = db
        .query_row(
            "SELECT q.request, r.context, r.allowed_signers, r.request_id, q.request_digest, r.registration_digest
             FROM receipt_verification_registrations_v1 r
             LEFT JOIN receipt_verification_requests_v1 q ON q.request_id = r.request_id
             WHERE r.verification_id = ?1",
            [id],
            |r| {
                let input = VerificationInput {
                    request: r.get::<_, BoundedBlob>(0)?.0,
                    context: r.get::<_, BoundedBlob>(1)?.0,
                    allowed_signers: r.get::<_, BoundedBlob>(2)?.0,
                };
                Ok((input, r.get(3)?, r.get(4)?, r.get(5)?))
            },
        )
        .optional()?
        .ok_or(StoreError::VerificationNotFound)?;
    let reg = input
        .registration()
        .map_err(|_| StoreError::DatabaseCorruption)?;
    if reg.id != id
        || reg.request_id.as_str() != request_id
        || reg.request_digest.as_str() != request_digest
        || reg.seal.as_str() != seal
    {
        return Err(StoreError::DatabaseCorruption);
    }
    Ok((input, reg))
}

// Retention validates the complete ledger before it deletes any eligible foundation rows.
// It never ages out receipt verification evidence.
pub(super) fn validate_all(db: &Connection) -> Result<(), StoreError> {
    if !schema_present(db)? {
        return Ok(());
    }
    let orphaned: bool = db.query_row(
        "SELECT EXISTS (
             SELECT request_id FROM receipt_verification_requests_v1
             EXCEPT SELECT request_id FROM receipt_verification_registrations_v1
         ) OR EXISTS (
             SELECT verification_id FROM receipt_verification_observations_v1
             EXCEPT SELECT verification_id FROM receipt_verification_registrations_v1
         )",
        [],
        |r| r.get(0),
    )?;
    if orphaned {
        return Err(StoreError::DatabaseCorruption);
    }
    let mut statement =
        db.prepare("SELECT verification_id FROM receipt_verification_registrations_v1")?;
    let ids = statement.query_map([], |r| r.get::<_, String>(0))?;
    for id in ids {
        let id = id?;
        let (_, reg) = read_registration(db, &id)?;
        history::read(db, &id, reg.seal)?;
    }
    Ok(())
}
