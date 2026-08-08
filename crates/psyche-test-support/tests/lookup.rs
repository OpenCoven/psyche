//! Durable adoption lookup regression coverage.

use psyche_coven::{AdoptionDisposition, AdoptionRequest, CovenPort, ExecutionRequestInput};
use psyche_test_support::FakeCoven;

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
