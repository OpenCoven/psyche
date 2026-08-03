//! Composition root. Owns the daemon lifecycle and the only shutdown path.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use psyche_config::Config;

/// Graceful shutdown stops intake, then drains, then exits. `Draining` is
/// observable so `psyche status` can distinguish it from `Running`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// Accepting work.
    Running,
    /// Intake stopped; in-flight work finishing.
    Draining,
    /// Fully stopped. Terminal.
    Stopped,
}

impl LifecycleState {
    /// Position in the lifecycle. The lifecycle only ever moves forward, and
    /// [`Lifecycle::advance`] enforces it against this.
    ///
    /// Deliberately not a public `Ord` derive: the ordering is an internal
    /// invariant of the state machine, and publishing it would invite
    /// `state < Stopped` comparisons that a future non-linear state (a failed
    /// or restarting runtime) could not honour.
    fn rank(self) -> u8 {
        match self {
            LifecycleState::Running => 0,
            LifecycleState::Draining => 1,
            LifecycleState::Stopped => 2,
        }
    }
}

/// Failures from driving the runtime lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// [`Runtime::shutdown`] was called on a runtime that had already begun
    /// stopping.
    #[error("runtime already stopped")]
    AlreadyStopped,
}

/// Current state plus the ordered log of states this runtime has occupied.
///
/// One mutex, not two: [`Runtime::shutdown`] must decide *and* publish its
/// transition without another caller interleaving, which is impossible if the
/// state and its log are separately locked.
#[derive(Debug)]
struct Lifecycle {
    current: LifecycleState,
    /// Bounded by construction: [`Lifecycle::advance`] refuses any move that is
    /// not strictly forward, and [`LifecycleState`] has three variants, so this
    /// holds at most three entries for the life of the process. An unguarded
    /// `push` would be a slow leak in a daemon that runs for months.
    history: Vec<LifecycleState>,
}

impl Lifecycle {
    /// Moves to `next` and records it, if `next` is strictly forward of the
    /// current state. Returns whether the move happened.
    ///
    /// Rejecting a non-forward move is what bounds `history`; it is not merely
    /// defensive. It also makes a double transition a no-op rather than a
    /// duplicate log entry that would break the ordering assertion.
    fn advance(&mut self, next: LifecycleState) -> bool {
        if next.rank() <= self.current.rank() {
            return false;
        }
        self.current = next;
        self.history.push(next);
        true
    }
}

/// The daemon composition root.
///
/// Deriving `Debug` is safe here only because [`psyche_config::Config`] redacts
/// its untyped `extensions` table, so `tracing::debug!(?runtime)` cannot print a
/// secret placed there. That property belongs to `Config` — if this field is
/// ever replaced with something that renders differently, this derive must be
/// revisited.
#[derive(Debug)]
pub struct Runtime {
    lifecycle: Arc<Mutex<Lifecycle>>,
    /// Held for the store and lease work that attaches at the drain point in
    /// the follow-on G2 plan; nothing in this slice reads it yet.
    #[expect(
        dead_code,
        reason = "consumed by the store/lease work in the G2 follow-on"
    )]
    config: Config,
}

impl Runtime {
    /// Builds the composition root and brings it to [`LifecycleState::Running`].
    ///
    /// `async` although nothing is awaited yet: the store and lease wiring in
    /// the follow-on G2 plan starts here, and widening a synchronous signature
    /// to `async` later would break every caller.
    pub async fn start(config: Config) -> Self {
        tracing::info!(state = "running", "psyche runtime started");
        Self {
            lifecycle: Arc::new(Mutex::new(Lifecycle {
                current: LifecycleState::Running,
                history: vec![LifecycleState::Running],
            })),
            config,
        }
    }

    /// Takes the lifecycle lock, recovering from poisoning instead of panicking.
    ///
    /// `expect` is denied outside tests, and the shutdown path is the last place
    /// that should panic — a daemon that panics on the way down leaves its
    /// socket and any lease behind. Poisoning means an earlier holder panicked,
    /// but every critical section here is a field assignment plus a `push` onto
    /// a three-element `Vec`, neither of which can leave `Lifecycle` in a
    /// half-written state. Recovering the guard is therefore sound, and the
    /// runtime still reaches `Stopped`.
    fn lifecycle(&self) -> MutexGuard<'_, Lifecycle> {
        self.lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The state this runtime currently occupies.
    #[must_use]
    pub fn state(&self) -> LifecycleState {
        self.lifecycle().current
    }

    /// Every state this runtime has occupied, oldest first.
    ///
    /// Ordered, not a set: the contract graceful shutdown owes an operator is
    /// that `Draining` happened *between* `Running` and `Stopped`, which a
    /// final-state check cannot distinguish from skipping the drain entirely.
    /// At most three entries — see [`Lifecycle::history`].
    #[must_use]
    pub fn transitions(&self) -> Vec<LifecycleState> {
        self.lifecycle().history.clone()
    }

    /// Records a forward transition, logging it if it happened, and reports
    /// whether it happened.
    ///
    /// The bool is the concurrency primitive: whichever caller gets `true` for
    /// [`LifecycleState::Draining`] owns the shutdown, because the test and the
    /// write both happen inside one lock acquisition.
    fn transition_to(&self, next: LifecycleState) -> bool {
        let moved = self.lifecycle().advance(next);
        if moved {
            tracing::info!(state = ?next, "psyche lifecycle transition");
        }
        moved
    }

