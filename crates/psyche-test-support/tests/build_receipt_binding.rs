//! Black-box qualification of the offline binding verifier with fresh, disposable SSH keys.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use psyche_core::contracts::build_receipt::{BuildReceiptBinding, NativeOutcome};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const ISSUER: &str = "psyche-receipt-issuer";
const NAMESPACE: &str = "psyche.build-receipt-binding.v1";
const REQUEST: &[u8] =
    include_bytes!("../../psyche-coven/tests/fixtures/execution-request-launch.json");
const BINDING: &[u8] =
    include_bytes!("../../../profiles/build-receipt-binding/golden/binding.json");
const CONTEXT: &[u8] =
    include_bytes!("../../../profiles/build-receipt-binding/golden/context.json");
const RECEIPT: &[u8] =
    include_bytes!("../../../profiles/build-receipt-binding/golden/receipt.json");
const FILES: [&str; 6] = [
    "allowed_signers",
    "context",
    "request",
    "receipt",
    "binding",
    "binding.sig",
];

struct Fixture(TempDir);

impl Fixture {
    fn new() -> Self {
        let fixture = Self(tempdir().unwrap());
        fixture.keygen(&[
            "-q", "-t", "ed25519", "-C", "fixture", "-N", "", "-f", "issuer",
        ]);
        fixture.trust(ISSUER, "issuer.pub");
        fixture.write("request", REQUEST);
        fixture.write("context", CONTEXT);
        fixture.write("receipt", RECEIPT);
        fixture.sign(BINDING, NAMESPACE);
        fixture
    }

    fn dir(&self) -> &Path {
        self.0.path()
    }

    fn keygen(&self, args: &[&str]) {
        let result = Command::new("ssh-keygen")
            .args(args)
            .current_dir(self.dir())
            .env_remove("SSH_AUTH_SOCK")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "ssh-keygen: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn trust(&self, principal: &str, public_key: &str) {
        let public_key = fs::read_to_string(self.dir().join(public_key)).unwrap();
        self.write(
            "allowed_signers",
            format!("{principal} {public_key}").as_bytes(),
        );
    }

    fn write(&self, filename: &str, bytes: &[u8]) {
        fs::write(self.dir().join(filename), bytes).unwrap();
    }

    fn sign(&self, bytes: &[u8], namespace: &str) {
        self.write("binding", bytes);
        let signature = self.dir().join("binding.sig");
        if signature.exists() {
            fs::remove_file(signature).unwrap();
        }
        self.keygen(&[
            "-q", "-Y", "sign", "-n", namespace, "-f", "issuer", "binding",
        ]);
    }

    fn verify(&self, result: &str, code: &str) {
        self.verify_with_env(result, code, &[]);
    }

    fn verify_with_env(&self, result: &str, code: &str, env: &[(&str, &Path)]) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_verify-build-binding"))
            .args(FILES)
            .envs(env.iter().copied())
            .current_dir(self.dir())
            .env_remove("SSH_AUTH_SOCK")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("verifier blocked on local fixture files");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let run = child.wait_with_output().unwrap();
        let output: Value = serde_json::from_slice(&run.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid verifier JSON: {error}; stderr: {}",
                String::from_utf8_lossy(&run.stderr)
            )
        });
        assert_eq!(
            output,
            json!({"result":result, "code":code, "redispatch":false})
        );
        assert_eq!(run.status.success(), result == "verified");
    }
}

