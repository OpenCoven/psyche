//! Offline conformance runner for the experimental first Build profile kit.
//!
//! The runner checks an extracted kit directory against its own manifest:
//! every member's SHA-256, the canonical bytes and digest of every selected
//! kind's vector, and every denial and canonicalization case. It never opens a
//! network connection, reads no credential and grants no execution authority.
//! The caller authenticates the archive against an independently recorded
//! SHA-256 before extracting it; the manifest is trusted only through that pin.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use psyche_core::contracts::{
    ContractError, RejectedDocument, RejectionReason, SchemaKind, SchemaVersion, decode_document,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use serde::Deserialize;
use serde_json::{Value, json};

/// The only profile format this runner accepts.
const FORMAT: &str = "psyche.experimental.first-build-profile.v1";
const FORMAT_FAMILY: &str = "psyche.experimental.first-build-profile.v";
const DENIALS_FORMAT: &str = "psyche.experimental.first-build-profile.denials.v1";
const STATUS: &str = "experimental-prerelease";
const MANIFEST: &str = "manifest.json";
const DENIALS: &str = "denials.json";
const DOCUMENT_LIMIT: u64 = 1_048_576;
const MEMBER_LIMIT: u64 = 33_554_432;
const MAX_MEMBERS: usize = 256;
const SUBJECT_LIMIT: usize = 96;

/// `SchemaKind::ALL` is private to `psyche-core`. The match below has no
/// wildcard arm, so a registry variant missing from this list fails to compile.
const REGISTRY: &[SchemaKind] = &[
    SchemaKind::IdentitySnapshot,
    SchemaKind::Intent,
    SchemaKind::SurfaceEvent,
    SchemaKind::Graph,
    SchemaKind::GraphNode,
    SchemaKind::Delegation,
    SchemaKind::Budget,
    SchemaKind::Approval,
    SchemaKind::ExecutionBinding,
    SchemaKind::Evidence,
    SchemaKind::Verdict,
    SchemaKind::Recovery,
    SchemaKind::Addon,
    SchemaKind::SurfaceEffect,
    SchemaKind::Delivery,
    SchemaKind::Error,
];

const fn registered(kind: SchemaKind) -> bool {
    match kind {
        SchemaKind::IdentitySnapshot
        | SchemaKind::Intent
        | SchemaKind::SurfaceEvent
        | SchemaKind::Graph
        | SchemaKind::GraphNode
        | SchemaKind::Delegation
        | SchemaKind::Budget
        | SchemaKind::Approval
        | SchemaKind::ExecutionBinding
        | SchemaKind::Evidence
        | SchemaKind::Verdict
        | SchemaKind::Recovery
        | SchemaKind::Addon
        | SchemaKind::SurfaceEffect
        | SchemaKind::Delivery
        | SchemaKind::Error => true,
    }
}

/// A bounded refusal. The subject is a member path or case name, filtered to a
/// safe character set and truncated, never raw input bytes.
struct Refusal {
    code: &'static str,
    subject: Option<String>,
}

fn refuse(code: &'static str) -> Refusal {
    Refusal {
        code,
        subject: None,
    }
}