    /// Stops intake, drains in-flight work, then exits. There is no forced
    /// path — a caller wanting immediate exit terminates the process.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::AlreadyStopped`] if shutdown has already been
    /// started by this or another caller. Exactly one concurrent caller is
    /// given the `Ok`.
    pub async fn shutdown(&self) -> Result<(), RuntimeError> {
        // Claim the shutdown and publish `Draining` under a single lock
        // acquisition. Testing the state and then transitioning in a second
        // acquisition would let two concurrent callers both observe `Running`
        // and both drive the machine, which once there is real drain work means
        // running it twice.
        if !self.transition_to(LifecycleState::Draining) {
            return Err(RuntimeError::AlreadyStopped);
        }

        // The drain seam. Nothing durable is in flight in this slice; the store
        // and lease work in the follow-on G2 plan awaits here. The guard is
        // deliberately dropped before this point — a `std` guard is `!Send`, so
        // holding one across the await this becomes would make the future
        // `!Send` and break the assertion below.

        self.transition_to(LifecycleState::Stopped);
        Ok(())
    }
}

// psyche-cli will hold this across tokio task boundaries.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<Runtime>();
    assert_send_sync_static::<RuntimeError>();
};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        psyche_config::load_str(
            r#"
schema_version = "psyche.config.v1"
data_dir = "/tmp/psyche-test"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn starts_running() {
        let rt = Runtime::start(test_config()).await;
        assert_eq!(rt.state(), LifecycleState::Running);
    }

    #[tokio::test]
    async fn shutdown_drains_then_stops_in_order() {
        let rt = Runtime::start(test_config()).await;
        rt.shutdown().await.unwrap();
        assert_eq!(rt.state(), LifecycleState::Stopped);
        assert_eq!(
            rt.transitions(),
            vec![
                LifecycleState::Running,
                LifecycleState::Draining,
                LifecycleState::Stopped
            ]
        );
    }

    #[tokio::test]
    async fn second_shutdown_is_an_error_not_a_panic() {
        let rt = Runtime::start(test_config()).await;
        rt.shutdown().await.unwrap();
        let err = rt.shutdown().await.unwrap_err();
        assert!(matches!(err, RuntimeError::AlreadyStopped));
    }

    // The transition log is what `psyche status` and the ordering assertion
    // above both read. A plain `push` would grow it on every rejected shutdown,
    // which in a daemon that is signalled repeatedly is an unbounded allocation.
    #[tokio::test]
    async fn the_transition_log_is_bounded_by_the_state_count() {
        let rt = Runtime::start(test_config()).await;
        for _ in 0..1_000 {
            let _ = rt.shutdown().await;
        }
        assert_eq!(rt.transitions().len(), 3, "{:?}", rt.transitions());
    }

    // Two callers must not both drive the machine: with a separate test and
    // transition, both could observe `Running` and both proceed, which once the
    // drain seam does real work means draining twice.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_shutdowns_elect_exactly_one_winner() {
        let rt = Arc::new(Runtime::start(test_config()).await);
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let rt = Arc::clone(&rt);
            tasks.push(tokio::spawn(async move { rt.shutdown().await.is_ok() }));
        }
        let mut winners = 0;
        for task in tasks {
            if task.await.unwrap() {
                winners += 1;
            }
        }
        assert_eq!(
            winners, 1,
            "expected exactly one caller to own the shutdown"
        );
        assert_eq!(
            rt.transitions(),
            vec![
                LifecycleState::Running,
                LifecycleState::Draining,
                LifecycleState::Stopped
            ]
        );
    }

    // `Runtime` derives `Debug` purely on the strength of `Config` redacting its
    // untyped extensions table. Asserting it here means replacing the field with
    // something that renders differently fails a test rather than quietly
    // turning `tracing::debug!(?runtime)` into a secret disclosure.
    #[tokio::test]
    async fn debug_does_not_print_an_extension_secret() {
        let secretish = "A".repeat(30);
        let raw = format!(
            r#"
schema_version = "psyche.config.v1"
data_dir = "/tmp/psyche-test"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"

[extensions."psyche.experiment.v1"]
looks_like_a_secret = "{secretish}"
"#
        );
        let rt = Runtime::start(psyche_config::load_str(&raw).unwrap()).await;
        let rendered = format!("{rt:?}");
        assert!(!rendered.contains("looks_like_a_secret"), "{rendered}");
        assert!(!rendered.contains(&secretish), "{rendered}");
        assert!(rendered.contains("redacted"), "{rendered}");
    }

    #[test]
    fn a_lifecycle_only_ever_moves_forward() {
        let mut lifecycle = Lifecycle {
            current: LifecycleState::Running,
            history: vec![LifecycleState::Running],
        };
        assert!(!lifecycle.advance(LifecycleState::Running));
        assert!(lifecycle.advance(LifecycleState::Stopped));
        // Backwards from the terminal state, which is what a resurrected
        // runtime would look like to `psyche status`.
        assert!(!lifecycle.advance(LifecycleState::Draining));
        assert_eq!(lifecycle.current, LifecycleState::Stopped);
        assert_eq!(
            lifecycle.history,
            vec![LifecycleState::Running, LifecycleState::Stopped]
        );
    }

    // A poisoned lock must not take the daemon's shutdown path down with it.
    #[tokio::test]
    async fn a_poisoned_lock_does_not_panic_the_shutdown_path() {
        let rt = Runtime::start(test_config()).await;
        let lock = Arc::clone(&rt.lifecycle);
        std::thread::spawn(move || {
            let _guard = lock.lock().unwrap();
            panic!("poison the lifecycle mutex");
        })
        .join()
        .expect_err("the spawned thread is expected to panic");
        assert!(rt.lifecycle.is_poisoned());

        assert_eq!(rt.state(), LifecycleState::Running);
        rt.shutdown().await.unwrap();
        assert_eq!(rt.state(), LifecycleState::Stopped);
    }
}
