//! Deterministic fakes and reusable Psyche conformance fixtures.

pub mod coven;
pub mod surface;

pub use coven::{
    BeforeTerminate, CovenScriptReturn, CovenScriptStep, FakeBuildError, FakeCall, FakeCoven,
    FakeCovenBuilder, FakeError, FakeOperation, StoreTerminationPersistence,
};
pub use surface::{
    FakeSurface, FakeSurfaceBuilder, SurfaceFakeBuildError, SurfaceFakeCall, SurfaceScriptReturn,
    SurfaceScriptStep,
};
