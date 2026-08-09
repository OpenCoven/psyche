#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use psyche_test_support::suites::{
    ConformanceOutcome, assert_c_s1_contract_negotiation, assert_c_s2_session_lifecycle,
    assert_c_s3_snapshot_attempt_binding, assert_c_s4_stable_adoption,
    assert_c_s5_non_adoption_proof, assert_c_s6_ambiguity_fence, assert_c_s7_ordered_cursor,
    assert_c_s8_terminal_authority, assert_c_s9_cancellation_acknowledgement,
    assert_c_s10_result_artifact_binding, assert_c_s11_restart_persistence,
    assert_c_s12_structured_denial, assert_surface_unknown_delivery, scripted_fixture,
    scripted_fixture_with_session_id, scripted_surface, unsupported_fixture,
};

#[tokio::test]
async fn c_s1_contract_negotiation() {
    assert_eq!(
        assert_c_s1_contract_negotiation(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s2_session_lifecycle() {
    assert_eq!(
        assert_c_s2_session_lifecycle(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s3_snapshot_attempt_binding() {
    assert_eq!(
        assert_c_s3_snapshot_attempt_binding(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s4_stable_adoption() {
    assert_eq!(
        assert_c_s4_stable_adoption(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s5_non_adoption_proof() {
    assert_eq!(
        assert_c_s5_non_adoption_proof(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s6_ambiguity_fence() {
    assert_eq!(
        assert_c_s6_ambiguity_fence(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s7_ordered_cursor() {
    assert_eq!(
        assert_c_s7_ordered_cursor(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s8_terminal_authority() {
    assert_eq!(
        assert_c_s8_terminal_authority(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s9_cancellation_acknowledgement() {
    assert_eq!(
        assert_c_s9_cancellation_acknowledgement(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s10_result_artifact_binding() {
    assert_eq!(
        assert_c_s10_result_artifact_binding(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s11_restart_persistence() {
    assert_eq!(
        assert_c_s11_restart_persistence(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn c_s12_structured_denial() {
    assert_eq!(
        assert_c_s12_structured_denial(&mut scripted_fixture()).await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn surface_unknown_delivery() {
    assert_surface_unknown_delivery(&scripted_surface()).await;
}

#[tokio::test]
async fn reusable_conformance_accepts_opaque_session_ids() {
    const OPAQUE_SESSION_ID: &str = "coven-session:opaque-7f4d2a";

    assert_eq!(
        assert_c_s2_session_lifecycle(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID))
            .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s3_snapshot_attempt_binding(&mut scripted_fixture_with_session_id(
            OPAQUE_SESSION_ID,
        ))
        .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s4_stable_adoption(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID)).await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s5_non_adoption_proof(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID))
            .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s6_ambiguity_fence(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID)).await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s7_ordered_cursor(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID)).await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s8_terminal_authority(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID))
            .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s9_cancellation_acknowledgement(&mut scripted_fixture_with_session_id(
            OPAQUE_SESSION_ID,
        ))
        .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s10_result_artifact_binding(&mut scripted_fixture_with_session_id(
            OPAQUE_SESSION_ID,
        ))
        .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s11_restart_persistence(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID))
            .await,
        ConformanceOutcome::Verified
    );
    assert_eq!(
        assert_c_s12_structured_denial(&mut scripted_fixture_with_session_id(OPAQUE_SESSION_ID))
            .await,
        ConformanceOutcome::Verified
    );
}

#[tokio::test]
async fn expected_unsupported_paths_execute_public_calls_without_mutation() {
    assert_all_expected_unsupported("CapabilityMissing").await;
    assert_all_expected_unsupported("ContractUnsupported").await;
}

async fn assert_all_expected_unsupported(code: &str) {
    let expected = ConformanceOutcome::ExpectedUnsupported {
        code: code.to_owned(),
    };

    assert_eq!(
        assert_c_s1_contract_negotiation(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s2_session_lifecycle(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s3_snapshot_attempt_binding(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s4_stable_adoption(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s5_non_adoption_proof(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s6_ambiguity_fence(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s7_ordered_cursor(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s8_terminal_authority(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s9_cancellation_acknowledgement(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s10_result_artifact_binding(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s11_restart_persistence(&mut unsupported_fixture(code)).await,
        expected
    );
    assert_eq!(
        assert_c_s12_structured_denial(&mut unsupported_fixture(code)).await,
        expected
    );
}
