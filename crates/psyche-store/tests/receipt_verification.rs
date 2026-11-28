#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

use psyche_core::digest::Sha256Digest;
use psyche_store::{
    Store, StoreError,
    receipt_verification::{
        VerificationDecision as Decision, VerificationEvidence, VerificationInput,
    },
};
use rusqlite::Connection;

fn database() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private").join("psyche.sqlite3");
    (root, path)
}

fn input() -> VerificationInput {
    VerificationInput {
        request: include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json")
            .to_vec(),
        context: include_bytes!("../../../profiles/build-receipt-binding/golden/context.json")
            .to_vec(),
        allowed_signers: include_bytes!(
            "../../../profiles/build-receipt-binding/golden/allowed_signers"
        )
        .to_vec(),
    }
}

fn evidence() -> VerificationEvidence {
    VerificationEvidence {
        receipt: Some(
            include_bytes!("../../../profiles/build-receipt-binding/golden/receipt.json").to_vec(),
        ),
        binding: Some(
            include_bytes!("../../../profiles/build-receipt-binding/golden/binding.json").to_vec(),
        ),
        signature: Some(
            include_bytes!("../../../profiles/build-receipt-binding/golden/binding.sig").to_vec(),
        ),
    }
}

#[test]
fn registrations_reopen_with_immutable_independent_trust_and_context() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let original = input();
    let first = store.register_verification(&original).unwrap();
    assert_eq!(first.decision, Decision::RecoveryRequired);
    assert_eq!(first.observation_count, 0);
    assert_eq!(store.register_verification(&original).unwrap(), first);

    let mut replacement = input();
    replacement.allowed_signers = b"replacement".to_vec();
    assert!(matches!(
        store.register_verification(&replacement),
        Err(StoreError::VerificationRegistrationConflict)
    ));

    let mut changed = input();
    changed.context = String::from_utf8(changed.context)
        .unwrap()
        .replace("terminal", "ambiguous")
        .into_bytes();
    assert!(matches!(
        store.register_verification(&changed),
        Err(StoreError::VerificationRegistrationConflict)
    ));

    let mut independent = input();
    independent.context = String::from_utf8(independent.context)
        .unwrap()
        .replace("fixture-task", "other-task")
        .into_bytes();
    let second = store.register_verification(&independent).unwrap();
    assert_ne!(first.verification_id, second.verification_id);

    independent.request = String::from_utf8(independent.request)
        .unwrap()
        .replace("principal:val", "principal:other")
        .into_bytes();
    assert!(matches!(
        store.register_verification(&independent),
        Err(StoreError::VerificationRegistrationConflict)
    ));

    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.verification_input(&first.verification_id).unwrap(),
        original
    );
    assert_eq!(
        store.inspect_verification(&second.verification_id).unwrap(),
        second
    );
}

#[test]
fn ambiguous_original_survives_refusals_authenticated_conflict_and_restart() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let id = store
        .register_verification(&input())
        .unwrap()
        .verification_id;
    let first = evidence();
    let original = store
        .append_verification(&id, &first, Decision::ReconciliationRequired)
        .unwrap();
    assert_eq!(
        original.original_decision,
        Some(Decision::ReconciliationRequired)
    );

    let mut other = first.clone();
    other.receipt = Some(b"different receipt".to_vec());
    let conflict = store
        .append_verification(&id, &other, Decision::BindingVerified)
        .unwrap();
    assert_eq!(conflict.decision, Decision::ConflictingEvidence);
    assert_eq!(conflict.quarantine_count, 1);
    assert_eq!(
        store
            .append_verification(&id, &other, Decision::BindingVerified)
            .unwrap(),
        conflict
    );

    let missing = VerificationEvidence::default();
    assert!(matches!(
        store.append_verification(&id, &missing, Decision::BindingVerified),
        Err(StoreError::InvalidVerification)
    ));
    let result = store
        .append_verification(&id, &missing, Decision::RecoveryRequired)
        .unwrap();
    assert_eq!(result.decision, Decision::RecoveryRequired);
    assert_eq!(
        result.original_receipt_digest,
        original.original_receipt_digest
    );
    assert_eq!(result.observation_count, 3);
    store.prune(time::OffsetDateTime::now_utc()).unwrap();
    assert_eq!(store.inspect_verification(&id).unwrap(), result);
    drop(store);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.inspect_verification(&id).unwrap(), result);
    let db = Connection::open(&path).unwrap();
    let bytes: (Vec<u8>, Vec<u8>, Vec<u8>, String) = db.query_row(
        "SELECT receipt, binding, signature, effective_code FROM receipt_verification_observations_v1 WHERE verification_id=?1 AND sequence=2",
        [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    ).unwrap();
    assert_eq!(
        bytes,
        (
            other.receipt.unwrap(),
            other.binding.unwrap(),
            other.signature.unwrap(),
            "conflicting_evidence".into()
        )
    );
}

