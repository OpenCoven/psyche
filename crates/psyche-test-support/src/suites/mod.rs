//! Reusable adapter-neutral conformance suites.

mod coven;
mod surface;

pub use coven::{
    ScriptedG2Fixture, UnsupportedCovenFixture, assert_c_s1_contract_negotiation,
    assert_c_s2_session_lifecycle, assert_c_s3_snapshot_attempt_binding,
    assert_c_s4_stable_adoption, assert_c_s5_non_adoption_proof, assert_c_s6_ambiguity_fence,
    assert_c_s7_ordered_cursor, assert_c_s8_terminal_authority,
    assert_c_s9_cancellation_acknowledgement, assert_c_s10_result_artifact_binding,
    assert_c_s11_restart_persistence, assert_c_s12_structured_denial, scripted_fixture,
    unsupported_fixture,
};
pub use surface::{assert_surface_unknown_delivery, scripted_surface};

/// Result of executing one reusable behavior suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConformanceOutcome {
    /// Every supported behavior was verified.
    Verified,
    /// The fixture made the required public call and returned its declared denial.
    ExpectedUnsupported {
        /// Exact stable structured denial code.
        code: String,
    },
}
