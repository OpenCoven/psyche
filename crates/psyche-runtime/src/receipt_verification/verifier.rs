//! The pinned verifier receives only private snapshots and produces one closed decision.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use psyche_core::contracts::build_receipt::deserialize_object;
use psyche_core::digest::Sha256Digest;
use psyche_store::receipt_verification::{
    VerificationDecision as Decision, VerificationEvidence, VerificationInput,
};
use serde::Deserialize;

use super::snapshot;

// SHA-256 of bin/verify-build-binding from each independently pinned #606 archive.
// Producer 557c579ed19f97e4177d4a6760e183eab1df11b2, run 45585485992.
fn host_pin() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("sha256:2437b1f15a9bd4d581f819288cb26fca9dee846351078278922dee201e8d0f65")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Some("sha256:be7e92487e6ed82c927f93f49fbe38e0074da72a8dd13fd79f7193f0c2cbb9a7")
    } else {
        None
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionWire {
    result: String,
    code: String,
    redispatch: bool,
}

fn decode(stdout: &[u8], exit: Option<i32>) -> Option<Decision> {
    let mut parser = serde_json::Deserializer::from_slice(stdout);
    let wire: DecisionWire = deserialize_object(&mut parser).ok()?;
    parser.end().ok()?;
    if wire.redispatch {
        return None;
    }
    match (exit?, wire.result.as_str(), wire.code.as_str()) {
        (0, "verified", "binding_verified") => Some(Decision::BindingVerified),
        (1, "unknown", "recovery_required") => Some(Decision::RecoveryRequired),
        (1, "unknown", "reconciliation_required") => Some(Decision::ReconciliationRequired),
        (1, "quarantine", "conflicting_evidence") => Some(Decision::ConflictingEvidence),
        (1, "denied", "invalid_context") => Some(Decision::InvalidContext),
        (1, "denied", "invalid_binding") => Some(Decision::InvalidBinding),
        (1, "denied", "invalid_request") => Some(Decision::InvalidRequest),
        (1, "denied", "invalid_receipt") => Some(Decision::InvalidReceipt),
        (1, "denied", "unsupported_version") => Some(Decision::UnsupportedVersion),
        (1, "denied", "untrusted_issuer") => Some(Decision::UntrustedIssuer),
        (1, "denied", "verifier_unavailable") => Some(Decision::VerifierUnavailable),
        _ => None,
    }
}

pub(super) fn verify(
    data_dir: &Path,
    executable: &Path,
    input: &VerificationInput,
    evidence: &VerificationEvidence,
) -> Option<Decision> {
    let pin = host_pin()?;
    let executable = snapshot::read(executable, 1_048_576).ok()?;
    if Sha256Digest::from_raw_bytes(&executable).as_str() != pin {
        return None;
    }
    run(data_dir, &executable, input, evidence).ok()
}

fn run(
    data_dir: &Path,
    executable: &[u8],
    input: &VerificationInput,
    evidence: &VerificationEvidence,
) -> io::Result<Decision> {
    let mut directory = tempfile::Builder::new();
    directory.prefix("receipt-verification-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.permissions(fs::Permissions::from_mode(0o700));
    }
    let directory = directory.tempdir_in(data_dir)?;
    let root = fs::canonicalize(directory.path())?;
    let binary = root.join("verify-build-binding");
    write_snapshot(&binary, executable, true)?;
    let files = [
        ("policy", Some(&input.allowed_signers)),
        ("context", Some(&input.context)),
        ("request", Some(&input.request)),
        ("receipt", evidence.receipt.as_ref()),
        ("binding", evidence.binding.as_ref()),
        ("signature", evidence.signature.as_ref()),
    ];
    for (name, bytes) in files {
        if let Some(bytes) = bytes {
            write_snapshot(&root.join(name), bytes, false)?;
        }
    }
    // The issuer policy is verified by the host's system OpenSSH. Never inherit a caller's
    // PATH, dynamic loader settings, ssh-agent access, or working directory.
    let output_path = root.join("decision");
    let mut child = Command::new(binary)
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", &root)
        .args(files.map(|(name, _)| name))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(File::create_new(&output_path)?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other("verifier did not complete"));
            }
        }
    };
    const OUTPUT_LIMIT: u64 = 256;
    let mut output = Vec::new();
    File::open(output_path)?
        .take(OUTPUT_LIMIT + 1)
        .read_to_end(&mut output)?;
    if output.len() as u64 > OUTPUT_LIMIT {
        return Err(io::Error::other("oversize verifier decision"));
    }
    decode(&output, status.code()).ok_or_else(|| io::Error::other("invalid verifier decision"))
}

/// The creating descriptor is the only writer; close it before spawning the verifier.
fn write_snapshot(path: &Path, bytes: &[u8], executable: bool) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o500 } else { 0o400 });
    }
    #[cfg(not(unix))]
    let _ = executable;
    options.open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::{Decision, decode};

    #[test]
    fn accepts_only_known_named_decisions_with_matching_exit_and_no_dispatch() {
        for decision in [
            Decision::BindingVerified,
            Decision::RecoveryRequired,
            Decision::ReconciliationRequired,
            Decision::ConflictingEvidence,
            Decision::InvalidContext,
            Decision::InvalidBinding,
            Decision::InvalidRequest,
            Decision::InvalidReceipt,
            Decision::UnsupportedVersion,
            Decision::UntrustedIssuer,
            Decision::VerifierUnavailable,
        ] {
            let mut fields = serde_json::json!({
                "result":decision.result(), "code":decision.code(), "redispatch":false,
            });
            let wire = fields.to_string();
            let exit = i32::from(decision != Decision::BindingVerified);
            assert_eq!(decode(wire.as_bytes(), Some(exit)), Some(decision));
            assert_eq!(decode(wire.as_bytes(), Some(1 - exit)), None);
            assert_eq!(decode(wire.as_bytes(), Some(2)), None);
            assert_eq!(decode(wire.as_bytes(), None), None);
            fields["redispatch"] = true.into();
            assert_eq!(decode(fields.to_string().as_bytes(), Some(exit)), None);
        }
        for wire in [
            r#"["verified","binding_verified",false]"#,
            r#"{"result":"verified","code":"unknown_code","redispatch":false}"#,
            r#"{"result":"verified","code":"binding_verified","redispatch":false,"extra":1}"#,
            r#"{"result":"verified","code":"binding_verified","redispatch":false,"redispatch":false}"#,
            r#"{"result":"verified","code":"binding_verified","redispatch":false} {}"#,
            r#"{"result":"verified","code":"binding_verified"}"#,
            r#"{"result":"denied","code":"binding_verified","redispatch":false}"#,
        ] {
            assert_eq!(decode(wire.as_bytes(), Some(0)), None);
        }
    }
}