fn refuse_at(code: &'static str, subject: &str) -> Refusal {
    let safe: String = subject
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
        .take(SUBJECT_LIMIT)
        .collect();
    Refusal {
        code,
        subject: Some(safe),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    status: String,
    repository: String,
    source_commit: String,
    target: String,
    rustc: String,
    cargo: String,
    rust_channel: String,
    build_profile: String,
    cargo_locked: bool,
    cargo_lock_sha256: String,
    kinds: BTreeMap<String, KindEntry>,
    excluded_kinds: BTreeMap<String, String>,
    components: BTreeMap<String, Value>,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KindEntry {
    proposed_class: ProposedClass,
    vector: String,
    sha256: String,
}

#[derive(Deserialize, Clone, Copy)]
enum ProposedClass {
    #[serde(rename = "stable-v1-target")]
    StableTarget,
    #[serde(rename = "experimental")]
    Experimental,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Denials {
    format: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    input: String,
    expect: Value,
}

fn kind_name(kind: SchemaKind) -> String {
    let full = SchemaVersion { kind, major: 1 }.to_string();
    full.strip_prefix("psyche.")
        .and_then(|rest| rest.strip_suffix(".v1"))
        .unwrap_or_default()
        .to_owned()
}

fn kind_from_name(name: &str) -> Option<SchemaKind> {
    REGISTRY
        .iter()
        .copied()
        .find(|kind| registered(*kind) && kind_name(*kind) == name)
}

/// The profile-local wire form of a decode rejection. It serializes the same
/// payload-light classification the store would quarantine.
fn rejection(bytes: &[u8], error: ContractError) -> Value {
    match RejectedDocument::from_decode_error(bytes, error).reason {
        RejectionReason::TooLarge => json!({"result": "rejected", "code": "too_large"}),
        RejectionReason::UnknownSchema => json!({"result": "rejected", "code": "unknown_schema"}),
        RejectionReason::UnsupportedMajor { found, supported } => json!({
            "result": "rejected", "code": "unsupported_major", "found": found, "supported": supported,
        }),
        RejectionReason::UnknownEnumValue { schema, field } => json!({
            "result": "rejected", "code": "unknown_enum_value", "schema": kind_name(schema), "field": field,
        }),
        RejectionReason::InvalidShape { schema, field } => json!({
            "result": "rejected", "code": "invalid_shape", "schema": kind_name(schema), "field": field,
        }),
    }
}

fn read_bounded(path: &Path, limit: u64, subject: &str) -> Result<Vec<u8>, Refusal> {
    let metadata = fs::symlink_metadata(path).map_err(|_| refuse_at("missing_member", subject))?;
    if !metadata.file_type().is_file() {
        return Err(refuse_at("member_invalid", subject));
    }
    if metadata.len() > limit {
        return Err(refuse_at("member_too_large", subject));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| refuse_at("member_unreadable", subject))?;
    if bytes.len() as u64 > limit {
        return Err(refuse_at("member_too_large", subject));
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256Digest::from_raw_bytes(bytes);
    digest
        .as_str()
        .strip_prefix("sha256:")
        .unwrap_or_default()
        .to_owned()
}

fn valid_member_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

/// Lists every entry below `root` as a `/`-separated relative path. Links,
/// devices and other non-regular entries fail closed.
fn walk(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) -> Result<(), Refusal> {
    let entries = fs::read_dir(dir).map_err(|_| refuse_at("member_unreadable", prefix))?;
    for entry in entries {
        let entry = entry.map_err(|_| refuse_at("member_unreadable", prefix))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| refuse("unexpected_member"))?;
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry
            .file_type()
            .map_err(|_| refuse_at("member_unreadable", &relative))?;
        if file_type.is_dir() {
            walk(&entry.path(), &relative, out)?;
        } else if file_type.is_file() {
            out.insert(relative);
        } else {
            return Err(refuse_at("member_invalid", &relative));
        }
        if out.len() > MAX_MEMBERS {
            return Err(refuse("too_many_members"));
        }
    }
    Ok(())
}

fn check_format(format: &str) -> Result<(), Refusal> {
    if format == FORMAT {
        return Ok(());
    }
    // Any other major of this family, older or newer, is refused rather than
    // read with this runner's assumptions.
    match format.strip_prefix(FORMAT_FAMILY) {
        Some(major) if !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()) => {
            Err(refuse("unsupported_profile_version"))
        }
        _ => Err(refuse("unknown_profile")),
    }
}

struct Options {
    root: PathBuf,
    source: Option<String>,
    target: Option<String>,
}

fn parse_args() -> Result<Options, Refusal> {
    let mut args = env::args_os().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| refuse("usage"))?;
    let mut source = None;
    let mut target = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .and_then(|value| value.into_string().ok())
            .ok_or_else(|| refuse("usage"))?;
        match flag.to_str() {
            Some("--source") if source.is_none() => source = Some(value),
            Some("--target") if target.is_none() => target = Some(value),
            _ => return Err(refuse("usage")),
        }
    }
    Ok(Options {
        root,
        source,
        target,
    })
}

