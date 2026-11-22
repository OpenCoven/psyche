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

// Everything in this module is a consumer model written for the fixture. The
// only Psyche API it calls is `ReconciliationRequest::validate`,
// `ReconciliationDisposition::validate_for` and the `ExecutionCorrelation`
// serde round-trip. No `coven-agents` code runs here.
mod journal {
    use super::*;
    use psyche_coven::ExecutionCorrelation;
    use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
    use std::{cell::Cell, env, path::Path, process::Command, time::Duration};

    // The consumer's local lookup key. It selects a binding and nothing else:
    // no retained record is keyed by it.
    const ATTEMPT: [&str; 3] = ["invocation-A", "attempt-A", "process-A"];
    const STARTS: [&str; 3] = [
        "abandoned-running",
        "termination-unknown",
        "recovery-required",
    ];
    const INELIGIBLE: [&str; 5] = [
        "running",
        "ready",
        "completed",
        "reconciled-fenced",
        "not-a-known-state",
    ];
    const HOST: &str = "fixture-host-A";
    const JOURNAL_ENV: &str = "PSYCHE_FENCING_TEST_JOURNAL";
    const EXIT_ENV: &str = "PSYCHE_FENCING_TEST_EXIT";

    // The channel's identity models an already authenticated peer: the peer
    // check below compares two strings. Authentication and containment are host
    // obligations outside this fixture.
    struct Host {
        peer: &'static str,
        request: ReconciliationRequest,
        reply: Option<ReconciliationDisposition>,
        replays: Cell<u32>,
    }

    impl Host {
        fn bound() -> Self {
            let request = request();
            Self {
                peer: HOST,
                reply: Some(fenced(&request)),
                request,
                replays: Cell::new(0),
            }
        }

        fn replay(&self, request: &ReconciliationRequest) -> Option<&ReconciliationDisposition> {
            self.replays.set(self.replays.get() + 1);
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

    fn request_id(request: &ReconciliationRequest) -> &str {
        request.correlation.request_id.as_str()
    }

    fn open(path: &Path) -> Connection {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
    }

    // `requests`, `dispositions` and `history` are keyed by the host-supplied
    // request identity. `bindings` maps the local key to that identity. Every
    // table rejects UPDATE and DELETE; the current state is the latest history
    // row that carries one.
    fn create(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "
            CREATE TABLE calls(kind TEXT NOT NULL);
            CREATE TABLE requests(request_id TEXT PRIMARY KEY, correlation BLOB NOT NULL,
                ambiguity TEXT NOT NULL, reason TEXT NOT NULL);
            CREATE TABLE bindings(inv TEXT, attempt TEXT, process TEXT, host TEXT,
                request_id TEXT NOT NULL, PRIMARY KEY(inv,attempt,process));
            CREATE TABLE dispositions(request_id TEXT PRIMARY KEY, value BLOB NOT NULL);
            CREATE TABLE history(seq INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT NOT NULL,
                event TEXT NOT NULL, state TEXT, value BLOB);
            CREATE VIEW current AS SELECT request_id, state FROM history AS latest
                WHERE state IS NOT NULL AND seq = (SELECT max(seq) FROM history
                    WHERE request_id = latest.request_id AND state IS NOT NULL);
            ",
        )
        .unwrap();
        for table in ["requests", "bindings", "dispositions", "history"] {
            for change in ["UPDATE", "DELETE"] {
                db.execute_batch(&format!(
                    "CREATE TRIGGER {table}_rejects_{change} BEFORE {change} ON {table}
                     BEGIN SELECT RAISE(ABORT, 'append-only journal'); END;"
                ))
                .unwrap();
            }
        }
        db
    }

    // Binds one local key to one host-supplied request and records its start
    // state. A request that already has a binding refuses a second one.
    fn bind_raw(
        db: &Connection,
        key: [&str; 3],
        host: Option<&str>,
        (request_id, correlation, ambiguity, reason): (&str, &[u8], &str, &str),
        state: &str,
    ) -> bool {
        let tx = db.unchecked_transaction().unwrap();
        let bound = tx
            .execute(
                "INSERT INTO requests VALUES (?1,?2,?3,?4)",
                params![request_id, correlation, ambiguity, reason],
            )
            .is_ok()
            && tx
                .execute(
                    "INSERT INTO bindings VALUES (?1,?2,?3,?4,?5)",
                    params![key[0], key[1], key[2], host, request_id],
                )
                .is_ok()
            && tx
                .execute(
                    "INSERT INTO history(request_id,event,state) VALUES (?1,'bound',?2)",
                    params![request_id, state],
                )
                .is_ok();
        if bound {
            tx.commit().unwrap();
        }
        bound
    }

