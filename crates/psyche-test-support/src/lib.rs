//! Deterministic fakes and reusable Psyche conformance fixtures.

pub mod coven;
pub mod surface;

pub use coven::{
    BeforeTerminate, CovenConformanceCase, CovenConformanceFixture, CovenConformanceObservations,
    CovenFaultPoint, CovenScriptReturn, CovenScriptStep, DurableDispositionKind,
    DurableDispositionObservation, FakeBuildError, FakeCoven, FakeCovenBuilder, FakeError,
    FakeOperation, FixtureAvailability, StoreTerminationPersistence,
};
pub use surface::{
    FakeSurface, FakeSurfaceBuilder, SurfaceFakeBuildError, SurfaceFakeCall, SurfaceScriptReturn,
    SurfaceScriptStep,
};