fn verify(options: &Options) -> Result<Value, Refusal> {
    let root = &options.root;
    let raw = read_bounded(&root.join(MANIFEST), DOCUMENT_LIMIT, MANIFEST)?;
    // Read the format alone first so an older or newer manifest is reported as
    // a version refusal and not as a shape error.
    let probe: Value = serde_json::from_slice(&raw).map_err(|_| refuse("manifest_invalid"))?;
    let format = probe
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| refuse("manifest_invalid"))?;
    check_format(format)?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|_| refuse("manifest_invalid"))?;
    if manifest.format != FORMAT
        || manifest.status != STATUS
        || manifest.repository != "OpenCoven/psyche"
        || manifest.build_profile != "release"
        || !manifest.cargo_locked
        || manifest.rustc.is_empty()
        || manifest.cargo.is_empty()
        || manifest.rust_channel.is_empty()
        || manifest.target.is_empty()
        || !is_hex(&manifest.source_commit, 40)
        || !is_hex(&manifest.cargo_lock_sha256, 64)
    {
        return Err(refuse("manifest_invalid"));
    }
    if options
        .source
        .as_ref()
        .is_some_and(|source| *source != manifest.source_commit)
        || options
            .target
            .as_ref()
            .is_some_and(|target| *target != manifest.target)
    {
        return Err(refuse("provenance_mismatch"));
    }

    // Members: exactly the manifest's files, plus the manifest itself.
    let mut present = BTreeSet::new();
    walk(root, "", &mut present)?;
    present.remove(MANIFEST);
    for name in &present {
        if !manifest.files.contains_key(name) {
            return Err(refuse_at("unexpected_member", name));
        }
    }
    for (name, expected) in &manifest.files {
        if !valid_member_name(name) || !is_hex(expected, 64) || name == MANIFEST {
            return Err(refuse_at("manifest_invalid", name));
        }
        if !present.contains(name) {
            return Err(refuse_at("missing_member", name));
        }
        let bytes = read_bounded(&root.join(name), MEMBER_LIMIT, name)?;
        if sha256_hex(&bytes) != *expected {
            return Err(refuse_at("member_digest_mismatch", name));
        }
    }

    // Every registry kind is either selected or explicitly excluded, once.
    let mut dispositioned = BTreeSet::new();
    for name in manifest.kinds.keys().chain(manifest.excluded_kinds.keys()) {
        let kind = kind_from_name(name).ok_or_else(|| refuse_at("unknown_kind", name))?;
        if !dispositioned.insert(kind_name(kind)) {
            return Err(refuse_at("kind_dispositioned_twice", name));
        }
    }
    if dispositioned.len() != REGISTRY.len() {
        return Err(refuse("registry_mismatch"));
    }

    let mut vectors = BTreeMap::new();
    let (mut stable_targets, mut experimental) = (0_u32, 0_u32);
    for (name, entry) in &manifest.kinds {
        if !manifest.files.contains_key(&entry.vector) {
            return Err(refuse_at("missing_member", &entry.vector));
        }
        let bytes = read_bounded(&root.join(&entry.vector), DOCUMENT_LIMIT, &entry.vector)?;
        let document = decode_document(&bytes).map_err(|_| refuse_at("vector_rejected", name))?;
        if kind_name(document.schema_version().kind) != *name {
            return Err(refuse_at("kind_mismatch", name));
        }
        let encoded = canonical_bytes(&document).map_err(|_| refuse_at("vector_rejected", name))?;
        if encoded != bytes {
            return Err(refuse_at("canonical_bytes_mismatch", name));
        }
        if sha256_hex(&bytes) != entry.sha256 {
            return Err(refuse_at("vector_digest_mismatch", name));
        }
        match entry.proposed_class {
            ProposedClass::StableTarget => stable_targets += 1,
            ProposedClass::Experimental => experimental += 1,
        }
        vectors.insert(name.clone(), bytes);
    }

    let raw = read_bounded(&root.join(DENIALS), DOCUMENT_LIMIT, DENIALS)?;
    let denials: Denials = serde_json::from_slice(&raw).map_err(|_| refuse("denials_invalid"))?;
    if denials.format != DENIALS_FORMAT || denials.cases.is_empty() {
        return Err(refuse("denials_invalid"));
    }
    let (mut rejected, mut canonicalized) = (0_u32, 0_u32);
    let mut names = BTreeSet::new();
    for case in &denials.cases {
        if !names.insert(case.name.as_str()) {
            return Err(refuse_at("denials_invalid", &case.name));
        }
        let input = case.input.as_bytes();
        match case.expect.get("result").and_then(Value::as_str) {
            Some("rejected") => match decode_document(input) {
                Ok(_) => return Err(refuse_at("denial_accepted", &case.name)),
                Err(error) => {
                    if rejection(input, error) != case.expect {
                        return Err(refuse_at("denial_reason_mismatch", &case.name));
                    }
                    rejected += 1;
                }
            },
            Some("canonicalizes") => {
                let vector = case
                    .expect
                    .get("vector")
                    .and_then(Value::as_str)
                    .and_then(|kind| vectors.get(kind))
                    .ok_or_else(|| refuse_at("denials_invalid", &case.name))?;
                let document = decode_document(input)
                    .map_err(|_| refuse_at("canonicalization_rejected", &case.name))?;
                let encoded = canonical_bytes(&document)
                    .map_err(|_| refuse_at("canonicalization_rejected", &case.name))?;
                if encoded != *vector || input == vector.as_slice() {
                    return Err(refuse_at("canonicalization_mismatch", &case.name));
                }
                canonicalized += 1;
            }
            _ => return Err(refuse_at("denials_invalid", &case.name)),
        }
    }

    Ok(json!({
        "result": "passed",
        "code": "profile_verified",
        "profile": FORMAT,
        "source_commit": manifest.source_commit,
        "target": manifest.target,
        "members_verified": manifest.files.len(),
        "vectors_verified": vectors.len(),
        "proposed_stable_targets": stable_targets,
        "experimental_kinds": experimental,
        "excluded_kinds": manifest.excluded_kinds.len(),
        "components": manifest.components.len(),
        "denials_verified": rejected,
        "canonicalizations_verified": canonicalized,
        "redispatch": false,
    }))
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn main() -> ExitCode {
    let outcome = parse_args().and_then(|options| verify(&options));
    let (report, code) = match outcome {
        Ok(report) => (report, ExitCode::SUCCESS),
        Err(refusal) => {
            let mut report = json!({"result": "denied", "code": refusal.code, "redispatch": false});
            if let (Some(subject), Some(object)) = (refusal.subject, report.as_object_mut()) {
                object.insert("subject".to_owned(), Value::String(subject));
            }
            (report, ExitCode::FAILURE)
        }
    };
    println!("{report}");
    code
}
