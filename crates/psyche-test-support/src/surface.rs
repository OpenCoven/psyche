//! Deterministic scripted surface fake.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use psyche_core::contracts::surface::{SurfaceEffect, SurfaceEvent};
use psyche_core::digest::canonical_bytes;
use psyche_surfaces::{DeliveryDisposition, PortError, SurfaceAcceptance, SurfacePort};

/// Redacted surface call observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceFakeCall {
    /// Event acceptance.
    Accept,
    /// Effect application.
    Apply,
}

/// Typed successful surface response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceScriptReturn {
    /// Acceptance response.
    Accept(SurfaceAcceptance),
    /// Delivery response.
    Apply(DeliveryDisposition),
}

impl SurfaceScriptReturn {
    fn call(&self) -> SurfaceFakeCall {
        match self {
            Self::Accept(_) => SurfaceFakeCall::Accept,
            Self::Apply(_) => SurfaceFakeCall::Apply,
        }
    }
}

/// One deterministic surface response or fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceScriptStep {
    /// Returns one typed response.
    Return(SurfaceScriptReturn),
    /// Returns one stable error.
    Error {
        /// Expected call.
        call: SurfaceFakeCall,
        /// Stable error.
        error: PortError,
    },
    /// Disconnects before durable mutation.
    DisconnectBeforeCommit(SurfaceFakeCall),
    /// Commits a response then loses the reply.
    DisconnectAfterCommit(SurfaceScriptReturn),
    /// Leaves state unchanged and reports a deterministic stall.
    Stall(SurfaceFakeCall),
}

impl SurfaceScriptStep {
    fn call(&self) -> SurfaceFakeCall {
        match self {
            Self::Return(response) | Self::DisconnectAfterCommit(response) => response.call(),
            Self::Error { call, .. } | Self::DisconnectBeforeCommit(call) | Self::Stall(call) => {
                *call
            }
        }
    }
}

/// Invalid fake surface construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceFakeBuildError {
    /// No behavior was scripted.
    #[error("surface fake has no scripted behavior")]
    EmptyScript,
}

#[derive(Debug, Default)]
struct SurfaceState {
    script: VecDeque<SurfaceScriptStep>,
    calls: Vec<SurfaceFakeCall>,
    acceptances: BTreeMap<String, (Vec<u8>, SurfaceAcceptance)>,
    deliveries: BTreeMap<String, (Vec<u8>, DeliveryDisposition)>,
}

/// Honest deterministic surface fake.
#[derive(Debug, Clone)]
pub struct FakeSurface {
    state: Arc<Mutex<SurfaceState>>,
}

impl FakeSurface {
    /// Begins fake construction.
    pub fn builder() -> FakeSurfaceBuilder {
        FakeSurfaceBuilder::default()
    }

    /// Returns redacted calls.
    pub fn calls(&self) -> Vec<SurfaceFakeCall> {
        self.state
            .lock()
            .map(|state| state.calls.clone())
            .unwrap_or_default()
    }

    /// Simulates process restart while preserving fake-owned durable outcomes.
    pub fn restart(&self) -> Self {
        self.clone()
    }

    fn record_replay(&self, call: SurfaceFakeCall) -> Result<(), PortError> {
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        state.calls.push(call);
        Ok(())
    }

    fn take(&self, call: SurfaceFakeCall) -> Result<SurfaceScriptStep, PortError> {
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        state.calls.push(call);
        let Some(step) = state.script.front() else {
            return Err(PortError::UnexpectedCall);
        };
        if step.call() != call {
            return Err(PortError::UnexpectedCall);
        }
        state.script.pop_front().ok_or(PortError::UnexpectedCall)
    }

    fn replay_acceptance(
        &self,
        event: &SurfaceEvent,
    ) -> Result<Option<SurfaceAcceptance>, PortError> {
        let bytes = canonical_bytes(event).map_err(|_| PortError::InvalidEvent)?;
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        let Some((stored_bytes, acceptance)) =
            state.acceptances.get(event.surface_event_id.as_str())
        else {
            return Ok(None);
        };
        if stored_bytes == &bytes {
            Ok(Some(acceptance.clone()))
        } else {
            Err(PortError::IntentConflict)
        }
    }