fn object(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn changed(bytes: &[u8], key: &str, value: Value) -> Vec<u8> {
    let mut object = object(bytes);
    object[key] = value;
    canonical_bytes(&object).unwrap()
}

#[test]
fn core_decoder_enforces_required_references_issuer_and_version() {
    let record: BuildReceiptBinding = serde_json::from_slice(BINDING).unwrap();
    assert_eq!(canonical_bytes(&record).unwrap(), BINDING);
    let mut neither = object(BINDING);
    for field in ["action_id", "idempotency_key"] {
        neither.as_object_mut().unwrap().remove(field);
    }
    for bytes in [
        canonical_bytes(&neither).unwrap(),
        changed(BINDING, "issuer", json!("another-issuer")),
        changed(
            BINDING,
            "schema_version",
            json!("psyche.build_receipt_binding.v2"),
        ),
    ] {
        assert!(serde_json::from_slice::<BuildReceiptBinding>(&bytes).is_err());
    }
}

#[test]
fn binding_and_context_require_named_fields_even_if_array_values_match() {
    let fixture = Fixture::new();
    let binding = object(BINDING);
    let positional = json!([
        binding["schema_version"],
        binding["issuer"],
        binding["request_id"],
        binding["request_digest"],
        binding["task_id"],
        binding["action_id"],
        binding["idempotency_key"],
        binding["receipt_digest"],
        binding["outcome"]
    ]);
    let bytes = canonical_bytes(&positional).unwrap();
    assert!(serde_json::from_slice::<BuildReceiptBinding>(&bytes).is_err());
    fixture.sign(&bytes, NAMESPACE);
    fixture.verify("denied", "invalid_binding");

    let context = object(CONTEXT);
    let positional = json!([
        context["task_id"],
        context["action_id"],
        context["idempotency_key"],
        context["outcome"]
    ]);
    fixture.sign(BINDING, NAMESPACE);
    fixture.write("context", &canonical_bytes(&positional).unwrap());
    fixture.verify("denied", "invalid_context");
}

#[test]
fn outcomes_accept_only_the_string_literals_defined_for_each_record() {
    let fixture = Fixture::new();
    for (outcome, context_result, context_code) in [
        (json!({"terminal": null}), "denied", "invalid_context"),
        (json!({"ambiguous": null}), "denied", "invalid_context"),
        (json!({"missing": null}), "denied", "invalid_context"),
        (json!(null), "denied", "invalid_context"),
        (json!(false), "denied", "invalid_context"),
        (json!(0), "denied", "invalid_context"),
        (json!(["terminal"]), "denied", "invalid_context"),
        (json!("missing"), "unknown", "recovery_required"),
    ] {
        let binding = changed(BINDING, "outcome", outcome.clone());
        assert!(
            serde_json::from_value::<NativeOutcome>(outcome.clone()).is_err(),
            "{outcome}"
        );
        assert!(
            serde_json::from_slice::<BuildReceiptBinding>(&binding).is_err(),
            "{outcome}"
        );
        fixture.sign(&binding, NAMESPACE);
        fixture.write("context", CONTEXT);
        fixture.verify("denied", "invalid_binding");

        fixture.sign(BINDING, NAMESPACE);
        fixture.write("context", &changed(CONTEXT, "outcome", outcome));
        fixture.verify(context_result, context_code);
    }
}

#[test]
fn valid_signature_reports_verifier_unavailable_when_local_verification_cannot_run() {
    let fixture = Fixture::new();
    let file = fixture.dir().join("receipt");
    fixture.verify_with_env(
        "denied",
        "verifier_unavailable",
        &[("TMPDIR", &file), ("TMP", &file), ("TEMP", &file)],
    );

    let empty_bin = fixture.dir().join("empty-bin");
    fs::create_dir(&empty_bin).unwrap();
    fixture.verify_with_env("denied", "verifier_unavailable", &[("PATH", &empty_bin)]);
    fixture.verify("verified", "binding_verified");
}

#[cfg(unix)]
#[test]
fn reference_verifier_refuses_a_receipt_fifo_without_a_writer() {
    let fixture = Fixture::new();
    let path = fixture.dir().join("receipt");
    fs::remove_file(&path).unwrap();
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
    fixture.verify("denied", "invalid_receipt");
}

#[test]
fn native_receipt_size_limit_accepts_exactly_65536_bytes() {
    let fixture = Fixture::new();
    let mut receipt = vec![b'x'; 65_536];
    let digest = Sha256Digest::from_raw_bytes(&receipt);
    fixture.write("receipt", &receipt);
    fixture.sign(
        &changed(BINDING, "receipt_digest", json!(digest)),
        NAMESPACE,
    );
    fixture.verify("verified", "binding_verified");
    receipt.push(b'x');
    fixture.write("receipt", &receipt);
    fixture.verify("denied", "invalid_receipt");
}

#[test]
fn signed_golden_verifies_again_after_restart_without_changing_evidence() {
    let fixture = Fixture::new();
    for _ in 0..2 {
        fixture.verify("verified", "binding_verified");
        assert_eq!(fs::read(fixture.dir().join("binding")).unwrap(), BINDING);
        assert_eq!(fs::read(fixture.dir().join("receipt")).unwrap(), RECEIPT);
    }
}

#[test]
fn only_the_configured_issuer_key_principal_and_signature_namespace_verify() {
    let fixture = Fixture::new();
    fixture.keygen(&[
        "-q", "-t", "ed25519", "-C", "other", "-N", "", "-f", "other",
    ]);
    fixture.trust(ISSUER, "other.pub");
    fixture.verify("denied", "untrusted_issuer");
    fixture.trust("other-principal", "issuer.pub");
    fixture.verify("denied", "untrusted_issuer");
    fixture.trust(ISSUER, "issuer.pub");
    fixture.sign(BINDING, "psyche.unrelated.v1");
    fixture.verify("denied", "untrusted_issuer");
    fixture.sign(
        &changed(BINDING, "issuer", json!("other-principal")),
        NAMESPACE,
    );
    fixture.verify("denied", "untrusted_issuer");

    fixture.sign(BINDING, NAMESPACE);
    fixture.write(
        "binding",
        &changed(BINDING, "action_id", json!("new-action")),
    );
    fixture.verify("denied", "untrusted_issuer");
    fixture.sign(BINDING, NAMESPACE);
    fs::remove_file(fixture.dir().join("binding.sig")).unwrap();
    fixture.verify("unknown", "recovery_required");
    fixture.write("binding.sig", b"malformed signature");
    fixture.verify("denied", "untrusted_issuer");
    fixture.sign(BINDING, NAMESPACE);
    fs::remove_file(fixture.dir().join("allowed_signers")).unwrap();
    fixture.verify("denied", "untrusted_issuer");
}

#[test]
fn signed_conflicts_quarantine_without_overwriting_either_side() {
    let fixture = Fixture::new();
    let wrong_digest = format!("sha256:{}", "0".repeat(64));
    for (field, value) in [
        ("request_id", "req_01J00000000000000000000001"),
        ("request_digest", wrong_digest.as_str()),
        ("receipt_digest", wrong_digest.as_str()),
        ("task_id", "other-task"),
        ("action_id", "other-action"),
        ("idempotency_key", "other-key"),
    ] {
        let signed = changed(BINDING, field, json!(value));
        fixture.sign(&signed, NAMESPACE);
        fixture.verify("quarantine", "conflicting_evidence");
        assert_eq!(fs::read(fixture.dir().join("binding")).unwrap(), signed);
        assert_eq!(fs::read(fixture.dir().join("receipt")).unwrap(), RECEIPT);
    }

    fixture.sign(BINDING, NAMESPACE);
    fixture.write(
        "request",
        &changed(REQUEST, "payload_digest", json!(wrong_digest)),
    );
    fixture.verify("quarantine", "conflicting_evidence");
    fixture.write("request", REQUEST);
    let mut edited = RECEIPT.to_vec();
    edited.push(b' ');
    fixture.write("receipt", &edited);
    fixture.verify("quarantine", "conflicting_evidence");
    fixture.write("context", &changed(CONTEXT, "outcome", json!("ambiguous")));
    fixture.verify("quarantine", "conflicting_evidence");
}

#[test]
fn both_optional_reference_shapes_match_the_independent_native_context_exactly() {
    let fixture = Fixture::new();
    for field in ["action_id", "idempotency_key"] {
        let mut binding = object(BINDING);
        binding.as_object_mut().unwrap().remove(field);
        fixture.sign(&canonical_bytes(&binding).unwrap(), NAMESPACE);
        fixture.write("context", CONTEXT);
        fixture.verify("quarantine", "conflicting_evidence");

        let mut context = object(CONTEXT);
        context.as_object_mut().unwrap().remove(field);
        fixture.write("context", &canonical_bytes(&context).unwrap());
        fixture.verify("verified", "binding_verified");
        fixture.sign(BINDING, NAMESPACE);
        fixture.verify("quarantine", "conflicting_evidence");
    }
}

#[test]
fn ambiguous_and_missing_evidence_never_become_verified_terminal_receipts() {
    let fixture = Fixture::new();
    fixture.write("context", &changed(CONTEXT, "outcome", json!("missing")));
    fixture.verify("unknown", "recovery_required");
    fixture.write("context", &changed(CONTEXT, "outcome", json!("ambiguous")));
    fixture.verify("unknown", "reconciliation_required");
    fixture.sign(&changed(BINDING, "outcome", json!("ambiguous")), NAMESPACE);
    fixture.write("context", CONTEXT);
    fixture.verify("unknown", "reconciliation_required");

    fixture.sign(BINDING, NAMESPACE);
    fixture.write("receipt", &[]);
    fixture.verify("unknown", "recovery_required");
    for (filename, bytes) in [
        ("receipt", RECEIPT),
        ("binding", BINDING),
        ("request", REQUEST),
        ("context", CONTEXT),
    ] {
        fs::remove_file(fixture.dir().join(filename)).unwrap();
        fixture.verify("unknown", "recovery_required");
        fixture.write(filename, bytes);
    }
}

#[test]
fn malformed_signed_records_and_unrecognized_versions_fail_closed() {
    let fixture = Fixture::new();
    let mut neither = object(BINDING);
    for key in ["idempotency_key", "action_id"] {
        neither.as_object_mut().unwrap().remove(key);
    }
    let mut padded = BINDING.to_vec();
    padded.push(b'\n');
    let duplicate = format!(
        "{{\"task_id\":\"fixture-task\",{}",
        &std::str::from_utf8(BINDING).unwrap()[1..]
    );
    for bytes in [
        duplicate.into_bytes(),
        padded,
        canonical_bytes(&neither).unwrap(),
        changed(BINDING, "unexpected", json!(false)),
        changed(BINDING, "outcome", json!("unrecognized")),
        changed(BINDING, "action_id", Value::Null),
        changed(BINDING, "idempotency_key", json!("contains space")),
        changed(BINDING, "task_id", json!("a".repeat(256))),
    ] {
        fixture.sign(&bytes, NAMESPACE);
        fixture.verify("denied", "invalid_binding");
    }
    fixture.sign(
        &changed(
            BINDING,
            "schema_version",
            json!("psyche.build_receipt_binding.v2"),
        ),
        NAMESPACE,
    );
    fixture.verify("denied", "unsupported_version");

    fixture.sign(BINDING, NAMESPACE);
    fixture.write(
        "request",
        &changed(REQUEST, "harness", json!("unrecognized")),
    );
    fixture.verify("denied", "invalid_request");
    fixture.write("request", REQUEST);
    for bytes in [
        changed(CONTEXT, "outcome", json!("unrecognized")),
        changed(CONTEXT, "action_id", Value::Null),
        changed(CONTEXT, "extra", json!(false)),
    ] {
        fixture.write("context", &bytes);
        fixture.verify("denied", "invalid_context");
    }
}
