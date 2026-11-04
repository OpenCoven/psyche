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
fn exact_fenced_disposition_validates_and_replays_equal() {
    let request = request();
    let disposition = fenced(&request);
    disposition.validate_for(&request).unwrap();
    assert_eq!(disposition.clone(), disposition);
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