    fn bind(
        db: &Connection,
        key: [&str; 3],
        host: Option<&str>,
        request: &ReconciliationRequest,
        state: &str,
    ) -> bool {
        bind_raw(
            db,
            key,
            host,
            (
                request_id(request),
                &serde_json::to_vec(&request.correlation).unwrap(),
                &request.ambiguity_digest.to_string(),
                &request.reason_code,
            ),
            state,
        )
    }

    fn initialize(path: &Path, state: &str, bound: bool) {
        let db = create(path);
        assert!(bind(&db, ATTEMPT, bound.then_some(HOST), &request(), state));
    }

    fn decode(value: &[u8]) -> ReconciliationDisposition {
        let (kind, disposition_id, fence_token, correlation, ambiguity_digest, recorded_at): (
            String,
            String,
            String,
            ExecutionCorrelation,
            Sha256Digest,
            String,
        ) = serde_json::from_slice(value).unwrap();
        assert_eq!(kind, "fenced");
        ReconciliationDisposition::Fenced {
            disposition_id,
            fence_token,
            correlation,
            ambiguity_digest,
            recorded_at: OffsetDateTime::parse(&recorded_at, &Rfc3339).unwrap(),
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ExitPoint {
        None,
        BeforeCommit,
        AfterCommit,
    }

    fn reconcile(path: &Path, key: [&str; 3], host: &Host, exit: ExitPoint) -> Observation {
        let mut db = open(path);
        db.busy_timeout(Duration::ZERO).unwrap();
        // The journal's write lock is held before the host is asked, so the
        // check for a retained disposition and its insert cannot interleave
        // with another writer. A journal that is busy stays blocked.
        let Ok(tx) = db.transaction_with_behavior(TransactionBehavior::Immediate) else {
            return Observation::Blocked;
        };
        let binding = tx
            .query_row(
                "SELECT host, request_id, correlation, ambiguity, reason
                 FROM bindings JOIN requests USING (request_id)
                 WHERE inv=?1 AND attempt=?2 AND process=?3",
                key,
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .unwrap();
        let Some((Some(peer), stored_id, correlation, ambiguity, reason)) = binding else {
            return Observation::Blocked;
        };
        if peer != host.peer {
            return Observation::Blocked;
        }
        // A stored request that no longer decodes, no longer validates or is
        // filed under another identity cannot be reconciled.
        let (Ok(correlation), Ok(ambiguity_digest)) = (
            serde_json::from_slice::<ExecutionCorrelation>(&correlation),
            Sha256Digest::parse(&ambiguity),
        ) else {
            return Observation::Blocked;
        };
        let request = ReconciliationRequest {
            correlation,
            ambiguity_digest,
            reason_code: reason,
        };
        if request.validate().is_err() || request_id(&request) != stored_id {
            return Observation::Blocked;
        }
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
        // The retained disposition is keyed by the request identity alone.
        let retained = tx
            .query_row(
                "SELECT value FROM dispositions WHERE request_id=?1",
                [&stored_id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .unwrap();
        if let Some(retained) = retained {
            if value == retained {
                return Observation::Duplicate;
            }
            tx.execute(
                "INSERT INTO history(request_id,event,value) VALUES (?1,'conflict',?2)",
                params![stored_id, value],
            )
            .unwrap();
            tx.commit().unwrap();
            return Observation::Conflict;
        }
        tx.execute(
            "INSERT INTO dispositions VALUES (?1,?2)",
            params![stored_id, value],
        )
        .unwrap();
        let changed = tx
            .execute(
                "INSERT INTO history(request_id,event,state)
                 SELECT request_id,'transition','reconciled-fenced' FROM current
                 WHERE request_id=?1
                 AND state IN ('abandoned-running','termination-unknown','recovery-required')",
                [&stored_id],
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

    // Current state, retained dispositions and local bindings. A new attempt
    // would need a new binding, so the last count is the attempt count.
    fn observe(path: &Path) -> (String, i64, i64) {
        let db = open(path);
        let observation: (String, i64, i64) = db
            .query_row(
                "SELECT (SELECT state FROM current WHERE request_id=bindings.request_id),
                    (SELECT count(*) FROM dispositions), (SELECT count(*) FROM bindings)
                 FROM bindings WHERE inv=?1 AND attempt=?2 AND process=?3",
                ATTEMPT,
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(probe_dispatch(&db), 0);
        let calls: i64 = db
            .query_row("SELECT count(*) FROM calls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(calls, 0);
        observation
    }

    // The test worker exercises its normal poll path on each reopened journal
    // and reports how many calls this poll dispatched. A separate recovery
    // decision would have to enqueue new work.
    fn probe_dispatch(db: &Connection) -> usize {
        ["model", "tool"]
            .into_iter()
            .map(|kind| {
                db.execute(
                    "INSERT INTO calls SELECT ?1 FROM bindings JOIN current USING (request_id)
                     WHERE state='ready'",
                    [kind],
                )
                .unwrap()
            })
            .sum()
    }

    fn history(path: &Path) -> Vec<(String, Option<String>)> {
        let db = open(path);
        let mut rows = db
            .prepare("SELECT event, state FROM history ORDER BY seq")
            .unwrap();
        rows.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn event(name: &str, state: Option<&str>) -> (String, Option<String>) {
        (name.to_owned(), state.map(str::to_owned))
    }

    fn retained_disposition(path: &Path) -> Option<ReconciliationDisposition> {
        open(path)
            .query_row("SELECT value FROM dispositions", [], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()
            .unwrap()
            .map(|value| decode(&value))
    }

    fn divergent(field: &str) -> Host {
        let mut host = Host::bound();
        let Some(ReconciliationDisposition::Fenced {
            fence_token,
            disposition_id,
            ..
        }) = host.reply.as_mut()
        else {
            unreachable!()
        };
        match field {
            "token" => *fence_token = "other-token".into(),
            _ => *disposition_id = "other-disposition".into(),
        }
        host
    }

    #[test]
    fn ready_control_reaches_both_dispatch_spies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("consumer.db");
        initialize(&path, "ready", true);
        assert_eq!(probe_dispatch(&open(&path)), 2);
    }

    // Run only by the crash fixture below, which passes `--ignored --exact`
    // and both variables. In any other run it does nothing, and it never
    // creates a journal.
    #[test]
    #[ignore = "subprocess helper for the crash fixture"]
    fn child_process() {
        let (Some(path), Ok(exit)) = (env::var_os(JOURNAL_ENV), env::var(EXIT_ENV)) else {
            return;
        };
        let exit = match exit.as_str() {
            "before" => ExitPoint::BeforeCommit,
            "after" => ExitPoint::AfterCommit,
            _ => return,
        };
        let path = Path::new(&path);
        if !path.is_file() {
            return;
        }
        reconcile(path, ATTEMPT, &Host::bound(), exit);
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
                    .args(["--ignored", "--exact", "journal::child_process"])
                    .env(JOURNAL_ENV, &path)
                    .env(EXIT_ENV, fault)
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
                assert_eq!(history(&path).len(), 1 + usize::from(committed));
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
                assert_eq!(
                    history(&path),
                    [
                        event("bound", Some(state)),
                        event("transition", Some("reconciled-fenced"))
                    ]
                );
                assert_eq!(retained_disposition(&path), Some(fenced(&request())));
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
                    assert_eq!(
                        reconcile(&path, ATTEMPT, &divergent(field), ExitPoint::None),
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
                assert_eq!(retained_disposition(&path), Some(fenced(&request())));
            }
        }
    }

    // Each reply comes from the bound peer for the bound request and fails
    // `validate_for` in exactly one way.
    fn invalid_bound_replies() -> Vec<Host> {
        ["stale", "digest", "correlation", "token"]
            .into_iter()
            .map(|field| {
                let mut host = Host::bound();
                let Some(ReconciliationDisposition::Fenced {
                    fence_token,
                    correlation,
                    ambiguity_digest,
                    recorded_at,
                    ..
                }) = host.reply.as_mut()
                else {
                    unreachable!()
                };
                match field {
                    "stale" => {
                        *recorded_at =
                            OffsetDateTime::parse("2000-01-01T00:00:00Z", &Rfc3339).unwrap();
                    }
                    "digest" => *ambiguity_digest = digest_of('e'),
                    "correlation" => correlation.request_digest = digest_of('e'),
                    _ => fence_token.clear(),
                }
                assert!(
                    host.reply
                        .as_ref()
                        .unwrap()
                        .validate_for(&host.request)
                        .is_err(),
                    "{field}"
                );
                host
            })
            .collect()
    }

    #[test]
    fn invalid_reply_from_the_bound_host_never_records_or_replaces_a_disposition() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            for reply in invalid_bound_replies() {
                assert_eq!(
                    reconcile(&path, ATTEMPT, &reply, ExitPoint::None),
                    Observation::Blocked
                );
            }
            assert_eq!(observe(&path), (state.to_owned(), 0, 1));
            assert_eq!(retained_disposition(&path), None);
            assert_eq!(
                reconcile(&path, ATTEMPT, &Host::bound(), ExitPoint::None),
                Observation::Recorded
            );
            for reply in invalid_bound_replies() {
                assert_eq!(
                    reconcile(&path, ATTEMPT, &reply, ExitPoint::None),
                    Observation::Blocked
                );
            }
            assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
            assert_eq!(retained_disposition(&path), Some(fenced(&request())));
            assert_eq!(history(&path).len(), 2);
        }
    }

    #[test]
    fn a_request_refuses_a_second_local_binding() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let db = open(&path);
            let others = [
                [ATTEMPT[0], ATTEMPT[1], "process-B"],
                ["invocation-B", "attempt-B", "process-B"],
            ];
            for key in others {
                assert!(!bind(&db, key, Some(HOST), &request(), state));
            }
            drop(db);
            assert_eq!(observe(&path), (state.to_owned(), 0, 1));
            assert_eq!(history(&path), [event("bound", Some(state))]);
            assert_eq!(
                reconcile(&path, ATTEMPT, &Host::bound(), ExitPoint::None),
                Observation::Recorded
            );
            for key in others {
                for host in [Host::bound(), divergent("token"), divergent("id")] {
                    assert_eq!(
                        reconcile(&path, key, &host, ExitPoint::None),
                        Observation::Blocked
                    );
                }
            }
            assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
            assert_eq!(retained_disposition(&path), Some(fenced(&request())));
        }
    }

    // A journal that already holds two local bindings for one request, written
    // past `bind`, still retains one disposition: the second process cannot
    // record a different one.
    #[test]
    fn retained_disposition_is_keyed_by_the_request_not_by_the_local_attempt() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let second = [ATTEMPT[0], ATTEMPT[1], "process-B"];
            open(&path)
                .execute(
                    "INSERT INTO bindings VALUES (?1,?2,?3,?4,?5)",
                    params![
                        second[0],
                        second[1],
                        second[2],
                        HOST,
                        request_id(&request())
                    ],
                )
                .unwrap();
            assert_eq!(
                reconcile(&path, ATTEMPT, &Host::bound(), ExitPoint::None),
                Observation::Recorded
            );
            for field in ["token", "id"] {
                assert_eq!(
                    reconcile(&path, second, &divergent(field), ExitPoint::None),
                    Observation::Conflict
                );
            }
            assert_eq!(
                reconcile(&path, second, &Host::bound(), ExitPoint::None),
                Observation::Duplicate
            );
            let keys: Vec<String> = open(&path)
                .prepare("SELECT request_id FROM dispositions")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert_eq!(keys, [request_id(&request())]);
            assert_eq!(retained_disposition(&path), Some(fenced(&request())));
            assert_eq!(
                history(&path),
                [
                    event("bound", Some(state)),
                    event("transition", Some("reconciled-fenced")),
                    event("conflict", None),
                    event("conflict", None)
                ]
            );
        }
    }

    #[test]
    fn valid_fence_never_transitions_or_records_from_an_ineligible_state() {
        for state in INELIGIBLE {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let dispatched = if state == "ready" { 2 } else { 0 };
            assert_eq!(probe_dispatch(&open(&path)), dispatched);
            for _ in 0..2 {
                assert_eq!(
                    reconcile(&path, ATTEMPT, &Host::bound(), ExitPoint::None),
                    Observation::Blocked
                );
                assert_eq!(history(&path), [event("bound", Some(state))]);
                assert_eq!(retained_disposition(&path), None);
                let db = open(&path);
                let (current, bindings): (String, i64) = db
                    .query_row(
                        "SELECT (SELECT state FROM current), (SELECT count(*) FROM bindings)",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!((current.as_str(), bindings), (state, 1));
                assert_eq!(probe_dispatch(&db), dispatched);
            }
        }
    }

    // `validate_for` cannot see a changed `reason_code`: a disposition carries
    // none. The modelled host must refuse to replay for any request other than
    // its own exact one.
    #[test]
    fn host_replays_only_for_its_own_exact_request() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let mut host = Host::bound();
            host.request.reason_code = "other_reason".to_owned();
            host.request.validate().unwrap();
            host.reply
                .as_ref()
                .unwrap()
                .validate_for(&request())
                .unwrap();
            assert_eq!(
                reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                Observation::Blocked
            );
            assert_eq!(host.replays.get(), 1);
            assert_eq!(observe(&path), (state.to_owned(), 0, 1));
            assert_eq!(history(&path), [event("bound", Some(state))]);
        }
    }

    #[test]
    fn returned_reply_stays_blocked_and_records_nothing() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let request = request();
            let returned = ReconciliationDisposition::Returned {
                disposition_id: "disp-1".to_owned(),
                session_id: "session-1".to_owned(),
                correlation: request.correlation.clone(),
                ambiguity_digest: request.ambiguity_digest.clone(),
                recorded_at: request.correlation.created_at + time::Duration::seconds(1),
            };
            returned.validate_for(&request).unwrap();
            let host = Host {
                reply: Some(returned),
                ..Host::bound()
            };
            assert_eq!(
                reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                Observation::Blocked
            );
            assert_eq!(host.replays.get(), 1);
            assert_eq!(observe(&path), (state.to_owned(), 0, 1));
            assert_eq!(history(&path), [event("bound", Some(state))]);
        }
    }

