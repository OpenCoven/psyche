//! Composition root. Owns the daemon lifecycle and the only shutdown path.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use psyche_config::Config;
use tokio::sync::watch;

/// Graceful shutdown stops intake, then drains, then exits. `Draining` is
/// observable so `psyche status` can distinguish it from `Running`.
///
/// Deliberately **not** `#[non_exhaustive]`, unlike [`RuntimeError`]. A new
/// state is a new thing an operator can be told, and it *should* break every
/// renderer until each one decides how to say it. `#[non_exhaustive]` would push
/// consumers into a `_` arm that silently misreports the new state as whatever
/// the fallback says.
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

/// The wire spelling of a state: `running`, `draining`, `stopped`.
///
/// This exists so no consumer has to reach for `{:?}`. A `Debug` rendering that
/// something else formats into JSON or a log field becomes a compatibility
/// surface that can never be renamed — and `Debug` here would emit the
/// capitalised variant name, which is not what any of them want.
impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LifecycleState::Running => "running",
            LifecycleState::Draining => "draining",
            LifecycleState::Stopped => "stopped",
        })
    }
}

/// Failures from driving the runtime lifecycle.
///
/// Deliberately empty. Nothing in this slice can fail: [`Runtime::start`] does
/// no I/O yet, and a losing [`Runtime::shutdown`] caller waits for the winner
/// and returns `Ok` rather than erroring. The type and the `Result` signatures
/// exist so that the first real failure — opening `data_dir`, binding the Coven
/// socket, acquiring a lease — is an added variant rather than a breaking
/// signature change.
///
/// Do not add a variant speculatively. Add it with the code that returns it. An
/// earlier draft carried a `ShutdownInProgress` variant that nothing
/// constructed; it implied to every reader that `shutdown` refuses a second
/// caller, which is the behaviour the waiting loser deliberately replaced.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {}

/// Which side of the shutdown election a caller ended up on.
///
/// Private, and the reason [`Runtime::shutdown`] is a thin wrapper over
/// [`Runtime::shutdown_inner`]: the public contract is that *the runtime is
/// stopped*, which is true for both roles, but "exactly one caller advances the
/// state machine" is an internal invariant that the concurrency test has to be
/// able to see. Exposing it publicly would be API no caller has asked for; if
/// one ever does, the extension point is `Ok(ShutdownOutcome)`, never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownRole {
    /// Won the election and drove the drain.
    Driver,
    /// Lost the election and waited for the driver to reach
    /// [`LifecycleState::Stopped`].
    Observer,
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
    /// Publishes each transition to [`Runtime::subscribe`]rs, and is how a
    /// losing shutdown caller waits for the drain it did not drive.
    ///
    /// The mutex above remains the election; this is only the announcement.
    /// `watch::Sender::send` has no compare-and-swap, so deciding the transition
    /// here instead would lose the single-acquisition property that the
    /// 24,000-attempt concurrency test exists to protect.
    state_tx: watch::Sender<LifecycleState>,
    config: Config,
}

