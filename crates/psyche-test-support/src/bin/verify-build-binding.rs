//! Offline first-profile reference verifier for the ADR 4 receipt binding.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use psyche_core::contracts::build_receipt::{
    BuildReceiptBinding, BuildReference, ISSUER, NativeOutcome, SCHEMA_VERSION, SIGNATURE_NAMESPACE,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use psyche_coven::{AdoptionRequest, ExecutionRequestInput};
use serde::Deserialize;
use serde_json::json;
use tempfile::NamedTempFile;

const LIMIT: u64 = 65_536;

#[derive(Clone, Copy)]
enum Refusal {
    Missing,
    Ambiguous,
    Conflict,
    Denied(&'static str),
}

impl Refusal {
    fn report(self) -> (&'static str, &'static str) {
        match self {
            Self::Missing => ("unknown", "recovery_required"),
            Self::Ambiguous => ("unknown", "reconciliation_required"),
            Self::Conflict => ("quarantine", "conflicting_evidence"),
            Self::Denied(code) => ("denied", code),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Terminal,
    Ambiguous,
    Missing,
}

/// Supplied by the trusted native reader, independently of the signed file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    task_id: BuildReference,
    #[serde(default, deserialize_with = "BuildReference::present")]
    action_id: Option<BuildReference>,
    #[serde(default, deserialize_with = "BuildReference::present")]
    idempotency_key: Option<BuildReference>,
    outcome: JournalState,
}

#[cfg(unix)]
fn open_input(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};

    // A FIFO substituted for a regular file must not block us before fstat.
    Ok(open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

#[cfg(not(unix))]
fn open_input(path: &Path) -> io::Result<File> {
    File::open(path)
}

fn input(path: &Path, invalid: &'static str) -> Result<Vec<u8>, Refusal> {
    let fail = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => Refusal::Missing,
        _ => Refusal::Denied(invalid),
    };
    let file = open_input(path).map_err(fail)?;
    let meta = file.metadata().map_err(fail)?;
    if !meta.is_file() || meta.len() > LIMIT {
        return Err(Refusal::Denied(invalid));
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).map_err(fail)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Refusal::Denied(invalid));
    }
    Ok(bytes)
}

// Snapshot the bounded signature and policy that the caller supplied. The
// verifier process receives private regular files rather than reopening inputs.
fn snapshot(bytes: &[u8]) -> io::Result<NamedTempFile> {
    let mut temp = NamedTempFile::new()?;
    temp.write_all(bytes)?;
    temp.flush()?;
    Ok(temp)
}

fn authenticated(policy: &[u8], signature: &[u8], message: &[u8]) -> io::Result<bool> {
    let policy = snapshot(policy)?;
    let signature = snapshot(signature)?;
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(policy.path())
        .args(["-I", ISSUER, "-n", SIGNATURE_NAMESPACE, "-s"])
        .arg(signature.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let sent = child
        .stdin
        .take()
        .ok_or(io::Error::other("missing verifier stdin"))?
        .write_all(message);
    let status = child.wait()?;
    Ok(sent.is_ok() && status.success())
}

fn verify(args: &[PathBuf]) -> Result<(), Refusal> {
    let [
        policy_path,
        context_path,
        request_path,
        receipt_path,
        binding_path,
        sig_path,
    ] = args
    else {
        return Err(Refusal::Denied("invalid_arguments"));
    };
    let context: Context = serde_json::from_slice(&input(context_path, "invalid_context")?)
        .map_err(|_| Refusal::Denied("invalid_context"))?;
    if context.action_id.is_none() && context.idempotency_key.is_none() {
        return Err(Refusal::Denied("invalid_context"));
    }
    if matches!(context.outcome, JournalState::Missing) {
        return Err(Refusal::Missing);
    }
    let bytes = input(binding_path, "invalid_binding")?;
    // Negotiate the version before interpreting version-specific fields.
    #[derive(Deserialize)]
    struct Header {
        schema_version: String,
        #[serde(default)]
        issuer: serde_json::Value,
    }
    let header: Header =
        serde_json::from_slice(&bytes).map_err(|_| Refusal::Denied("invalid_binding"))?;
    if header.schema_version != SCHEMA_VERSION {
        return Err(Refusal::Denied("unsupported_version"));
    }
    let untrusted = Refusal::Denied("untrusted_issuer");
    if header.issuer.as_str().is_some_and(|s| s != ISSUER) {
        return Err(untrusted);
    }
    let binding: BuildReceiptBinding =
        serde_json::from_slice(&bytes).map_err(|_| Refusal::Denied("invalid_binding"))?;
    if canonical_bytes(&binding).map_err(|_| Refusal::Denied("invalid_binding"))? != bytes {
        return Err(Refusal::Denied("invalid_binding"));
    }
    let policy = input(policy_path, "untrusted_issuer").map_err(|_| untrusted)?;
    let signature = input(sig_path, "untrusted_issuer").map_err(|_| untrusted)?;
    if !authenticated(&policy, &signature, &bytes).map_err(|_| untrusted)? {
        return Err(untrusted);
    }

    let request_bytes = input(request_path, "invalid_request")?;
    let request: ExecutionRequestInput =
        serde_json::from_slice(&request_bytes).map_err(|_| Refusal::Denied("invalid_request"))?;
    if canonical_bytes(&request).map_err(|_| Refusal::Denied("invalid_request"))? != request_bytes {
        return Err(Refusal::Denied("invalid_request"));
    }
    let request = AdoptionRequest::new(request).map_err(|_| Refusal::Denied("invalid_request"))?;
    if request.correlation().request_id != binding.request_id
        || request.request_digest() != &binding.request_digest
        || context.task_id != binding.task_id
        || context.action_id != binding.action_id
        || context.idempotency_key != binding.idempotency_key
    {
        return Err(Refusal::Conflict);
    }
    let receipt = input(receipt_path, "invalid_receipt")?;
    if receipt.is_empty() {
        return Err(Refusal::Missing);
    }
    if Sha256Digest::from_raw_bytes(&receipt) != binding.receipt_digest {
        return Err(Refusal::Conflict);
    }
    if binding.outcome == NativeOutcome::Ambiguous
        || matches!(context.outcome, JournalState::Ambiguous)
    {
        return Err(Refusal::Ambiguous);
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    let (result, code, status) = match verify(&args) {
        Ok(()) => ("verified", "binding_verified", ExitCode::SUCCESS),
        Err(failure) => {
            let (result, code) = failure.report();
            (result, code, ExitCode::FAILURE)
        }
    };
    println!(
        "{}",
        json!({"result": result, "code": code, "redispatch": false})
    );
    status
}