    #[test]
    fn stored_request_that_is_corrupt_or_invalid_stays_blocked() {
        let request = request();
        let correlation = serde_json::to_vec(&request.correlation).unwrap();
        let ambiguity = request.ambiguity_digest.to_string();
        let id = request_id(&request);
        for state in STARTS {
            for case in ["correlation", "digest", "reason", "identity"] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("consumer.db");
                let db = create(&path);
                let mut host = Host::bound();
                let stored: (&str, &[u8], &str, &str) = match case {
                    "correlation" => (id, b"{}", &ambiguity, &request.reason_code),
                    "digest" => (id, &correlation, "sha256:short", &request.reason_code),
                    "reason" => {
                        // The host holds the same invalid request, so only the
                        // consumer's own validation can stop the replay.
                        host.request.reason_code = "Not A Reason".to_owned();
                        assert!(host.request.validate().is_err());
                        (id, &correlation, &ambiguity, "Not A Reason")
                    }
                    _ => (
                        "another-request",
                        &correlation,
                        &ambiguity,
                        &request.reason_code,
                    ),
                };
                assert!(bind_raw(&db, ATTEMPT, Some(HOST), stored, state), "{case}");
                drop(db);
                assert_eq!(
                    reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                    Observation::Blocked,
                    "{case}"
                );
                assert_eq!(host.replays.get(), 0, "{case}");
                assert_eq!(observe(&path), (state.to_owned(), 0, 1), "{case}");
                assert_eq!(history(&path), [event("bound", Some(state))], "{case}");
            }
        }
    }

