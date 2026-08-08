//! Deterministic Coven scripts and Store-backed termination persistence.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use psyche_core::contracts::CanonicalDocument;
use psyche_core::contracts::execution::{CancellationState, ExecutionBinding};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::RequestId;
use psyche_coven::{
    AdoptionDisposition, AdoptionRequest, Capability, CapabilityProfile, CovenPort, EventCursor,
    EventPage, ExecutionCorrelation, NegotiateRequest, PortError, ReconciliationDisposition,
    ReconciliationRequest, ResultBundle, SessionSnapshot, TerminationDisposition,
    TerminationPersistence, TerminationPersistenceFailure, TerminationRequest,
};
use psyche_store::{Store, StoreError};

/// Redacted Coven operation identity used by scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeOperation {
    /// Contract negotiation.
    Negotiate,
    /// Stable request adoption.
    Adopt,
    /// Durable adoption lookup.
    Lookup,
    /// Ambiguity reconciliation.
    Reconcile,
    /// Session inspection.
    Inspect,
    /// Ordered event read.
    Events,
    /// Result metadata read.
    Result,
    /// Authoritative termination.
    Terminate,
}

/// Redacted call observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeCall {
    /// Contract negotiation.
    Negotiate,
    /// Stable request adoption.
    Adopt,
    /// Durable adoption lookup.
    Lookup,
    /// Ambiguity reconciliation.
    Reconcile,
    /// Session inspection.
    Inspect,
    /// Ordered event read.
    Events,
    /// Result metadata read.
    Result,
    /// Authoritative termination.
    Terminate,
}

impl From<FakeOperation> for FakeCall {
    fn from(value: FakeOperation) -> Self {
        match value {
            FakeOperation::Negotiate => Self::Negotiate,
            FakeOperation::Adopt => Self::Adopt,
            FakeOperation::Lookup => Self::Lookup,
            FakeOperation::Reconcile => Self::Reconcile,
            FakeOperation::Inspect => Self::Inspect,
            FakeOperation::Events => Self::Events,
            FakeOperation::Result => Self::Result,
            FakeOperation::Terminate => Self::Terminate,
        }
    }
}

/// Typed response carried by a successful fake script step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CovenScriptReturn {
    /// Negotiated capability profile.
    Negotiate(CapabilityProfile),
    /// Adoption disposition.
    Adopt(AdoptionDisposition),
    /// Lookup disposition.
    Lookup(AdoptionDisposition),
    /// Reconciliation disposition.
    Reconcile(ReconciliationDisposition),
    /// Session snapshot.
    Inspect(SessionSnapshot),
    /// Event page.
    Events(EventPage),
    /// Result bundle.
    Result(ResultBundle),
    /// Termination disposition.
    Terminate(TerminationDisposition),
}

impl CovenScriptReturn {
    fn operation(&self) -> FakeOperation {
        match self {
            Self::Negotiate(_) => FakeOperation::Negotiate,
            Self::Adopt(_) => FakeOperation::Adopt,
            Self::Lookup(_) => FakeOperation::Lookup,
            Self::Reconcile(_) => FakeOperation::Reconcile,
            Self::Inspect(_) => FakeOperation::Inspect,
            Self::Events(_) => FakeOperation::Events,
            Self::Result(_) => FakeOperation::Result,
            Self::Terminate(_) => FakeOperation::Terminate,
        }
    }
}

/// One deterministic fake outcome or fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CovenScriptStep {
    /// Returns one typed response.
    Return(CovenScriptReturn),
    /// Returns one stable payload-free error.
    Error {
        /// Expected operation.
        operation: FakeOperation,
        /// Error returned by the operation.
        error: PortError,
    },
    /// Simulates a disconnect immediately before durable fake state changes.
    DisconnectBeforeCommit(FakeOperation),
    /// Commits the carried response and then simulates a lost reply.
    DisconnectAfterCommit(CovenScriptReturn),
    /// Deliberately returns a response that conflicts with a durable replay.
    ConflictingReplay(CovenScriptReturn),
    /// Leaves durable state unchanged and returns a deterministic stalled error.
    Stall(FakeOperation),
}

