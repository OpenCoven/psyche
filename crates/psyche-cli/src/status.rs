//! What `psyche status` may say, and what it may not.
//!
//! `status` runs in a different process from the daemon and this build has no
//! IPC, so it cannot see a running `psyched`. The previous document said
//! `{"state":"stopped","observed":false}` — a false statement on any host where
//! a daemon *is* running, and `jq -r .state` is what a consumer actually writes.
//! An `observed` flag next to a populated `state` is an invitation to read the
//! state and ignore the flag.
//!
//! So the caveat is structural rather than advisory: [`Observation`] can hold a
//! state or a reason, never both, and the rendering below cannot emit a `state`
//! it was not given. A consumer that learns to trust a bare `state` field, and is
//! told about the caveat in a later release, has already shipped the code that
//! ignores it.

use psyche_runtime::LifecycleState;

/// Schema identifier on `status --json` output.
///
/// Versioned like `psyche.config.v1` and `coven.daemon.v1`. This repository
/// treats schema versioning as first-class for its configuration and for the
/// Coven API; its own machine-readable output is owed the same, and the moment
/// to add the envelope is before anyone parses a document without one.
pub const STATUS_SCHEMA: &str = "psyche.status.v1";

/// Why no state was observed.
///
/// A closed vocabulary, and deliberately a Rust enum rather than a free string:
/// a consumer branching on `reason` needs the set to be enumerable, and a
/// `format!` at the call site is how it stops being.
///
/// `NoIpc` is the only variant this build can produce, because there is no IPC
/// at all yet. The IPC work extends this with the distinctions that only exist
/// once there is a socket to fail against:
///
/// - `socket-absent` — the configured path does not exist; nothing is running.
/// - `connect-refused` — the path exists but nothing is listening, which is a
///   stale socket rather than an absent daemon.
/// - `permission-denied` — a daemon may well be running; this caller cannot ask.
///
/// They are named here rather than added now because a variant nothing
/// constructs implies a distinction the build cannot actually draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unobserved {
    /// This build has no daemon IPC, so no state can be read from anywhere.
    NoIpc,
}

/// The wire spelling of a reason. One definition, used by both renderings.
impl std::fmt::Display for Unobserved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Unobserved::NoIpc => "no-ipc",
        })
    }
}

/// What `status` managed to find out.
///
/// The invariant "`state` is populated only when `observed` is true" lives in
/// this type rather than in the code that builds the document, so there is no
/// way to write a renderer that violates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// A daemon was reached and reported this state.
    ///
    /// Unreachable in this build. It exists so that the IPC work adds a call
    /// site rather than a field, and so the renderings below are already written
    /// for the answer they will eventually have to give.
    Observed(LifecycleState),
    /// No state was read, for this reason.
    Unobserved(Unobserved),
}

/// The machine rendering: a versioned `psyche.status.v1` document.
///
/// `state` is `null` rather than absent when nothing was observed — a consumer
/// distinguishing "no state" from "no such field" should not have to — and
/// `reason` is present only in that case, because a reason alongside an answer
/// would be a reason for nothing.
#[must_use]
pub fn render_json(observation: &Observation) -> String {
    // The state's spelling comes from `LifecycleState`'s `Display`, never from a
    // literal here, so the wire word has exactly one definition.
    let (state, reason) = match observation {
        Observation::Observed(state) => (
            serde_json::Value::String(state.to_string()),
            serde_json::Value::Null,
        ),
        Observation::Unobserved(reason) => (
            serde_json::Value::Null,
            serde_json::Value::String(reason.to_string()),
        ),
    };
    serde_json::json!({
        "schema": STATUS_SCHEMA,
        "observed": matches!(observation, Observation::Observed(_)),
        "state": state,
        "reason": reason,
    })
    .to_string()
}

/// The human rendering, carrying the same caveat.
///
/// Names no state when none was observed. The previous line led with
/// `state: stopped (not observed: ...)`, and an operator skimming for a state
/// word found one.
#[must_use]
pub fn render_text(observation: &Observation) -> String {
    match observation {
        Observation::Observed(state) => format!("state: {state}\n"),
        Observation::Unobserved(reason) => {
            format!("state: not observed ({reason}: no daemon IPC in this build)\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unobserved_status_carries_a_null_state_and_a_reason() {
        let document: serde_json::Value =
            serde_json::from_str(&render_json(&Observation::Unobserved(Unobserved::NoIpc)))
                .unwrap();

        assert_eq!(document["schema"], serde_json::json!(STATUS_SCHEMA));
        assert_eq!(document["observed"], serde_json::json!(false));
        assert_eq!(document["state"], serde_json::Value::Null);
        assert_eq!(document["reason"], serde_json::json!("no-ipc"));
    }

    /// The shape the IPC work will produce. Asserted now so that adding the call
    /// site later is not also a change to the document.
    #[test]
    fn an_observed_status_carries_the_state_and_no_reason() {
        let document: serde_json::Value = serde_json::from_str(&render_json(
            &Observation::Observed(LifecycleState::Draining),
        ))
        .unwrap();

        assert_eq!(document["observed"], serde_json::json!(true));
        assert_eq!(document["state"], serde_json::json!("draining"));
        assert_eq!(document["reason"], serde_json::Value::Null);
    }

    /// The two renderings must agree about how much the command knows. The text
    /// form previously said `stopped` while the JSON form said `observed: false`.
    #[test]
    fn the_text_rendering_names_no_state_it_did_not_observe() {
        let rendered = render_text(&Observation::Unobserved(Unobserved::NoIpc));
        assert!(rendered.contains("not observed"), "{rendered}");
        assert!(rendered.contains("no-ipc"), "{rendered}");
        for state in ["running", "draining", "stopped"] {
            assert!(!rendered.contains(state), "{rendered}");
        }
    }

    #[test]
    fn reasons_render_as_their_wire_spellings() {
        assert_eq!(Unobserved::NoIpc.to_string(), "no-ipc");
    }
}