    #[test]
    fn journal_history_is_append_only_and_keeps_each_conflict() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            assert_eq!(
                reconcile(&path, ATTEMPT, &Host::bound(), ExitPoint::None),
                Observation::Recorded
            );
            assert_eq!(
                reconcile(&path, ATTEMPT, &divergent("token"), ExitPoint::None),
                Observation::Conflict
            );
            let expected = [
                event("bound", Some(state)),
                event("transition", Some("reconciled-fenced")),
                event("conflict", None),
            ];
            assert_eq!(history(&path), expected);
            let offered: Vec<u8> = open(&path)
                .query_row(
                    "SELECT value FROM history WHERE event='conflict'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(Some(decode(&offered)), divergent("token").reply);
            let db = open(&path);
            for table in ["requests", "bindings", "dispositions", "history"] {
                for change in [
                    format!("UPDATE {table} SET request_id='rewritten'"),
                    format!("DELETE FROM {table}"),
                ] {
                    assert!(db.execute(&change, []).is_err(), "{change}");
                }
            }
            drop(db);
            assert_eq!(history(&path), expected);
            assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
            assert_eq!(retained_disposition(&path), Some(fenced(&request())));
        }
    }

    #[test]
    fn journal_is_locked_before_the_host_is_asked() {
        for state in STARTS {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("consumer.db");
            initialize(&path, state, true);
            let writer = open(&path);
            writer.execute_batch("BEGIN IMMEDIATE").unwrap();
            let host = Host::bound();
            assert_eq!(
                reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                Observation::Blocked
            );
            assert_eq!(host.replays.get(), 0);
            writer.execute_batch("ROLLBACK").unwrap();
            assert_eq!(observe(&path), (state.to_owned(), 0, 1));
            assert_eq!(
                reconcile(&path, ATTEMPT, &host, ExitPoint::None),
                Observation::Recorded
            );
            assert_eq!(host.replays.get(), 1);
            assert_eq!(observe(&path), ("reconciled-fenced".to_owned(), 1, 1));
        }
    }
}