impl CovenScriptStep {
    fn operation(&self) -> FakeOperation {
        match self {
            Self::Return(response)
            | Self::DisconnectAfterCommit(response)
            | Self::ConflictingReplay(response) => response.operation(),
            Self::Error { operation, .. }
            | Self::DisconnectBeforeCommit(operation)
            | Self::Stall(operation) => *operation,
        }
    }
}

/// Fake construction failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FakeBuildError {
    /// A capability was advertised without any matching script step.
    #[error("advertised capability has no scripted behavior")]
    UnscriptedCapability {
        /// Typed capability missing a script.
        capability: Capability,
    },
    /// A configured contract or script is invalid.
    #[error("fake Coven configuration is invalid")]
    InvalidConfiguration,
}

/// Payload-free fake runtime failures use the same behavior error contract.
pub type FakeError = PortError;

/// Deterministic assertion invoked after request persistence and before response.
pub type BeforeTerminate =
    Arc<dyn Fn(&TerminationRequest) -> Result<(), PortError> + Send + Sync + 'static>;

#[derive(Default)]
struct FakeState {
    script: VecDeque<CovenScriptStep>,
    calls: Vec<FakeCall>,
    adoptions: BTreeMap<String, (Sha256Digest, Vec<u8>, AdoptionDisposition)>,
    sessions: BTreeMap<String, Vec<ExecutionCorrelation>>,
    reconciliations: BTreeMap<String, (ReconciliationRequest, ReconciliationDisposition)>,
    results: BTreeMap<String, ResultBundle>,
    terminations: BTreeMap<String, TerminationDisposition>,
}

/// Honest, deterministic, thread-safe Coven fake.
#[derive(Clone)]
pub struct FakeCoven {
    contract: String,
    capabilities: BTreeSet<String>,
    current_time: time::OffsetDateTime,
    state: Arc<Mutex<FakeState>>,
    before_terminate: Option<BeforeTerminate>,
}

impl fmt::Debug for FakeCoven {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FakeCoven")
    }
}

impl FakeCoven {
    /// Begins deterministic fake construction.
    pub fn builder() -> FakeCovenBuilder {
        FakeCovenBuilder::default()
    }

