#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use psyche_core::digest::Sha256Digest;
use psyche_coven::{
    AdoptionRequest, ExecutionRequestInput, PortError, ReconciliationDisposition,
    ReconciliationRequest,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const LAUNCH_GOLDEN: &[u8] = include_bytes!("fixtures/execution-request-launch.json");

fn digest_of(character: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", character.to_string().repeat(64))).unwrap()
}

fn request() -> ReconciliationRequest {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    ReconciliationRequest {
        correlation: AdoptionRequest::new(input).unwrap().correlation(),
        ambiguity_digest: digest_of('d'),
        reason_code: "fence_ambiguous".to_owned(),
    }
}

fn fenced(request: &ReconciliationRequest) -> ReconciliationDisposition {
    ReconciliationDisposition::Fenced {
        disposition_id: "disp-1".to_owned(),
        fence_token: "opaque-fence".to_owned(),
        correlation: request.correlation.clone(),
        ambiguity_digest: request.ambiguity_digest.clone(),
        recorded_at: request.correlation.created_at + time::Duration::seconds(1),
    }
}

#[test]
fn exact_fenced_disposition_validates() {
    let request = request();
    let disposition = fenced(&request);
    disposition.validate_for(&request).unwrap();
}

#[test]
fn wrong_correlation_is_rejected() {
    let request = request();
    let mut other = request.clone();
    other.correlation.request_digest = digest_of('e');
    assert_eq!(
        fenced(&other).validate_for(&request),
        Err(PortError::CorrelationMismatch)
    );
}

#[test]
fn wrong_ambiguity_digest_is_rejected() {
    let request = request();
    let mut other = request.clone();
    other.ambiguity_digest = digest_of('e');
    assert_eq!(
        fenced(&other).validate_for(&request),
        Err(PortError::CorrelationMismatch)
    );
}

#[test]
fn disposition_recorded_before_correlation_is_stale() {
    let request = request();
    let mut disposition = fenced(&request);
    if let ReconciliationDisposition::Fenced { recorded_at, .. } = &mut disposition {
        *recorded_at = OffsetDateTime::parse("2000-01-01T00:00:00Z", &Rfc3339).unwrap();
    }
    assert_eq!(
        disposition.validate_for(&request),
        Err(PortError::CorrelationMismatch)
    );
}

#[test]
fn empty_fence_token_is_rejected() {
    let request = request();
    let mut disposition = fenced(&request);
    if let ReconciliationDisposition::Fenced { fence_token, .. } = &mut disposition {
        fence_token.clear();
    }
    assert_eq!(
        disposition.validate_for(&request),
        Err(PortError::InvalidResponse)
    );
}

#[test]
fn unresolved_stays_unresolved_and_proves_nothing() {
    let request = request();
    ReconciliationDisposition::Unresolved
        .validate_for(&request)
        .unwrap();
    assert_ne!(ReconciliationDisposition::Unresolved, fenced(&request));
}

mod journal {
    use super::*;
    use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
    use std::{env, path::Path, process::Command};

    const ATTEMPT: [&str; 3] = ["invocation-A", "attempt-A", "process-A"];
    const STARTS: [&str; 3] = [
        "abandoned-running",
        "termination-unknown",
        "recovery-required",
    ];
    const HOST: &str = "fixture-host-A";

    // The channel's identity models an already authenticated peer. Authentication and
    // containment are host obligations outside this fixture.
    struct Host {
        peer: &'static str,
        request: ReconciliationRequest,
        reply: Option<ReconciliationDisposition>,
    }

    impl Host {
        fn bound() -> Self {
            let request = request();
            Self {
                peer: HOST,
                reply: Some(fenced(&request)),
                request,
            }
        }

        fn replay(&self, request: &ReconciliationRequest) -> Option<&ReconciliationDisposition> {
            if &self.request == request {
                self.reply.as_ref()
            } else {
                None
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Observation {
        Blocked,
        Conflict,
        Recorded,
        Duplicate,
    }

    fn initialize(path: &Path, state: &str, bound: bool) {
        let db = Connection::open(path).unwrap();
        db.execute_batch("
            CREATE TABLE calls(kind TEXT NOT NULL);
            CREATE TABLE attempts (inv TEXT, attempt TEXT, process TEXT, state TEXT NOT NULL,
                host TEXT, correlation BLOB, ambiguity TEXT, reason TEXT, PRIMARY KEY(inv,attempt,process));
            CREATE TABLE dispositions (inv TEXT, attempt TEXT, process TEXT, value BLOB NOT NULL,
                PRIMARY KEY(inv,attempt,process));
        ").unwrap();
        let request = request();
        db.execute(
            "INSERT INTO attempts VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                ATTEMPT[0],
                ATTEMPT[1],
                ATTEMPT[2],
                state,
                bound.then_some(HOST),
                serde_json::to_vec(&request.correlation).unwrap(),
                request.ambiguity_digest.to_string(),
                request.reason_code
            ],
        )
        .unwrap();
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ExitPoint {
        None,
        BeforeCommit,
        AfterCommit,
    }

    fn reconcile(path: &Path, key: [&str; 3], host: &Host, exit: ExitPoint) -> Observation {
        let mut db = Connection::open(path).unwrap();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let binding = tx
            .query_row(
                "SELECT host, correlation, ambiguity, reason FROM attempts
             WHERE inv=?1 AND attempt=?2 AND process=?3",
                key,
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .unwrap();
        let Some((Some(peer), correlation, ambiguity, reason)) = binding else {
            return Observation::Blocked;
        };
        if peer != host.peer {
            return Observation::Blocked;
        }
        let request = ReconciliationRequest {
            correlation: serde_json::from_slice(&correlation).unwrap(),
            ambiguity_digest: Sha256Digest::parse(&ambiguity).unwrap(),
            reason_code: reason,
        };
        let Some(outcome) = host.replay(&request) else {
            return Observation::Blocked;
        };
        let ReconciliationDisposition::Fenced {
            disposition_id,
            fence_token,
            correlation,
            ambiguity_digest,
            recorded_at,
        } = outcome
        else {
            return Observation::Blocked;
        };
        if outcome.validate_for(&request).is_err() {
            return Observation::Blocked;
        }
        let value = serde_json::to_vec(&(
            "fenced",
            disposition_id,
            fence_token,
            correlation,
            ambiguity_digest,
            recorded_at.format(&Rfc3339).unwrap(),
        ))
        .unwrap();
        let retained = tx
            .query_row(
                "SELECT value FROM dispositions WHERE inv=?1 AND attempt=?2 AND process=?3",
                key,
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .unwrap();
        if let Some(retained) = retained {
            return if value == retained {
                Observation::Duplicate
            } else {
                Observation::Conflict
            };
        }
        tx.execute(
            "INSERT INTO dispositions VALUES (?1,?2,?3,?4)",
            params![key[0], key[1], key[2], value],
        )
        .unwrap();
        let changed = tx
            .execute(
                "UPDATE attempts SET state='reconciled-fenced'
             WHERE inv=?1 AND attempt=?2 AND process=?3
             AND state IN ('abandoned-running','termination-unknown','recovery-required')",
                key,
            )
            .unwrap();
        if changed != 1 {
            return Observation::Blocked;
        }
        if exit == ExitPoint::BeforeCommit {
            std::process::exit(71);
        }
        tx.commit().unwrap();
        if exit == ExitPoint::AfterCommit {
            std::process::exit(72);
        }
        Observation::Recorded
    }

    fn observe(path: &Path) -> (String, i64, i64) {
        let db = Connection::open(path).unwrap();
        let observation: (String, i64, i64) = db
            .query_row(
                "SELECT state, (SELECT count(*) FROM dispositions), (SELECT count(*) FROM attempts)
             FROM attempts WHERE inv=?1 AND attempt=?2 AND process=?3",
                ATTEMPT,
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(probe_dispatch(&db), 0);
        observation
    }

    // The test worker exercises its normal poll path on each reopened journal.
    // A separate recovery decision would have to enqueue new work.
    fn probe_dispatch(db: &Connection) -> i64 {
        for kind in ["model", "tool"] {
            db.execute(
                "INSERT INTO calls SELECT ?1 FROM attempts WHERE state='ready'",
                [kind],
            )
            .unwrap();
        }
        db.query_row("SELECT count(*) FROM calls", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn ready_control_reaches_both_dispatch_spies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("consumer.db");
        initialize(&path, "ready", true);
        assert_eq!(probe_dispatch(&Connection::open(path).unwrap()), 2);
    }

    #[test]
    fn child_process() {
        let Some(path) = env::var_os("PSYCHE_FENCING_TEST_JOURNAL") else {
            return;
        };
        let exit = match env::var("PSYCHE_FENCING_TEST_EXIT").as_deref() {
            Ok("before") => ExitPoint::BeforeCommit,
            Ok("after") => ExitPoint::AfterCommit,
            _ => panic!("missing exit point"),
        };
        reconcile(Path::new(&path), ATTEMPT, &Host::bound(), exit);
        panic!("journal worker returned before requested exit");
    }

    #[test]
    fn crashes_on_both_sides_of_commit_reconcile_once_on_fresh_open() {
        for state in STARTS {
            for (fault, code, committed) in [("before", 71, false), ("after", 72, true)] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("consumer.db");
                initialize(&path, state, true);
                let exit = Command::new(env::current_exe().unwrap())
                    .args(["--exact", "journal::child_process"])
                    .env("PSYCHE_FENCING_TEST_JOURNAL", &path)
                    .env("PSYCHE_FENCING_TEST_EXIT", fault)
                    .output()
                    .unwrap();
                assert_eq!(exit.status.code(), Some(code), "{exit:?}");
                let expected_state = if committed {
                    "reconciled-fenced"
                } else {
                    state
                };
                assert_eq!(
                    observe(&path),
                    (expected_state.to_owned(), i64::from(committed), 1)
                );
                let host = Host::bound();
                let expected = if committed {
                    Observation::Duplicate
                } else {
                    Observation::Recorded
                };
                assert_eq!(reconcile(&path, ATTEMPT, &host, ExitPoint::None), expected);
                assert_eq!(
                    reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                    Observation::Duplicate
                );
                assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
            }
        }
    }

    #[test]
    fn source_binding_and_replay_are_required_before_and_after_commit() {
        for state in STARTS {
            for bound in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("consumer.db");
                initialize(&path, state, bound);
                let host = Host::bound();
                for key in [
                    ["invocation-B", ATTEMPT[1], ATTEMPT[2]],
                    [ATTEMPT[0], "attempt-B", ATTEMPT[2]],
                    [ATTEMPT[0], ATTEMPT[1], "process-B"],
                ] {
                    assert_eq!(
                        reconcile(&path, key, &host, ExitPoint::None),
                        Observation::Blocked
                    );
                }
                for reply in [
                    Host {
                        peer: "fixture-host-B",
                        ..Host::bound()
                    },
                    Host {
                        reply: None,
                        ..Host::bound()
                    },
                    Host {
                        reply: Some(ReconciliationDisposition::Unresolved),
                        ..Host::bound()
                    },
                ] {
                    assert_eq!(
                        reconcile(&path, ATTEMPT, &reply, ExitPoint::None),
                        Observation::Blocked
                    );
                }
                if !bound {
                    assert_eq!(
                        reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                        Observation::Blocked
                    );
                    assert_eq!(observe(&path), (state.to_owned(), 0, 1));
                    continue;
                }
                assert_eq!(observe(&path), (state.to_owned(), 0, 1));
                assert_eq!(
                    reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                    Observation::Recorded
                );
                for field in ["token", "id"] {
                    let mut divergent = Host::bound();
                    let Some(ReconciliationDisposition::Fenced {
                        fence_token,
                        disposition_id,
                        ..
                    }) = divergent.reply.as_mut()
                    else {
                        unreachable!()
                    };
                    match field {
                        "token" => *fence_token = "other-token".into(),
                        _ => *disposition_id = "other-disposition".into(),
                    }
                    assert_eq!(
                        reconcile(&path, ATTEMPT, &divergent, ExitPoint::None),
                        Observation::Conflict
                    );
                }
                for reply in [None, Some(ReconciliationDisposition::Unresolved)] {
                    assert_eq!(
                        reconcile(
                            &path,
                            ATTEMPT,
                            &Host {
                                reply,
                                ..Host::bound()
                            },
                            ExitPoint::None
                        ),
                        Observation::Blocked
                    );
                }
                assert_eq!(
                    reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                    Observation::Duplicate
                );
                assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
            }
        }
    }
}
