//! Local, observation-only Build receipt verification. Registration snapshots the original operator inputs. Supplying evidence never calls a dispatch port.

mod snapshot;
mod verifier;

use std::path::Path;

use psyche_config::Config;
use psyche_core::contracts::build_receipt::{BuildReference, deserialize_object};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use psyche_core::id::RequestId;
use psyche_coven::{AdoptionRequest, ExecutionCorrelation, ExecutionRequestInput};
use psyche_store::receipt_verification::{
    VerificationDecision, VerificationEvidence, VerificationInput, VerificationSummary,
};
use psyche_store::{Store, StoreError};
use serde::{Deserialize, Serialize};

const PROFILE_LIMIT: u64 = psyche_store::receipt_verification::INPUT_LIMIT as u64;

/// Local ingress and persistence errors. Supplied file names and contents are never included.
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    /// This platform has no supported implementation for opening verifier inputs.
    #[error("receipt verification input is unavailable on this platform")]
    UnsupportedPlatform,
    /// A required file is absent, inaccessible, unsafe, or exceeds the profile limit.
    #[error("could not snapshot the {0} as a bounded regular file")]
    Input(&'static str),
    /// The request fails the typed execution contract or is not its exact canonical encoding.
    #[error("invalid canonical execution request")]
    Request,
    /// The independently supplied native context does not satisfy the profile.
    #[error("invalid original Build context")]
    Context,
    /// Persistence errors use the store's payload-free operator diagnostics.
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(try_from = "String", rename_all = "snake_case")]
enum JournalOutcome {
    Terminal,
    Ambiguous,
    Missing,
}