    /// Returns a redacted call log.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.state
            .lock()
            .map(|state| state.calls.clone())
            .unwrap_or_default()
    }

    /// Number of script steps not yet consumed.
    pub fn remaining_steps(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.script.len())
            .unwrap_or_default()
    }

    /// Simulates process restart while preserving only fake-owned durable state.
    pub fn restart(&self) -> Self {
        Self {
            contract: self.contract.clone(),
            capabilities: self.capabilities.clone(),
            current_time: self.current_time,
            state: Arc::clone(&self.state),
            before_terminate: self.before_terminate.clone(),
        }
    }

    /// Returns a restarted view with a new deterministic current time.
    pub fn at_time(&self, current_time: time::OffsetDateTime) -> Self {
        Self {
            contract: self.contract.clone(),
            capabilities: self.capabilities.clone(),
            current_time,
            state: Arc::clone(&self.state),
            before_terminate: self.before_terminate.clone(),
        }
    }

    fn record(&self, operation: FakeOperation) -> Result<(), PortError> {
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        state.calls.push(operation.into());
        Ok(())
    }

    fn take(&self, operation: FakeOperation) -> Result<CovenScriptStep, PortError> {
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        state.calls.push(operation.into());
        let Some(step) = state.script.front() else {
            return Err(PortError::UnexpectedCall);
        };
        if step.operation() != operation {
            return Err(PortError::UnexpectedCall);
        }
        state.script.pop_front().ok_or(PortError::UnexpectedCall)
    }

    fn store_adoption(
        &self,
        request: &AdoptionRequest,
        disposition: &AdoptionDisposition,
    ) -> Result<AdoptionDisposition, PortError> {
        disposition.validate()?;
        let correlation = request.correlation();
        let bytes = canonical_bytes(request.input()).map_err(PortError::from)?;
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        let key = correlation.request_id.as_str().to_owned();
        if let Some((stored_digest, stored_bytes, stored_disposition)) = state.adoptions.get(&key) {
            if stored_digest == request.request_digest() && stored_bytes == &bytes {
                return Ok(stored_disposition.clone());
            }
            return Err(PortError::IntentConflict);
        }
        if let AdoptionDisposition::Adopted { session_id } = disposition {
            let correlation = request.correlation();
            let correlations = state.sessions.entry(session_id.clone()).or_default();
            if !correlations.contains(&correlation) {
                correlations.push(correlation);
            }
        }
        state.adoptions.insert(
            key,
            (request.request_digest().clone(), bytes, disposition.clone()),
        );
        Ok(disposition.clone())
    }

    fn replay_adoption(
        &self,
        request: &AdoptionRequest,
    ) -> Result<Option<AdoptionDisposition>, PortError> {
        let correlation = request.correlation();
        let bytes = canonical_bytes(request.input()).map_err(PortError::from)?;
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        let Some((stored_digest, stored_bytes, stored_disposition)) =
            state.adoptions.get(correlation.request_id.as_str())
        else {
            return Ok(None);
        };
        if stored_digest == request.request_digest() && stored_bytes == &bytes {
            Ok(Some(stored_disposition.clone()))
        } else {
            Err(PortError::IntentConflict)
        }
    }

    fn lookup_adoption(
        &self,
        request_id: &RequestId,
        scripted: &AdoptionDisposition,
    ) -> Result<AdoptionDisposition, PortError> {
        scripted.validate()?;
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if let Some((_, _, stored)) = state.adoptions.get(request_id.as_str()) {
            if stored == scripted {
                Ok(stored.clone())
            } else {
                Err(PortError::IntentConflict)
            }
        } else {
            Ok(scripted.clone())
        }
    }

    fn store_reconciliation(
        &self,
        request: &ReconciliationRequest,
        disposition: &ReconciliationDisposition,
    ) -> Result<ReconciliationDisposition, PortError> {
        if disposition == &ReconciliationDisposition::Unresolved {
            return Ok(ReconciliationDisposition::Unresolved);
        }
        let key = request.correlation.request_id.as_str().to_owned();
        {
            let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
            if let Some((stored_request, stored_disposition)) = state.reconciliations.get(&key) {
                return if stored_request == request && stored_disposition == disposition {
                    Ok(stored_disposition.clone())
                } else {
                    Err(PortError::IntentConflict)
                };
            }
        }
        disposition.validate_for(request)?;
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if let Some((stored_request, stored_disposition)) = state.reconciliations.get(&key) {
            if stored_request == request && stored_disposition == disposition {
                return Ok(stored_disposition.clone());
            }
            return Err(PortError::IntentConflict);
        }
        state
            .reconciliations
            .insert(key, (request.clone(), disposition.clone()));
        Ok(disposition.clone())
    }

    fn replay_reconciliation(
        &self,
        request: &ReconciliationRequest,
    ) -> Result<Option<ReconciliationDisposition>, PortError> {
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        let Some((stored_request, stored_disposition)) = state
            .reconciliations
            .get(request.correlation.request_id.as_str())
        else {
            return Ok(None);
        };
        if stored_request == request {
            Ok(Some(stored_disposition.clone()))
        } else {
            Err(PortError::IntentConflict)
        }
    }

    fn store_result(&self, bundle: ResultBundle) -> Result<ResultBundle, PortError> {
        bundle.validate().map_err(|_| PortError::InvalidResponse)?;
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if !state.sessions.is_empty()
            && !state
                .sessions
                .get(&bundle.session_id)
                .is_some_and(|correlations| correlations.contains(&bundle.correlation))
        {
            return Err(PortError::CorrelationMismatch);
        }
        if let Some(stored) = state.results.get(&bundle.session_id) {
            return if stored == &bundle {
                Ok(stored.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        state
            .results
            .insert(bundle.session_id.clone(), bundle.clone());
        Ok(bundle)
    }

    fn replay_result(&self, session_id: &str) -> Result<Option<ResultBundle>, PortError> {
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        Ok(state.results.get(session_id).cloned())
    }

    fn validate_session_correlation(
        &self,
        session_id: &str,
        correlation: &ExecutionCorrelation,
    ) -> Result<(), PortError> {
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if !state.sessions.is_empty()
            && !state
                .sessions
                .get(session_id)
                .is_some_and(|stored| stored.contains(correlation))
        {
            Err(PortError::CorrelationMismatch)
        } else {
            Ok(())
        }
    }

    fn termination_key(request: &TerminationRequest) -> Result<String, PortError> {
        request
            .binding()
            .termination_request
            .as_ref()
            .map(|correlation| correlation.termination_request_id.as_str().to_owned())
            .ok_or(PortError::InvalidRequest)
    }

    fn store_termination(
        &self,
        key: String,
        disposition: TerminationDisposition,
    ) -> Result<TerminationDisposition, PortError> {
        let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        if let Some(stored) = state.terminations.get(&key) {
            return if stored == &disposition {
                Ok(stored.clone())
            } else {
                Err(PortError::IntentConflict)
            };
        }
        state.terminations.insert(key, disposition.clone());
        Ok(disposition)
    }

    fn replay_termination(&self, key: &str) -> Result<Option<TerminationDisposition>, PortError> {
        let state = self.state.lock().map_err(|_| PortError::Unavailable)?;
        Ok(state.terminations.get(key).cloned())
    }
}

/// Builder for [`FakeCoven`].
pub struct FakeCovenBuilder {
    contract: String,
    capabilities: BTreeSet<Capability>,
    current_time: time::OffsetDateTime,
    script: VecDeque<CovenScriptStep>,
    before_terminate: Option<BeforeTerminate>,
}

impl Default for FakeCovenBuilder {
    fn default() -> Self {
        Self {
            contract: "coven.daemon.v1".to_owned(),
            capabilities: BTreeSet::new(),
            current_time: time::OffsetDateTime::UNIX_EPOCH,
            script: VecDeque::new(),
            before_terminate: None,
        }
    }
}

impl fmt::Debug for FakeCovenBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FakeCovenBuilder")
    }
}

