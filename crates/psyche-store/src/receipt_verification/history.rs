use psyche_core::digest::{Sha256Digest, digest};
use rusqlite::{Connection, Transaction, params};
use serde::{
    Deserialize, Serialize,
    de::value::{Error, StrDeserializer},
};

use super::{
    BoundedBlob, INPUT_LIMIT, VerificationDecision as Decision, VerificationEvidence,
    VerificationSummary,
};
use crate::StoreError;

impl VerificationEvidence {
    pub(super) fn validate(&self, decision: Decision) -> Result<(), StoreError> {
        for bytes in [&self.receipt, &self.binding, &self.signature] {
            if bytes.as_ref().is_some_and(|b| b.len() > INPUT_LIMIT)
                || (decision.is_original() && bytes.as_ref().is_none_or(Vec::is_empty))
            {
                return Err(StoreError::InvalidVerification);
            }
        }
        Ok(())
    }

    fn missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.receipt.as_ref().is_none_or(Vec::is_empty) {
            missing.push("receipt");
        }
        if self.binding.is_none() {
            missing.push("binding");
        }
        if self.signature.is_none() {
            missing.push("signature");
        }
        missing
    }

    fn digests(&self) -> EvidenceDigests {
        EvidenceDigests {
            receipt: self.receipt.as_deref().map(Sha256Digest::from_raw_bytes),
            binding: self.binding.as_deref().map(Sha256Digest::from_raw_bytes),
            signature: self.signature.as_deref().map(Sha256Digest::from_raw_bytes),
        }
    }
}

#[derive(Serialize, PartialEq, Eq)]
struct EvidenceDigests {
    receipt: Option<Sha256Digest>,
    binding: Option<Sha256Digest>,
    signature: Option<Sha256Digest>,
}

pub(super) struct History {
    pub(super) summary: VerificationSummary,
    original_binding: Option<Sha256Digest>,
    previous: Option<(EvidenceDigests, Decision)>,
    seal: Sha256Digest,
}

impl History {
    fn effective_decision(&self, decision: Decision, evidence: &EvidenceDigests) -> Decision {
        if decision.is_original()
            && let Some(original) = self.summary.original_decision
            && (self.summary.original_receipt_digest != evidence.receipt
                || self.original_binding != evidence.binding
                || original != decision)
        {
            return Decision::ConflictingEvidence;
        }
        decision
    }

    // Link each observation to its predecessor (or the immutable registration for sequence 1).
    fn observation_digest(
        &self,
        sequence: u64,
        evidence: &EvidenceDigests,
        verifier: Decision,
        effective: Decision,
    ) -> Result<Sha256Digest, StoreError> {
        Ok(digest(&(
            &self.seal,
            sequence,
            evidence,
            verifier.code(),
            effective.code(),
        ))?)
    }

    fn apply(
        &mut self,
        evidence: EvidenceDigests,
        missing_evidence: Vec<&'static str>,
        verifier: Decision,
        effective: Decision,
        seal: Sha256Digest,
    ) {
        if effective.is_original() && self.summary.original_decision.is_none() {
            self.summary.original_decision = Some(effective);
            self.summary
                .original_receipt_digest
                .clone_from(&evidence.receipt);
            self.original_binding.clone_from(&evidence.binding);
        }
        self.summary.decision = effective;
        self.summary.missing_evidence = missing_evidence;
        self.summary.observation_count += 1;
        self.summary.quarantine_count += u64::from(effective == Decision::ConflictingEvidence);
        self.previous = Some((evidence, verifier));
        self.seal = seal;
    }

    pub(super) fn append(
        &mut self,
        tx: &Transaction<'_>,
        evidence: &VerificationEvidence,
        verifier: Decision,
    ) -> Result<(), StoreError> {
        let digests = evidence.digests();
        if self
            .previous
            .as_ref()
            .is_some_and(|(d, v)| *d == digests && *v == verifier)
        {
            return Ok(());
        }
        let effective = self.effective_decision(verifier, &digests);
        let sequence = self.summary.observation_count + 1;
        let seal = self.observation_digest(sequence, &digests, verifier, effective)?;
        let sql_sequence = i64::try_from(sequence).map_err(|_| StoreError::DatabaseOperation)?;
        tx.execute(
            "INSERT INTO receipt_verification_observations_v1
             (verification_id, sequence, receipt, binding, signature, verifier_code, effective_code, observation_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                self.summary.verification_id,
                sql_sequence,
                evidence.receipt,
                evidence.binding,
                evidence.signature,
                verifier.code(),
                effective.code(),
                seal.as_str(),
            ]
        )?;
        // Anchor the append before commit so truncation, even of the last observation, fails closed.
        let updated = tx.execute(
            "UPDATE receipt_verification_registrations_v1 SET head_digest=?1 WHERE verification_id=?2 AND head_digest=?3",
            params![seal.as_str(), self.summary.verification_id, self.seal.as_str()],
        )?;
        if updated != 1 {
            return Err(StoreError::DatabaseCorruption);
        }
        self.apply(
            digests,
            evidence.missing_fields(),
            verifier,
            effective,
            seal,
        );
        Ok(())
    }
}

pub(super) fn read(
    db: &Connection,
    id: &str,
    registration_seal: Sha256Digest,
) -> Result<History, StoreError> {
    let mut history = History {
        summary: VerificationSummary {
            verification_id: id.to_owned(),
            decision: Decision::RecoveryRequired,
            missing_evidence: VerificationEvidence::default().missing_fields(),
            observation_count: 0,
            original_receipt_digest: None,
            original_decision: None,
            quarantine_count: 0,
        },
        original_binding: None,
        previous: None,
        seal: registration_seal,
    };
    let mut stmt = db.prepare("SELECT sequence, receipt, binding, signature, verifier_code, effective_code, observation_digest
        FROM receipt_verification_observations_v1 WHERE verification_id=?1 ORDER BY sequence")?;
    let mut rows = stmt.query([id])?;
    while let Some(row) = rows.next()? {
        let sequence =
            u64::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::DatabaseCorruption)?;
        let evidence = VerificationEvidence {
            receipt: row.get::<_, Option<BoundedBlob>>(1)?.map(|b| b.0),
            binding: row.get::<_, Option<BoundedBlob>>(2)?.map(|b| b.0),
            signature: row.get::<_, Option<BoundedBlob>>(3)?.map(|b| b.0),
        };
        let code: String = row.get(4)?;
        let verifier = Decision::deserialize(StrDeserializer::<Error>::new(&code))
            .map_err(|_| StoreError::DatabaseCorruption)?;
        evidence
            .validate(verifier)
            .map_err(|_| StoreError::DatabaseCorruption)?;
        let digests = evidence.digests();
        let effective = history.effective_decision(verifier, &digests);
        let stored_effective: String = row.get(5)?;
        let stored_seal: String = row.get(6)?;
        let seal = history.observation_digest(sequence, &digests, verifier, effective)?;
        if sequence != history.summary.observation_count + 1
            || stored_effective != effective.code()
            || seal.as_str() != stored_seal
        {
            return Err(StoreError::DatabaseCorruption);
        }
        history.apply(
            digests,
            evidence.missing_fields(),
            verifier,
            effective,
            seal,
        );
    }
    let head: String = db.query_row(
        "SELECT head_digest FROM receipt_verification_registrations_v1 WHERE verification_id=?1",
        [id],
        |r| r.get(0),
    )?;
    if head != history.seal.as_str() {
        return Err(StoreError::DatabaseCorruption);
    }
    Ok(history)
}