impl Runtime {
    /// Builds the composition root and brings it to [`LifecycleState::Running`].
    ///
    /// `async` although nothing is awaited yet, and fallible although nothing
    /// fails yet, for the same reason: the store and lease wiring in the
    /// follow-on G2 plan starts here, and that work — opening `data_dir`,
    /// binding the Coven socket, acquiring a lease — is exactly what fails.
    /// Widening either signature later would break every caller, which is the
    /// break these two decisions exist to avoid.
    ///
    /// # Errors
    ///
    /// None are possible in this build — [`RuntimeError`] has no variants, so
    /// this always returns `Ok`. The signature is the point: the first thing
    /// startup acquires becomes a variant, not a breaking change.
    pub async fn start(config: Config) -> Result<Self, RuntimeError> {
        // `Sender::new`, not `watch::channel(..)`: `channel` also hands back a
        // `Receiver` that this type has no use for, and dropping it would leave
        // a sender with no receivers. See `transition_to` for why that matters.
        let state_tx = watch::Sender::new(LifecycleState::Running);
        tracing::info!(state = %LifecycleState::Running, "psyche runtime started");
        Ok(Self {
            lifecycle: Arc::new(Mutex::new(Lifecycle {
                current: LifecycleState::Running,
                history: vec![LifecycleState::Running],
            })),
            state_tx,
            config,
        })
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

    /// The configuration this runtime was started with.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// A receiver that observes each state this runtime enters.
    ///
    /// The only way to learn that a runtime has stopped without polling
    /// [`Runtime::state`]. Two properties of `watch` a caller must know:
    ///
    /// - The current state counts as already seen, so `changed()` reports the
    ///   *next* transition. Read [`watch::Receiver::borrow`] first if the state
    ///   at subscription time matters.
    /// - Values coalesce. A receiver that does not keep up sees the latest
    ///   state, not every state — a subscriber polled once after a fast
    ///   shutdown observes `Stopped` and never `Draining`. [`Runtime::transitions`]
    ///   is the lossless record; this is the live one.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<LifecycleState> {
        self.state_tx.subscribe()
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

    /// Records a forward transition, publishing and logging it if it happened,
    /// and reports whether it happened.
    ///
    /// The bool is the concurrency primitive: whichever caller gets `true` for
    /// [`LifecycleState::Draining`] owns the shutdown, because the test and the
    /// write both happen inside one lock acquisition.
    fn transition_to(&self, next: LifecycleState) -> bool {
        let mut lifecycle = self.lifecycle();
        if !lifecycle.advance(next) {
            return false;
        }
        // Published while the guard is still held, so a subscriber can never
        // observe a state the machine has not entered, and two transitions
        // cannot be published in the opposite order to the one they were made.
        //
        // `send_replace`, not `send`: `send` fails when no receiver exists and
        // — this is the part that bites — does not store the value it failed to
        // deliver. A `let _ = send(..)` here would leave the published state at
        // `Running` for a runtime nobody had subscribed to yet, and the first
        // losing caller to subscribe would then wait on a `Stopped` that had
        // already been sent and dropped. `send_replace` always stores.
        self.state_tx.send_replace(next);
        // Dropped before logging: a subscriber's `Layer` runs arbitrary code on
        // this thread, and panicking with the lifecycle lock held would poison
        // it in the middle of shutdown.
        drop(lifecycle);
        tracing::info!(state = %next, "psyche lifecycle transition");
        true
    }

    /// Waits until [`LifecycleState::Stopped`] has been published.
    ///
    /// Returns immediately if it already has, which is the common case for a
    /// caller arriving after the runtime has fully stopped.
    async fn await_stopped(&self) {
        let mut receiver = self.subscribe();
        // `borrow_and_update` before `changed`, in that order and in a loop:
        // `subscribe` marks the current value as seen, so awaiting `changed`
        // first would miss a `Stopped` that was published before this caller
        // subscribed, and wait for a transition that can never come.
        while *receiver.borrow_and_update() != LifecycleState::Stopped {
            if receiver.changed().await.is_err() {
                // Unreachable: the sender lives in `self`, which this call
                // borrows. Breaking rather than panicking anyway — this is the
                // shutdown path.
                break;
            }
        }
    }

    /// Stops intake, drains in-flight work, then exits. There is no forced
    /// path — a caller wanting immediate exit terminates the process.
    ///
    /// Safe to call from any number of callers. Exactly one drives the drain;
    /// the rest wait for it and return once the runtime is
    /// [`LifecycleState::Stopped`]. That waiting is not politeness, it is the
    /// contract: an operator whose first SIGTERM appears to do nothing sends a
    /// second, and a caller that returned immediately would let `psyched` fall
    /// out of `main` and exit the process mid-drain — graceful shutdown
    /// defeated by ordinary operator behaviour.
    ///
    /// Returns `Ok(())` for both roles. A caller that merely waited still has
    /// the answer it asked for — the runtime is stopped, and it stopped
    /// gracefully. An error would be false by the time it was returned, and
    /// every caller would have to translate it back into success.
    ///
    /// # Errors
    ///
    /// None are possible in this build — [`RuntimeError`] has no variants, so
    /// this always returns `Ok`. Losing the election is explicitly *not* an
    /// error; that is what the paragraph above is about.
    pub async fn shutdown(&self) -> Result<(), RuntimeError> {
        match self.shutdown_inner().await? {
            // Both roles, one answer — see the note above.
            ShutdownRole::Driver | ShutdownRole::Observer => Ok(()),
        }
    }

    /// [`Runtime::shutdown`], plus which side of the election the caller was on.
    async fn shutdown_inner(&self) -> Result<ShutdownRole, RuntimeError> {
        // Claim the shutdown and publish `Draining` under a single lock
        // acquisition. Testing the state and then transitioning in a second
        // acquisition would let two concurrent callers both observe `Running`
        // and both drive the machine, which once there is real drain work means
        // running it twice.
        if !self.transition_to(LifecycleState::Draining) {
            self.await_stopped().await;
            return Ok(ShutdownRole::Observer);
        }

        // The drain seam. Nothing durable is in flight in this slice; the store
        // and lease work in the follow-on G2 plan awaits here. No lifecycle
        // guard is live across it — `transition_to` takes and drops its own —
        // which is what keeps this future `Send`; the assertion below fails to
        // compile if that stops being true.

        self.transition_to(LifecycleState::Stopped);
        Ok(ShutdownRole::Driver)
    }
}

// These bounds are asserted here rather than left to the first caller that needs
// them. No consumer requires them *today* — psyche-cli awaits both futures
// inline on one task and contains no `tokio::spawn`, no `Arc<Runtime>` and no
// sharing across threads — but a daemon that supervises anything acquires all
// three, and the point of asserting them now is the note below about where the
// error would otherwise appear.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<Runtime>();
    assert_send_sync_static::<RuntimeError>();