impl FakeCovenBuilder {
    /// Sets the exact negotiated contract.
    #[must_use]
    pub fn contract(mut self, contract: impl Into<String>) -> Self {
        self.contract = contract.into();
        self
    }

    /// Advertises one capability, which must have a matching script.
    #[must_use]
    pub fn capability(mut self, capability: Capability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    /// Sets the deterministic clock used for expiration checks.
    #[must_use]
    pub fn current_time(mut self, current_time: time::OffsetDateTime) -> Self {
        self.current_time = current_time;
        self
    }

    /// Replaces the script with an explicit ordered queue.
    #[must_use]
    pub fn script(mut self, script: VecDeque<CovenScriptStep>) -> Self {
        self.script = script;
        self
    }

    /// Appends one explicit script step.
    #[must_use]
    pub fn step(mut self, step: CovenScriptStep) -> Self {
        self.script.push_back(step);
        self
    }

    /// Scripts one adoption response.
    #[must_use]
    pub fn adoption(mut self, disposition: AdoptionDisposition) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Adopt(
                disposition,
            )));
        self
    }

    /// Scripts one lookup response.
    #[must_use]
    pub fn lookup(mut self, disposition: AdoptionDisposition) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Lookup(
                disposition,
            )));
        self
    }

    /// Scripts one reconciliation response.
    #[must_use]
    pub fn reconciliation(mut self, disposition: ReconciliationDisposition) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Reconcile(
                disposition,
            )));
        self
    }

    /// Scripts one session snapshot.
    #[must_use]
    pub fn snapshot(mut self, snapshot: SessionSnapshot) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Inspect(
                snapshot,
            )));
        self
    }

    /// Scripts one event page.
    #[must_use]
    pub fn event_page(mut self, page: EventPage) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Events(page)));
        self
    }

    /// Scripts one result bundle.
    #[must_use]
    pub fn result(mut self, bundle: ResultBundle) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Result(bundle)));
        self
    }

    /// Scripts one acknowledged termination response.
    #[must_use]
    pub fn acknowledge_termination(
        mut self,
        evidence: psyche_core::contracts::execution::CancellationAcknowledgementEvidence,
    ) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Terminate(
                TerminationDisposition::Acknowledged { evidence },
            )));
        self
    }

    /// Scripts one unresolved termination response.
    #[must_use]
    pub fn unresolved_termination(
        mut self,
        evidence: psyche_core::contracts::execution::CancellationUnresolvedEvidence,
    ) -> Self {
        self.script
            .push_back(CovenScriptStep::Return(CovenScriptReturn::Terminate(
                TerminationDisposition::Unresolved { evidence },
            )));
        self
    }

    /// Scripts a deliberately divergent response for coordinator conflict tests.
    #[must_use]
    pub fn conflicting_termination(mut self, disposition: TerminationDisposition) -> Self {
        self.script.push_back(CovenScriptStep::ConflictingReplay(
            CovenScriptReturn::Terminate(disposition),
        ));
        self
    }

    /// Installs a deterministic assertion immediately before terminate returns.
    #[must_use]
    pub fn before_terminate(mut self, assertion: BeforeTerminate) -> Self {
        self.before_terminate = Some(assertion);
        self
    }

    /// Validates honesty and constructs the fake.
    pub fn build(self) -> Result<FakeCoven, FakeBuildError> {
        let negotiation = NegotiateRequest::new(self.contract.clone());
        if negotiation.validate().is_err() {
            return Err(FakeBuildError::InvalidConfiguration);
        }
        if self.current_time.offset() != time::UtcOffset::UTC {
            return Err(FakeBuildError::InvalidConfiguration);
        }
        for capability in &self.capabilities {
            let operation = match capability {
                Capability::StableAdoption => FakeOperation::Adopt,
                Capability::AmbiguityFence => FakeOperation::Reconcile,
                Capability::OrderedEvents => FakeOperation::Events,
                Capability::AuthoritativeTermination => FakeOperation::Terminate,
                Capability::ContentAddressedResults => FakeOperation::Result,
            };
            if !self.script.iter().any(|step| step.operation() == operation) {
                return Err(FakeBuildError::UnscriptedCapability {
                    capability: *capability,
                });
            }
        }
        Ok(FakeCoven {
            contract: self.contract,
            capabilities: self
                .capabilities
                .into_iter()
                .map(|capability| capability.as_str().to_owned())
                .collect(),
            current_time: self.current_time,
            state: Arc::new(Mutex::new(FakeState {
                script: self.script,
                ..FakeState::default()
            })),
            before_terminate: self.before_terminate,
        })
    }
}