impl TryFrom<String> for JournalOutcome {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "terminal" => Ok(Self::Terminal),
            "ambiguous" => Ok(Self::Ambiguous),
            "missing" => Ok(Self::Missing),
            _ => Err("unknown native outcome"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeContext {
    task_id: BuildReference,
    #[serde(default, deserialize_with = "BuildReference::present")]
    action_id: Option<BuildReference>,
    #[serde(default, deserialize_with = "BuildReference::present")]
    idempotency_key: Option<BuildReference>,
    outcome: JournalOutcome,
}

struct Registration {
    request: ExecutionCorrelation,
    context: NativeContext,
}

impl Registration {
    fn validate(input: &VerificationInput) -> Result<Self, VerificationError> {
        let request: ExecutionRequestInput =
            serde_json::from_slice(&input.request).map_err(|_| VerificationError::Request)?;
        if canonical_bytes(&request).map_err(|_| VerificationError::Request)? != input.request {
            return Err(VerificationError::Request);
        }
        let request = AdoptionRequest::new(request).map_err(|_| VerificationError::Request)?;
        let mut parser = serde_json::Deserializer::from_slice(&input.context);
        let context: NativeContext =
            deserialize_object(&mut parser).map_err(|_| VerificationError::Context)?;
        parser.end().map_err(|_| VerificationError::Context)?;
        if context.action_id.is_none() && context.idempotency_key.is_none() {
            return Err(VerificationError::Context);
        }
        Ok(Self {
            request: request.correlation(),
            context,
        })
    }

    fn report(self, summary: VerificationSummary) -> VerificationReport {
        VerificationReport {
            verification_id: summary.verification_id,
            request_id: self.request.request_id,
            request_digest: self.request.request_digest,
            task_id: self.context.task_id,
            action_id: self.context.action_id,
            idempotency_key: self.context.idempotency_key,
            native_outcome: self.context.outcome,
            result: summary.decision.result(),
            code: summary.decision.code(),
            redispatch: false,
            missing_evidence: summary.missing_evidence,
            observation_count: summary.observation_count,
            quarantine_count: summary.quarantine_count,
            original_receipt_digest: summary.original_receipt_digest,
            original_result: summary.original_decision.map(|decision| decision.result()),
            original_code: summary.original_decision.map(|decision| decision.code()),
            decision: summary.decision,
        }
    }
}

/// Bounded operator view. The latest observation and first authenticated original are reported independently.
#[derive(Debug, Serialize)]
pub struct VerificationReport {
    verification_id: String,
    request_id: RequestId,
    request_digest: Sha256Digest,
    task_id: BuildReference,
    action_id: Option<BuildReference>,
    idempotency_key: Option<BuildReference>,
    native_outcome: JournalOutcome,
    result: &'static str,
    code: &'static str,
    redispatch: bool,
    // The latest candidate alone determines this list. Empty native receipt bytes require
    // recovery. Present, malformed binding or signature bytes receive the verifier's refusal.
    missing_evidence: Vec<&'static str>,
    observation_count: u64,
    quarantine_count: u64,
    original_receipt_digest: Option<Sha256Digest>,
    original_result: Option<&'static str>,
    original_code: Option<&'static str>,
    #[serde(skip)]
    decision: VerificationDecision,
}

impl VerificationReport {
    /// Whether the effective decision verifies this binding.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self.decision, VerificationDecision::BindingVerified)
    }
}

fn store(config: &Config) -> Result<Store, StoreError> {
    Store::open(&config.data_dir.join("psyche.sqlite3"))
}

/// Register the original canonical request, original trusted native context, and independent issuer policy. Repeating identical registration reopens the same verification.
pub fn submit(
    config: &Config,
    request: &Path,
    context: &Path,
    allowed_signers: &Path,
) -> Result<VerificationReport, VerificationError> {
    let input = VerificationInput {
        request: snapshot::required(request, "request")?,
        context: snapshot::required(context, "context")?,
        allowed_signers: snapshot::required(allowed_signers, "issuer policy")?,
    };
    let registration = Registration::validate(&input)?;
    let summary = store(config)?.register_verification(&input)?;
    Ok(registration.report(summary))
}

/// Inspect a persisted verification without running the verifier or reading the original caller files.
pub fn inspect(config: &Config, id: &str) -> Result<VerificationReport, VerificationError> {
    let store = store(config)?;
    let input = store.verification_input(id)?;
    let registration = Registration::validate(&input)?;
    Ok(registration.report(store.inspect_verification(id)?))
}

/// Observe one candidate evidence set with the negotiated executable. Each optional path denotes one source file; omitted or absent files represent missing evidence.
pub fn supply(
    config: &Config,
    id: &str,
    verifier: &Path,
    receipt: Option<&Path>,
    binding: Option<&Path>,
    signature: Option<&Path>,
) -> Result<VerificationReport, VerificationError> {
    let mut store = store(config)?;
    let input = store.verification_input(id)?;
    let registration = Registration::validate(&input)?;
    let evidence = VerificationEvidence {
        receipt: snapshot::optional(receipt, "receipt")?,
        binding: snapshot::optional(binding, "binding")?,
        signature: snapshot::optional(signature, "signature")?,
    };
    let decision = verifier::verify(&config.data_dir, verifier, &input, &evidence)
        .unwrap_or(VerificationDecision::VerifierUnavailable);
    let summary = store.append_verification(id, &evidence, decision)?;
    Ok(registration.report(summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_REQUEST: &[u8] =
        include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json");
    const GOLDEN_CONTEXT: &[u8] =
        include_bytes!("../../../profiles/build-receipt-binding/golden/context.json");

    fn input() -> VerificationInput {
        VerificationInput {
            request: GOLDEN_REQUEST.into(),
            context: GOLDEN_CONTEXT.into(),
            allowed_signers: vec![],
        }
    }

    #[cfg(unix)]
    #[test]
    fn registration_survives_source_changes_and_unpinned_code_never_runs() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let data = root.join("state");
        let config = psyche_config::load_str(&format!(
            "schema_version = \"psyche.config.v1\"\ndata_dir = {}\n[coven]\nsocket = \"/run/coven.sock\"\nrequired_api_version = \"coven.daemon.v1\"",
            toml::Value::String(data.to_str().unwrap().to_owned())
        )).unwrap();
        let request = root.join("request");
        let context = root.join("context");
        let policy = root.join("policy");
        fs::write(&request, GOLDEN_REQUEST).unwrap();
        fs::write(&context, GOLDEN_CONTEXT).unwrap();
        fs::write(&policy, b"original independent test policy").unwrap();
        let submitted = submit(&config, &request, &context, &policy).unwrap();
        let id = &submitted.verification_id;
        assert_eq!(submitted.observation_count, 0);
        assert_eq!(submitted.code, "recovery_required");
        assert_eq!(
            submitted.missing_evidence,
            ["receipt", "binding", "signature"]
        );

        fs::write(&policy, b"replaced").unwrap();
        assert!(matches!(
            submit(&config, &request, &context, &policy),
            Err(VerificationError::Store(
                StoreError::VerificationRegistrationConflict
            ))
        ));
        fs::remove_file(request).unwrap();
        fs::remove_file(context).unwrap();
        fs::remove_file(policy).unwrap();
        let forged = root.join("fake-verifier");
        fs::write(&forged, b"#!/bin/sh\n: > ../unexpected-execution\necho '{\"result\":\"verified\",\"code\":\"binding_verified\",\"redispatch\":false}'\n").unwrap();
        fs::set_permissions(&forged, fs::Permissions::from_mode(0o700)).unwrap();
        let receipt = root.join("receipt");
        fs::write(&receipt, b"private receipt bytes").unwrap();
        let refused = supply(&config, id, &forged, Some(&receipt), None, None).unwrap();
        fs::remove_file(receipt).unwrap();
        assert!(!data.join("unexpected-execution").exists());
        assert!(!refused.is_verified());
        assert_eq!(refused.code, "verifier_unavailable");
        assert_eq!(refused.observation_count, 1);
        assert_eq!(refused.missing_evidence, ["binding", "signature"]);
        let reopened = inspect(&config, id).unwrap();
        assert_eq!(
            serde_json::to_value(refused).unwrap(),
            serde_json::to_value(&reopened).unwrap()
        );
        assert!(reopened.original_receipt_digest.is_none());
        assert_eq!(reopened.missing_evidence, ["binding", "signature"]);
        let stored = store(&config).unwrap().verification_input(id).unwrap();
        assert_eq!(stored.allowed_signers, b"original independent test policy");
        assert_eq!(stored.request, GOLDEN_REQUEST);
        assert_eq!(stored.context, GOLDEN_CONTEXT);
    }

    #[test]
    fn admission_checks_canonical_bytes_and_execution_request_invariants() {
        let mut input = input();
        assert!(Registration::validate(&input).is_ok());
        input.request.push(b'\n');
        assert!(matches!(
            Registration::validate(&input),
            Err(VerificationError::Request)
        ));
        let mut request: serde_json::Value = serde_json::from_slice(GOLDEN_REQUEST).unwrap();
        request["cwd"] = "/outside-the-project".into();
        input.request = canonical_bytes(&request).unwrap();
        assert!(matches!(
            Registration::validate(&input),
            Err(VerificationError::Request)
        ));
    }

    #[test]
    fn admission_rejects_untrusted_shapes_and_widened_native_context() {
        let mut input = input();
        for context in [
            r#"{"task_id":"task","outcome":"terminal"}"#,
            r#"{"task_id":"task","action_id":null,"idempotency_key":"k","outcome":"terminal"}"#,
            r#"{"task_id":"task","action_id":"a","outcome":"success"}"#,
            r#"{"task_id":"task","action_id":"a","outcome":{"terminal":null}}"#,
            r#"{"task_id":"task","action_id":"a","outcome":"terminal","authority":true}"#,
            r#"{"task_id":"task","action_id":"a","outcome":"terminal","outcome":"ambiguous"}"#,
            r#"{"task_id":"task","action_id":"a","outcome":"terminal"} []"#,
            r#"["task","action","key","terminal"]"#,
        ] {
            input.context = context.into();
            assert!(matches!(
                Registration::validate(&input),
                Err(VerificationError::Context)
            ));
        }
        for outcome in ["terminal", "ambiguous", "missing"] {
            input.context = serde_json::to_vec(&serde_json::json!({
                "task_id":"task", "idempotency_key":"k", "outcome":outcome
            }))
            .unwrap();
            assert!(Registration::validate(&input).is_ok());
        }
    }
}
