//! Deterministic fakes and reusable Psyche conformance fixtures.

pub mod coven;
pub mod suites;
pub mod surface;

pub use coven::{
    BeforeTerminate, CovenConformanceCase, CovenConformanceFixture, CovenConformanceObservations,
    CovenFaultPoint, CovenScriptReturn, CovenScriptStep, DurableDispositionKind,
    DurableDispositionObservation, FakeBuildError, FakeCoven, FakeCovenBuilder, FakeError,
    FakeOperation, FixtureAvailability, FixtureControlError, RedispatchEligibility,
    StoreTerminationPersistence,
};
pub use suites::{
    ConformanceOutcome, ScriptedG2Fixture, UnsupportedCovenFixture,
    assert_c_s1_contract_negotiation, assert_c_s2_session_lifecycle,
    assert_c_s3_snapshot_attempt_binding, assert_c_s4_stable_adoption,
    assert_c_s5_non_adoption_proof, assert_c_s6_ambiguity_fence, assert_c_s7_ordered_cursor,
    assert_c_s8_terminal_authority, assert_c_s9_cancellation_acknowledgement,
    assert_c_s10_result_artifact_binding, assert_c_s11_restart_persistence,
    assert_c_s12_structured_denial, assert_surface_unknown_delivery, scripted_fixture,
    scripted_fixture_with_session_id, scripted_surface, unsupported_fixture,
};
pub use surface::{
    FakeSurface, FakeSurfaceBuilder, SurfaceFakeBuildError, SurfaceFakeCall, SurfaceScriptReturn,
    SurfaceScriptStep,
};