#[async_trait::async_trait]
impl CovenPort for FakeCoven {
    async fn negotiate(&self, request: NegotiateRequest) -> Result<CapabilityProfile, PortError> {
        request.validate()?;
        if request.required_api_version != self.contract {
            self.record(FakeOperation::Negotiate)?;
            return Err(PortError::ContractUnsupported {});
        }
        if !request.required_capabilities.is_subset(&self.capabilities) {
            self.record(FakeOperation::Negotiate)?;
            return Err(PortError::CapabilityMissing {});
        }
        let configured = CapabilityProfile {
            api_version: self.contract.clone(),
            capabilities: self.capabilities.clone(),
        };
        match self.take(FakeOperation::Negotiate)? {
            CovenScriptStep::Return(CovenScriptReturn::Negotiate(profile)) => {
                profile.validate().map_err(|_| PortError::InvalidResponse)?;
                if profile == configured {
                    Ok(profile)
                } else {
                    Err(PortError::InvalidResponse)
                }
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_)
            | CovenScriptStep::DisconnectAfterCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn adopt(&self, request: AdoptionRequest) -> Result<AdoptionDisposition, PortError> {
        request.validate_digest()?;
        if let Some(disposition) = self.replay_adoption(&request)? {
            self.record(FakeOperation::Adopt)?;
            return Ok(disposition);
        }
        if self.current_time > request.correlation().valid_until {
            return Err(PortError::InvalidRequest);
        }
        match self.take(FakeOperation::Adopt)? {
            CovenScriptStep::Return(CovenScriptReturn::Adopt(disposition)) => {
                self.store_adoption(&request, &disposition)
            }
            CovenScriptStep::DisconnectAfterCommit(CovenScriptReturn::Adopt(disposition)) => {
                self.store_adoption(&request, &disposition)?;
                Err(PortError::Unavailable)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            CovenScriptStep::ConflictingReplay(_) => Err(PortError::UnexpectedCall),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn lookup(&self, request_id: &RequestId) -> Result<AdoptionDisposition, PortError> {
        let durable = {
            let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
            let disposition = state
                .adoptions
                .get(request_id.as_str())
                .map(|(_, _, disposition)| disposition.clone());
            if disposition.is_some() {
                state.calls.push(FakeCall::Lookup);
            }
            disposition
        };
        if let Some(disposition) = durable {
            return Ok(disposition);
        }
        match self.take(FakeOperation::Lookup)? {
            CovenScriptStep::Return(CovenScriptReturn::Lookup(disposition)) => {
                self.lookup_adoption(request_id, &disposition)
            }
            CovenScriptStep::DisconnectAfterCommit(CovenScriptReturn::Lookup(disposition)) => {
                self.lookup_adoption(request_id, &disposition)?;
                Err(PortError::Unavailable)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn reconcile(
        &self,
        request: ReconciliationRequest,
    ) -> Result<ReconciliationDisposition, PortError> {
        request.validate()?;
        if let Some(disposition) = self.replay_reconciliation(&request)? {
            self.record(FakeOperation::Reconcile)?;
            return Ok(disposition);
        }
        match self.take(FakeOperation::Reconcile)? {
            CovenScriptStep::Return(CovenScriptReturn::Reconcile(disposition)) => {
                self.store_reconciliation(&request, &disposition)
            }
            CovenScriptStep::DisconnectAfterCommit(CovenScriptReturn::Reconcile(disposition)) => {
                self.store_reconciliation(&request, &disposition)?;
                Err(PortError::Unavailable)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn inspect(&self, session_id: &str) -> Result<SessionSnapshot, PortError> {
        if session_id.is_empty() || session_id.len() > 255 {
            return Err(PortError::InvalidRequest);
        }
        match self.take(FakeOperation::Inspect)? {
            CovenScriptStep::Return(CovenScriptReturn::Inspect(snapshot)) => {
                snapshot.validate()?;
                if snapshot.session_id == session_id {
                    self.validate_session_correlation(session_id, &snapshot.correlation)?;
                    Ok(snapshot)
                } else {
                    Err(PortError::CorrelationMismatch)
                }
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_)
            | CovenScriptStep::DisconnectAfterCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn events(&self, cursor: EventCursor) -> Result<EventPage, PortError> {
        cursor.validate()?;
        match self.take(FakeOperation::Events)? {
            CovenScriptStep::Return(CovenScriptReturn::Events(page)) => {
                page.validate_for(&cursor)?;
                Ok(page)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_)
            | CovenScriptStep::DisconnectAfterCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn result(&self, session_id: &str) -> Result<ResultBundle, PortError> {
        if session_id.is_empty() || session_id.len() > 255 {
            return Err(PortError::InvalidRequest);
        }
        if let Some(bundle) = self.replay_result(session_id)? {
            self.record(FakeOperation::Result)?;
            return Ok(bundle);
        }
        match self.take(FakeOperation::Result)? {
            CovenScriptStep::Return(CovenScriptReturn::Result(bundle)) => {
                if bundle.session_id == session_id {
                    self.store_result(bundle)
                } else {
                    Err(PortError::CorrelationMismatch)
                }
            }
            CovenScriptStep::DisconnectAfterCommit(CovenScriptReturn::Result(bundle)) => {
                if bundle.session_id != session_id {
                    return Err(PortError::CorrelationMismatch);
                }
                self.store_result(bundle)?;
                Err(PortError::Unavailable)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }

    async fn terminate(
        &self,
        request: TerminationRequest,
    ) -> Result<TerminationDisposition, PortError> {
        let key = Self::termination_key(&request)?;
        if let Some(stored) = self.replay_termination(&key)? {
            let scripted = {
                let mut state = self.state.lock().map_err(|_| PortError::Unavailable)?;
                match state.script.front() {
                    Some(CovenScriptStep::ConflictingReplay(CovenScriptReturn::Terminate(_)))
                    | Some(CovenScriptStep::Return(CovenScriptReturn::Terminate(_))) => {
                        state.calls.push(FakeCall::Terminate);
                        state.script.pop_front()
                    }
                    _ => None,
                }
            };
            if let Some(assertion) = &self.before_terminate {
                assertion(&request)?;
            }
            return match scripted {
                Some(CovenScriptStep::ConflictingReplay(CovenScriptReturn::Terminate(
                    disposition,
                ))) => Ok(disposition),
                Some(CovenScriptStep::Return(CovenScriptReturn::Terminate(disposition))) => {
                    if disposition == stored {
                        Ok(stored)
                    } else {
                        Err(PortError::IntentConflict)
                    }
                }
                Some(_) => Err(PortError::UnexpectedCall),
                None => {
                    self.record(FakeOperation::Terminate)?;
                    Ok(stored)
                }
            };
        }
        let step = self.take(FakeOperation::Terminate)?;
        if let CovenScriptStep::ConflictingReplay(_) = step {
            return Err(PortError::UnexpectedCall);
        }
        if let Some(assertion) = &self.before_terminate {
            assertion(&request)?;
        }
        match step {
            CovenScriptStep::Return(CovenScriptReturn::Terminate(disposition)) => {
                self.store_termination(key, disposition)
            }
            CovenScriptStep::DisconnectAfterCommit(CovenScriptReturn::Terminate(disposition)) => {
                self.store_termination(key, disposition)?;
                Err(PortError::Unavailable)
            }
            CovenScriptStep::Error { error, .. } => Err(error),
            CovenScriptStep::DisconnectBeforeCommit(_) => Err(PortError::Unavailable),
            CovenScriptStep::Stall(_) => Err(PortError::Stalled),
            _ => Err(PortError::UnexpectedCall),
        }
    }
}

/// Real Store-backed implementation of the narrow termination persistence port.
#[derive(Debug)]
pub struct StoreTerminationPersistence {
    store: Store,
}

impl StoreTerminationPersistence {
    /// Wraps an already-open durable Store.
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// Returns the wrapped Store.
    pub fn into_inner(self) -> Store {
        self.store
    }

    fn persist(
        &mut self,
        candidate: ExecutionBinding,
        phase: PersistencePhase,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<StoreError>> {
        let expected_bytes = canonical_bytes(&candidate).map_err(|error| {
            TerminationPersistenceFailure::Conflict(StoreError::Contract(error))
        })?;
        let history = self
            .store
            .execution_binding_revisions(&candidate.attempt_id)
            .map_err(classify_store_error)?;

        if let Some(existing) = history
            .iter()
            .find(|existing| existing.revision == candidate.revision)
        {
            let existing_bytes = canonical_bytes(existing).map_err(|error| {
                TerminationPersistenceFailure::Write(StoreError::Contract(error))
            })?;
            return if existing_bytes == expected_bytes {
                validate_persistence_predecessor(&history, &candidate, phase)?;
                Ok(existing_bytes)
            } else {
                Err(TerminationPersistenceFailure::Conflict(
                    StoreError::ExecutionBindingRevisionConflict {
                        attempt_id: candidate.attempt_id.clone(),
                        revision: candidate.revision,
                    },
                ))
            };
        }

        let Some(predecessor) = history.last() else {
            return Err(revision_conflict(&candidate));
        };
        if candidate.revision != predecessor.revision.saturating_add(1) {
            return Err(revision_conflict(&candidate));
        }
        validate_persistence_predecessor(&history, &candidate, phase)?;

        self.store
            .insert(&CanonicalDocument::ExecutionBinding(candidate.clone()))
            .map_err(classify_store_error)?;
        let committed = self
            .store
            .execution_binding_revisions(&candidate.attempt_id)
            .map_err(classify_store_error)?;
        let Some(committed) = committed
            .iter()
            .find(|binding| binding.revision == candidate.revision)
        else {
            return Err(TerminationPersistenceFailure::Write(
                StoreError::DatabaseOperation,
            ));
        };
        let committed_bytes = canonical_bytes(committed)
            .map_err(|error| TerminationPersistenceFailure::Write(StoreError::Contract(error)))?;
        if committed_bytes == expected_bytes {
            Ok(committed_bytes)
        } else {
            Err(revision_conflict(&candidate))
        }
    }
}

impl TerminationPersistence for StoreTerminationPersistence {
    type Error = StoreError;

    fn persist_requested(
        &mut self,
        requested: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.persist(requested, PersistencePhase::Requested)
    }

    fn persist_outcome(
        &mut self,
        outcome: ExecutionBinding,
    ) -> Result<Vec<u8>, TerminationPersistenceFailure<Self::Error>> {
        self.persist(outcome, PersistencePhase::Outcome)
    }
}

#[derive(Debug, Clone, Copy)]
enum PersistencePhase {
    Requested,
    Outcome,
}

fn validate_persistence_predecessor(
    history: &[ExecutionBinding],
    candidate: &ExecutionBinding,
    phase: PersistencePhase,
) -> Result<(), TerminationPersistenceFailure<StoreError>> {
    let Some(predecessor_revision) = candidate.revision.checked_sub(1) else {
        return Err(revision_conflict(candidate));
    };
    let Some(predecessor) = history
        .iter()
        .find(|binding| binding.revision == predecessor_revision)
    else {
        return Err(revision_conflict(candidate));
    };
    if candidate.previous_revision_digest.as_ref()
        != Some(
            &digest(predecessor).map_err(|error| {
                TerminationPersistenceFailure::Write(StoreError::Contract(error))
            })?,
        )
        || !frozen_execution_fields_match(predecessor, candidate)
        || predecessor
            .coven_session_id
            .as_deref()
            .is_none_or(str::is_empty)
        || predecessor.coven_session_id != candidate.coven_session_id
    {
        return Err(revision_conflict(candidate));
    }
    match phase {
        PersistencePhase::Requested => {
            if candidate.cancellation_state != CancellationState::TerminationRequested
                || predecessor.cancellation_state != CancellationState::NotRequested
            {
                return Err(revision_conflict(candidate));
            }
        }
        PersistencePhase::Outcome => {
            if predecessor.cancellation_state != CancellationState::TerminationRequested
                || !matches!(
                    candidate.cancellation_state,
                    CancellationState::AcknowledgedTerminated
                        | CancellationState::AcknowledgedAlreadyTerminal
                        | CancellationState::TerminationUnknown
                )
                || predecessor.termination_request != candidate.termination_request
                || predecessor.termination_reason_code != candidate.termination_reason_code
            {
                return Err(revision_conflict(candidate));
            }
        }
    }
    Ok(())
}

fn frozen_execution_fields_match(
    previous: &ExecutionBinding,
    candidate: &ExecutionBinding,
) -> bool {
    previous.attempt_id == candidate.attempt_id
        && previous.familiar_snapshot_id == candidate.familiar_snapshot_id
        && previous.project_id == candidate.project_id
        && previous.request_id == candidate.request_id
        && previous.request_digest == candidate.request_digest
        && timestamp_exact(previous.request_created_at, candidate.request_created_at)
        && timestamp_exact(previous.request_valid_until, candidate.request_valid_until)
        && previous.coven_contract_version == candidate.coven_contract_version
}

fn timestamp_exact(previous: time::OffsetDateTime, candidate: time::OffsetDateTime) -> bool {
    previous == candidate && previous.offset() == candidate.offset()
}

fn revision_conflict(candidate: &ExecutionBinding) -> TerminationPersistenceFailure<StoreError> {
    TerminationPersistenceFailure::Conflict(StoreError::ExecutionBindingRevisionConflict {
        attempt_id: candidate.attempt_id.clone(),
        revision: candidate.revision,
    })
}

fn classify_store_error(error: StoreError) -> TerminationPersistenceFailure<StoreError> {
    match error {
        error @ (StoreError::ExecutionBindingRevisionConflict { .. } | StoreError::Contract(_)) => {
            TerminationPersistenceFailure::Conflict(error)
        }
        error => TerminationPersistenceFailure::Write(error),
    }
}
