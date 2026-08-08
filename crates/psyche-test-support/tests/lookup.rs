//! Durable adoption lookup regression coverage.

use psyche_core::id::RequestId;
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, CovenPort, ExecutionRequestInput, PortError,
};
use psyche_test_support::{CovenScriptReturn, CovenScriptStep, FakeCoven, FakeOperation};

const LAUNCH_GOLDEN: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json");

#[tokio::test]
async fn lookup_replays_durable_adoption_after_restart_without_a_script_step() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let request_id = request.correlation().request_id;
    let disposition = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let fake = FakeCoven::builder()
        .adoption(disposition.clone())
        .build()
        .unwrap();

    assert_eq!(fake.adopt(request).await.unwrap(), disposition);
    assert_eq!(
        fake.restart().lookup(&request_id).await.unwrap(),
        disposition
    );
}

#[tokio::test]
async fn successful_scripted_lookup_replays_after_restart_without_consuming_another_step() {
    let first_id = RequestId::parse("req_01J00000000000000000000000").unwrap();
    let second_id = RequestId::parse("req_01J00000000000000000000001").unwrap();
    let first = AdoptionDisposition::Adopted {
        session_id: "session-1".to_owned(),
    };
    let second = AdoptionDisposition::ProvenNotAdopted;
    let fake = FakeCoven::builder()
        .lookup(first.clone())
        .lookup(second.clone())
        .build()
        .unwrap();

    assert_eq!(fake.lookup(&first_id).await.unwrap(), first);
    let restarted = fake.restart();
    assert_eq!(restarted.lookup(&first_id).await.unwrap(), first);
    assert_eq!(restarted.lookup(&second_id).await.unwrap(), second);
}

#[tokio::test]
async fn lookup_after_commit_disconnect_replays_but_before_commit_does_not() {
    let request_id = RequestId::parse("req_01J00000000000000000000000").unwrap();
    let other_id = RequestId::parse("req_01J00000000000000000000001").unwrap();
    let disposition = AdoptionDisposition::ProvenNotAdopted;
    let after_commit = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectAfterCommit(
            CovenScriptReturn::Lookup(disposition.clone()),
        ))
        .build()
        .unwrap();

    assert_eq!(
        after_commit.lookup(&request_id).await,
        Err(PortError::Unavailable)
    );
    assert_eq!(
        after_commit.restart().lookup(&request_id).await.unwrap(),
        disposition
    );

    let before_commit = FakeCoven::builder()
        .step(CovenScriptStep::DisconnectBeforeCommit(
            FakeOperation::Lookup,
        ))
        .lookup(disposition.clone())
        .build()
        .unwrap();
    assert_eq!(
        before_commit.lookup(&request_id).await,
        Err(PortError::Unavailable)
    );
    assert_eq!(
        before_commit.restart().lookup(&request_id).await.unwrap(),
        disposition
    );
    assert_eq!(
        before_commit.lookup(&other_id).await,
        Err(PortError::UnexpectedCall)
    );
}

#[tokio::test]
async fn durable_scripted_lookup_conflicts_with_a_later_different_adoption() {
    let input: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    let request = AdoptionRequest::new(input).unwrap();
    let request_id = request.correlation().request_id;
    let fake = FakeCoven::builder()
        .lookup(AdoptionDisposition::ProvenNotAdopted)
        .adoption(AdoptionDisposition::Adopted {
            session_id: "session-1".to_owned(),
        })
        .build()
        .unwrap();

    assert_eq!(
        fake.lookup(&request_id).await.unwrap(),
        AdoptionDisposition::ProvenNotAdopted
    );
    assert_eq!(fake.adopt(request).await, Err(PortError::IntentConflict));
}