    // The types being `Send` is not the property the drain seam needs. Holding a
    // `std` `MutexGuard` across the await in `shutdown` would leave `Runtime`
    // perfectly `Send` and make the *future* `!Send` — a distinction that costs
    // nothing until someone writes `tokio::spawn(runtime.shutdown())`, at which
    // point the compiler reports it against the spawn in the consuming crate
    // rather than against the seam that caused it. These two assertions put the
    // error here instead, where the fix is.
    fn assert_send<T: Send>(_: &T) {}
    fn futures_are_send(runtime: &Runtime, config: Config) {
        assert_send(&Runtime::start(config));
        assert_send(&runtime.shutdown());
    }
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
        let rt = Runtime::start(test_config()).await.unwrap();
        assert_eq!(rt.state(), LifecycleState::Running);
    }

    #[tokio::test]
    async fn the_configuration_is_readable_after_start() {
        let rt = Runtime::start(test_config()).await.unwrap();
        assert_eq!(
            rt.config().data_dir,
            std::path::Path::new("/tmp/psyche-test")
        );
    }

    // The wire spellings are what `psyche status --json` emits and what log
    // filters match on. Pinned against the literals, not against each other, so
    // renaming a variant cannot silently rename a field an operator scripts.
    #[test]
    fn states_render_as_their_wire_spellings() {
        assert_eq!(LifecycleState::Running.to_string(), "running");
        assert_eq!(LifecycleState::Draining.to_string(), "draining");
        assert_eq!(LifecycleState::Stopped.to_string(), "stopped");
    }

    #[tokio::test]
    async fn shutdown_drains_then_stops_in_order() {
        let rt = Runtime::start(test_config()).await.unwrap();
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

    // Previously asserted that a second shutdown is an error. It is not any
    // more: it waits for the first to finish and reports the truth, which for a
    // runtime that has already stopped is available immediately.
    //
    // Both calls are bounded by a timeout because the failure they guard
    // against is a *hang*, not a wrong answer: a `transition_to` that publishes
    // with `send` rather than `send_replace` stores nothing when there are no
    // receivers, so the second call waits on a `Stopped` that was announced to
    // nobody and never recorded. Verified by mutation — without the timeout
    // that swap wedges the suite indefinitely instead of failing it, which in
    // CI is a stuck job rather than a red one.
    #[tokio::test]
    async fn a_later_shutdown_succeeds_without_redriving_the_drain() {
        use std::time::Duration;

        let rt = Runtime::start(test_config()).await.unwrap();
        for attempt in 0..2 {
            tokio::time::timeout(Duration::from_secs(5), rt.shutdown())
                .await
                .unwrap_or_else(|_| panic!("shutdown {attempt} never returned"))
                .unwrap();
        }
        assert_eq!(rt.state(), LifecycleState::Stopped);
        assert_eq!(rt.transitions().len(), 3, "{:?}", rt.transitions());
    }

    // The role, not just the outcome: with both callers returning `Ok`, this is
    // the only place the election is visible.
    #[tokio::test]
    async fn only_the_first_caller_drives_the_drain() {
        let rt = Runtime::start(test_config()).await.unwrap();
        assert_eq!(rt.shutdown_inner().await.unwrap(), ShutdownRole::Driver);
        assert_eq!(rt.shutdown_inner().await.unwrap(), ShutdownRole::Observer);
    }

    // The property A4 exists for: a caller that lost the election must not
    // return while the drain is still running. Driving the transitions directly
    // rather than racing two `shutdown` calls is deliberate — the drain seam
    // contains no await, so a real winner passes from `Draining` to `Stopped`
    // within a single poll and there is no window a second task could be
    // scheduled in. A test that tried to hit that window would be measuring the
    // scheduler, and would pass against a loser that returns immediately
    // whenever the winner happened to finish first.
    #[tokio::test]
    async fn a_losing_caller_does_not_return_before_stopped_is_published() {
        use std::time::Duration;

        let rt = Runtime::start(test_config()).await.unwrap();
        // Enter `Draining` with no winner running, so nothing will publish
        // `Stopped` unless this test does.
        assert!(rt.transition_to(LifecycleState::Draining));

        let waited = tokio::time::timeout(Duration::from_millis(250), rt.shutdown()).await;
        assert!(
            waited.is_err(),
            "a losing caller returned while the runtime was still {}",
            rt.state()
        );

        assert!(rt.transition_to(LifecycleState::Stopped));
        // Now that `Stopped` is published the same call returns, and promptly:
        // a caller arriving after the drain has finished must not block either.
        tokio::time::timeout(Duration::from_millis(250), rt.shutdown())
            .await
            .unwrap()
            .unwrap();
    }

    // The transition log is what `psyche status` and the ordering assertion
    // above both read. A plain `push` would grow it on every rejected shutdown,
    // which in a daemon that is signalled repeatedly is an unbounded allocation.
    #[tokio::test]
    async fn the_transition_log_is_bounded_by_the_state_count() {
        let rt = Runtime::start(test_config()).await.unwrap();
        for _ in 0..1_000 {
            let _ = rt.shutdown().await;
        }
        assert_eq!(rt.transitions().len(), 3, "{:?}", rt.transitions());
    }

    // A subscriber that keeps up sees every state, in order, with none skipped.
    // The transitions are driven by the test rather than by `shutdown` because
    // `watch` coalesces: against a real shutdown, whose two transitions happen
    // in one poll, a receiver is *expected* to observe only `Stopped`, and an
    // assertion of the full sequence would be green or red depending on the
    // scheduler. This form asserts the channel's ordering guarantee, which is
    // the part that is actually a guarantee.
    #[tokio::test]
    async fn a_subscriber_that_keeps_up_observes_every_state_in_order() {
        let rt = Runtime::start(test_config()).await.unwrap();
        let mut receiver = rt.subscribe();

        let mut seen = vec![*receiver.borrow_and_update()];
        for next in [LifecycleState::Draining, LifecycleState::Stopped] {
            assert!(rt.transition_to(next));
            receiver.changed().await.unwrap();
            seen.push(*receiver.borrow_and_update());
        }

        assert_eq!(
            seen,
            vec![
                LifecycleState::Running,
                LifecycleState::Draining,
                LifecycleState::Stopped
            ]
        );
    }

    // A subscriber created after the fact still learns the current state, which
    // is what makes `await_stopped` safe for a caller that arrives late.
    #[tokio::test]
    async fn a_late_subscriber_sees_the_state_the_runtime_is_actually_in() {
        let rt = Runtime::start(test_config()).await.unwrap();
        rt.shutdown().await.unwrap();
        assert_eq!(*rt.subscribe().borrow(), LifecycleState::Stopped);
    }

    // Two callers must not both drive the machine: with a separate test and
    // transition, both could observe `Running` and both proceed, which once the
    // drain seam does real work means draining twice.
    //
    // OS threads released by a `Barrier`, deliberately not tokio tasks.
    // `shutdown` contains no await inside its critical region, so tokio tasks
    // run to completion before the next is polled and never overlap no matter
    // how many workers the runtime has — a task-based version of this test
    // passes against a check-then-act implementation, which is the exact
    // regression it exists to catch.
    //
    // Mutation-tested rather than assumed. Against a `shutdown` that tests the
    // state and then transitions in a second lock acquisition, the previous
    // tokio-task form of this test detected the race 0 times in 60 runs; this
    // form detected it 60 times in 60, and 60/60 again against the wider
    // check-`== Stopped`-then-transition variant. If these constants are
    // lowered, re-run that mutation — at 8 threads and 200 rounds detection
    // drops to 42/60, which is not a guard.
    //
    // Many short rounds rather than one long one: the window a racy
    // implementation opens is the gap between dropping the lock after the test
    // and retaking it for the write, so what matters is how often threads
    // arrive at a *fresh* `Running` runtime together, not how long any one of
    // them runs.
    //
    // The reader thread added alongside them takes part in the same barrier, so
    // the contenders are still released together and the constants above still
    // mean what they meant when they were measured.
    #[test]
    fn concurrent_shutdowns_elect_exactly_one_winner() {
        use std::sync::Barrier;
        use std::sync::atomic::{AtomicUsize, Ordering};

        const THREADS: usize = 16;
        const ROUNDS: usize = 1_000;

        // One executor for the whole test. `Handle::block_on` drives the future
        // on the *calling* thread rather than handing it to a worker, so the
        // threads below contend directly instead of being serialised through
        // the scheduler.
        let executor = tokio::runtime::Builder::new_multi_thread().build().unwrap();

        for round in 0..ROUNDS {
            let rt = Arc::new(executor.block_on(Runtime::start(test_config())).unwrap());
            // +1 for the reader.
            let barrier = Arc::new(Barrier::new(THREADS + 1));
            let drivers = Arc::new(AtomicUsize::new(0));

            std::thread::scope(|scope| {
                for _ in 0..THREADS {
                    let rt = Arc::clone(&rt);
                    let barrier = Arc::clone(&barrier);
                    let drivers = Arc::clone(&drivers);
                    let handle = executor.handle().clone();
                    scope.spawn(move || {
                        // Everything that can be done ahead of the contended
                        // section is done before the barrier, so the threads
                        // are released as close to simultaneously as the OS
                        // allows.
                        let shutdown = async { rt.shutdown_inner().await };
                        barrier.wait();
                        // `matches!`, not `==`: `RuntimeError` is deliberately
                        // not `PartialEq`, and comparing errors is not what
                        // this asserts anyway.
                        if matches!(handle.block_on(shutdown), Ok(ShutdownRole::Driver)) {
                            drivers.fetch_add(1, Ordering::SeqCst);
                        }
                        // Whatever this caller's role was, `shutdown` returning
                        // means the runtime is stopped. A loser that returned
                        // early would be caught here observing `Running` or
                        // `Draining`.
                        assert_eq!(
                            rt.state(),
                            LifecycleState::Stopped,
                            "round {round}: shutdown returned before the runtime stopped"
                        );
                    });
                }

                // Read-only, and asserts the one thing a concurrent observer of
                // a state machine must be able to rely on.
                let rt = Arc::clone(&rt);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    let mut highest = LifecycleState::Running.rank();
                    for _ in 0..10_000 {
                        let observed = rt.state().rank();
                        assert!(
                            observed >= highest,
                            "round {round}: state went backwards, {observed} after {highest}"
                        );
                        highest = observed;
                        if highest == LifecycleState::Stopped.rank() {
                            break;
                        }
                    }
                });
            });

            assert_eq!(
                drivers.load(Ordering::SeqCst),
                1,
                "round {round}: expected exactly one caller to own the shutdown"
            );
            assert_eq!(
                rt.transitions(),
                vec![
                    LifecycleState::Running,
                    LifecycleState::Draining,
                    LifecycleState::Stopped
                ],
                "round {round}"
            );
        }
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
        let rt = Runtime::start(psyche_config::load_str(&raw).unwrap())
            .await
            .unwrap();
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
        let rt = Runtime::start(test_config()).await.unwrap();
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
