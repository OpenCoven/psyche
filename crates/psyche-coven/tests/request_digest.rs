#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use psyche_core::digest::canonical_bytes;
use psyche_coven::{AdoptionRequest, ExecutionRequestInput};
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const LAUNCH_GOLDEN: &[u8] = include_bytes!("fixtures/execution-request-launch.json");
const INPUT_GOLDEN: &[u8] = include_bytes!("fixtures/execution-request-input.json");

#[test]
fn execution_request_launch_matches_golden_bytes_and_digest() {
    let decoded: ExecutionRequestInput = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    decoded.validate().unwrap();
    assert_eq!(canonical_bytes(&decoded).unwrap(), LAUNCH_GOLDEN);

    let request = AdoptionRequest::new(decoded).unwrap();
    assert_eq!(
        request.request_digest().as_str(),
        "sha256:75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04"
    );
    assert_eq!(
        request.recompute_digest().unwrap(),
        request.request_digest().clone()
    );

    let value: serde_json::Value = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    assert!(value["created_at"].is_string());
    assert!(value["valid_until"].is_string());
    assert_eq!(value["created_at"], "2026-08-05T14:00:00Z");
    assert_eq!(value["valid_until"], "2026-08-05T14:05:00Z");

    #[derive(Serialize)]
    struct Unannotated {
        created_at: OffsetDateTime,
    }
    let unannotated = Unannotated {
        created_at: OffsetDateTime::parse("2026-08-05T14:00:00Z", &Rfc3339).unwrap(),
    };
    assert_ne!(
        canonical_bytes(&unannotated).unwrap(),
        br#"{"created_at":"2026-08-05T14:00:00Z"}"#
    );
}

#[test]
fn execution_request_input_matches_golden_bytes_and_digest() {
    let decoded: ExecutionRequestInput = serde_json::from_slice(INPUT_GOLDEN).unwrap();
    decoded.validate().unwrap();
    assert_eq!(canonical_bytes(&decoded).unwrap(), INPUT_GOLDEN);

    let request = AdoptionRequest::new(decoded).unwrap();
    assert_eq!(
        request.request_digest().as_str(),
        "sha256:c8c3d0cad99f65d0fdac7b2bb577cf1278412a7ea6255d443e45394109311c61"
    );
    assert_eq!(
        request.recompute_digest().unwrap(),
        request.request_digest().clone()
    );

    let value: serde_json::Value = serde_json::from_slice(INPUT_GOLDEN).unwrap();
    assert!(value["created_at"].is_string());
    assert!(value["valid_until"].is_string());
    assert_eq!(value["created_at"], "2026-08-05T14:01:00Z");
    assert_eq!(value["valid_until"], "2026-08-05T14:06:00Z");
}

#[test]
fn execution_request_artifact_order_is_digest_bound() {
    let mut ordered: serde_json::Value = serde_json::from_slice(LAUNCH_GOLDEN).unwrap();
    ordered["required_artifact_bindings"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "artifact_id": "artifact-2",
            "digest": format!("sha256:{}", "a".repeat(64)),
            "media_type": "application/json",
            "size": 2
        }));
    let ordered_request =
        AdoptionRequest::new(serde_json::from_value(ordered.clone()).unwrap()).unwrap();
    let mut reversed = ordered;
    reversed["required_artifact_bindings"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let reversed_request = AdoptionRequest::new(serde_json::from_value(reversed).unwrap()).unwrap();

    assert_ne!(
        ordered_request.request_digest(),
        reversed_request.request_digest()
    );
}