#[test]
fn concurrent_accepted_receipts_leave_one_original_after_reopen() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let id = store
        .register_verification(&input())
        .unwrap()
        .verification_id;
    drop(store);

    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
        .into_iter()
        .map(|receipt| {
            let path = path.clone();
            let id = id.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                let mut observation = evidence();
                observation.receipt = Some(receipt.to_vec());
                barrier.wait();
                (
                    receipt,
                    store
                        .append_verification(&id, &observation, Decision::BindingVerified)
                        .unwrap()
                        .decision,
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = workers.into_iter().map(|t| t.join().unwrap()).collect();
    let winner = outcomes
        .iter()
        .find(|(_, decision)| *decision == Decision::BindingVerified)
        .unwrap();
    assert_eq!(
        outcomes
            .iter()
            .filter(|(_, d)| *d == Decision::ConflictingEvidence)
            .count(),
        1
    );
    let summary = Store::open(&path)
        .unwrap()
        .inspect_verification(&id)
        .unwrap();
    assert_eq!(
        summary.original_receipt_digest,
        Some(Sha256Digest::from_raw_bytes(winner.0))
    );
    assert_eq!(summary.observation_count, 2);
    assert_eq!(summary.quarantine_count, 1);
}

#[test]
fn oversize_inputs_and_tampered_evidence_fail_closed() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let mut oversized = input();
    oversized.allowed_signers.resize(65_537, b'x');
    assert!(matches!(
        store.register_verification(&oversized),
        Err(StoreError::InvalidVerification)
    ));
    let id = store
        .register_verification(&input())
        .unwrap()
        .verification_id;
    let original = evidence();
    let mut oversized = original.clone();
    oversized.signature = Some(vec![0; 65_537]);
    assert!(matches!(
        store.append_verification(&id, &oversized, Decision::UntrustedIssuer),
        Err(StoreError::InvalidVerification)
    ));
    store
        .append_verification(&id, &original, Decision::BindingVerified)
        .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE receipt_verification_observations_v1 SET receipt=x'00' WHERE verification_id=?1",
        [&id],
    )
    .unwrap();
    assert!(matches!(
        store.inspect_verification(&id),
        Err(StoreError::DatabaseCorruption)
    ));
    assert!(matches!(
        store.append_verification(&id, &original, Decision::BindingVerified),
        Err(StoreError::DatabaseCorruption)
    ));
}

#[test]
fn adjunct_is_optional_and_unknown_or_incomplete_schema_fails_closed() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let objects: u32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'receipt_verification_*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(objects, 0);
    let id = store
        .register_verification(&input())
        .unwrap()
        .verification_id;
    assert_eq!(store.schema_version().unwrap(), 1);
    store
        .append_verification(&id, &evidence(), Decision::BindingVerified)
        .unwrap();
    db.execute(
        "DELETE FROM receipt_verification_observations_v1 WHERE verification_id=?1",
        [&id],
    )
    .unwrap();
    assert!(matches!(
        store.inspect_verification(&id),
        Err(StoreError::DatabaseCorruption)
    ));
    drop(store);
    db.execute("DROP TABLE receipt_verification_observations_v1", [])
        .unwrap();
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::DatabaseCorruption)
    ));

    let (_root, future_path) = database();
    drop(Store::open(&future_path).unwrap());
    let future = Connection::open(&future_path).unwrap();
    future
        .execute(
            "CREATE TABLE receipt_verification_registrations_v2 (value INTEGER) STRICT",
            [],
        )
        .unwrap();
    assert!(matches!(
        Store::open(&future_path),
        Err(StoreError::DatabaseCorruption)
    ));
}

#[test]
fn denials_and_preexisting_conflicts_do_not_establish_an_original() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let id = store
        .register_verification(&input())
        .unwrap()
        .verification_id;
    let good = evidence();
    let mut bad = good.clone();
    bad.signature = Some(Vec::new());
    let denied = store
        .append_verification(&id, &bad, Decision::UntrustedIssuer)
        .unwrap();
    assert_eq!(denied.original_receipt_digest, None);
    let quarantined = store
        .append_verification(&id, &bad, Decision::ConflictingEvidence)
        .unwrap();
    assert_eq!(quarantined.original_receipt_digest, None);
    assert_eq!(quarantined.quarantine_count, 1);
    let accepted = store
        .append_verification(&id, &good, Decision::BindingVerified)
        .unwrap();
    assert_eq!(accepted.original_decision, Some(Decision::BindingVerified));
    let ambiguous = store
        .append_verification(&id, &good, Decision::ReconciliationRequired)
        .unwrap();
    assert_eq!(ambiguous.decision, Decision::ConflictingEvidence);
    assert_eq!(ambiguous.original_decision, Some(Decision::BindingVerified));
    assert_eq!(
        ambiguous.original_receipt_digest,
        accepted.original_receipt_digest
    );
    drop(store);

    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE receipt_verification_registrations_v1 SET allowed_signers=x'00' WHERE verification_id=?1", [&id]).unwrap();
    let store = Store::open(&path).unwrap();
    assert!(matches!(
        store.verification_input(&id),
        Err(StoreError::DatabaseCorruption)
    ));
}

#[test]
fn missing_evidence_reflects_only_the_latest_snapshots_after_reopen() {
    let (_root, path) = database();
    let mut store = Store::open(&path).unwrap();
    let initial = store.register_verification(&input()).unwrap();
    assert_eq!(
        initial.missing_evidence,
        ["receipt", "binding", "signature"]
    );
    let id = initial.verification_id;
    let accepted = store
        .append_verification(&id, &evidence(), Decision::BindingVerified)
        .unwrap();
    assert!(accepted.missing_evidence.is_empty());

    let incomplete = VerificationEvidence {
        receipt: Some(Vec::new()),
        binding: None,
        signature: Some(Vec::new()),
    };
    let observed = store
        .append_verification(&id, &incomplete, Decision::RecoveryRequired)
        .unwrap();
    assert_eq!(observed.missing_evidence, ["receipt", "binding"]);
    assert_eq!(observed.original_decision, Some(Decision::BindingVerified));
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.inspect_verification(&id).unwrap(), observed);
    assert_eq!(
        store
            .append_verification(&id, &incomplete, Decision::RecoveryRequired)
            .unwrap(),
        observed
    );

    let malformed = VerificationEvidence {
        receipt: Some(vec![b'r']),
        binding: Some(Vec::new()),
        signature: None,
    };
    let observed = store
        .append_verification(&id, &malformed, Decision::InvalidBinding)
        .unwrap();
    assert_eq!(observed.missing_evidence, ["signature"]);
}