    fn commit_acceptance(
        &self,
        event: &SurfaceEvent,
        acceptance: SurfaceAcceptance,
    ) -> Result<SurfaceAcceptance, PortError> {
        acceptance.validate()?;
        if acceptance.surface_event_id != event.surface_event_id {
            return Err(PortError::InvalidResponse);
        }
        let bytes = canonical_bytes(event).map_err(|_| PortError::InvalidEvent)?;
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if let Some((stored_bytes, stored)) = state.acceptances.get(event.surface_event_id.as_str())
        {
            return if stored_bytes == &bytes {
                Ok(stored.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        state.acceptances.insert(
            event.surface_event_id.as_str().to_owned(),
            (bytes, acceptance.clone()),
        );
        Ok(acceptance)
    }

    fn replay_delivery(
        &self,
        effect: &SurfaceEffect,
    ) -> Result<Option<DeliveryDisposition>, PortError> {
        let bytes = canonical_bytes(effect).map_err(|_| PortError::InvalidEffect)?;
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        let Some((stored_bytes, disposition)) =
            state.deliveries.get(effect.surface_effect_id.as_str())
        else {
            return Ok(None);
        };
        if stored_bytes == &bytes {
            Ok(Some(disposition.clone()))
        } else {
            Err(PortError::IntentConflict)
        }
    }

    fn commit_delivery(
        &self,
        effect: &SurfaceEffect,
        disposition: DeliveryDisposition,
    ) -> Result<DeliveryDisposition, PortError> {
        disposition.validate()?;
        let bytes = canonical_bytes(effect).map_err(|_| PortError::InvalidEffect)?;
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if let Some((stored_bytes, stored)) =
            state.deliveries.get(effect.surface_effect_id.as_str())
        {
            return if stored_bytes == &bytes {
                Ok(stored.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        state.deliveries.insert(
            effect.surface_effect_id.as_str().to_owned(),
            (bytes, disposition.clone()),
        );
        Ok(disposition)
    }
}

/// Builder for [`FakeSurface`].
#[derive(Debug, Default)]
pub struct FakeSurfaceBuilder {
    script: VecDeque<SurfaceScriptStep>,
}

impl FakeSurfaceBuilder {
    /// Replaces the ordered script.
    #[must_use]
    pub fn script(mut self, script: VecDeque<SurfaceScriptStep>) -> Self {
        self.script = script;
        self
    }

    /// Appends one explicit script step.
    #[must_use]
    pub fn step(mut self, step: SurfaceScriptStep) -> Self {
        self.script.push_back(step);
        self
    }

    /// Scripts one event acceptance.
    #[must_use]
    pub fn acceptance(mut self, acceptance: SurfaceAcceptance) -> Self {
        self.script
            .push_back(SurfaceScriptStep::Return(SurfaceScriptReturn::Accept(
                acceptance,
            )));
        self
    }

    /// Scripts one delivery disposition.
    #[must_use]
    pub fn delivery(mut self, disposition: DeliveryDisposition) -> Self {
        self.script
            .push_back(SurfaceScriptStep::Return(SurfaceScriptReturn::Apply(
                disposition,
            )));
        self
    }

    /// Builds a nonempty honest fake.
    pub fn build(self) -> Result<FakeSurface, SurfaceFakeBuildError> {
        if self.script.is_empty() {
            return Err(SurfaceFakeBuildError::EmptyScript);
        }
        Ok(FakeSurface {
            state: Arc::new(Mutex::new(SurfaceState {
                script: self.script,
                calls: Vec::new(),
                acceptances: BTreeMap::new(),
                deliveries: BTreeMap::new(),
            })),
        })
    }
}

#[async_trait::async_trait]
impl SurfacePort for FakeSurface {
    async fn accept(&self, event: SurfaceEvent) -> Result<SurfaceAcceptance, PortError> {
        event.validate().map_err(|_| PortError::InvalidEvent)?;
        if let Some(acceptance) = self.replay_acceptance(&event)? {
            self.record_replay(SurfaceFakeCall::Accept)?;
            return Ok(acceptance);
        }
        match self.take(SurfaceFakeCall::Accept)? {
            SurfaceScriptStep::Return(SurfaceScriptReturn::Accept(acceptance)) => {
                self.commit_acceptance(&event, acceptance)
            }
            SurfaceScriptStep::DisconnectAfterCommit(SurfaceScriptReturn::Accept(acceptance)) => {
                self.commit_acceptance(&event, acceptance)?;
                Err(PortError::Unavailable)
            }
            SurfaceScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            SurfaceScriptStep::Error { error, .. } => Err(error),
            SurfaceScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn apply(&self, effect: SurfaceEffect) -> Result<DeliveryDisposition, PortError> {
        effect.validate().map_err(|_| PortError::InvalidEffect)?;
        if let Some(disposition) = self.replay_delivery(&effect)? {
            self.record_replay(SurfaceFakeCall::Apply)?;
            return Ok(disposition);
        }
        match self.take(SurfaceFakeCall::Apply)? {
            SurfaceScriptStep::Return(SurfaceScriptReturn::Apply(disposition)) => {
                self.commit_delivery(&effect, disposition)
            }
            SurfaceScriptStep::DisconnectAfterCommit(SurfaceScriptReturn::Apply(disposition)) => {
                self.commit_delivery(&effect, disposition)?;
                Err(PortError::Unavailable)
            }
            SurfaceScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            SurfaceScriptStep::Error { error, .. } => Err(error),
            SurfaceScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }
}
