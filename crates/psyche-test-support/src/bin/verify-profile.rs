//! Offline conformance runner for the experimental first Build profile kit.
//!
//! The runner checks an extracted kit directory: its exact member set and
//! each member's SHA-256, the manifest's provenance and component pins, the
//! canonical bytes and digest of every selected kind's vector, and every
//! denial and canonicalization case. The selection, the vectors and the cases
//! are compiled into the runner, so a kit cannot narrow or replace them by
//! editing its own manifest. It never opens a network connection, reads no
//! credential and grants no execution authority. The caller authenticates the
//! archive against an independently recorded SHA-256 before extracting it.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use psyche_core::contracts::{
    ContractError, RejectedDocument, RejectionReason, SchemaKind, SchemaVersion, decode_document,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes};
use serde::Deserialize;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};

/// The only profile format this runner accepts.
const FORMAT: &str = "psyche.experimental.first-build-profile.v1";
const FORMAT_FAMILY: &str = "psyche.experimental.first-build-profile.v";
const DENIALS_FORMAT: &str = "psyche.experimental.first-build-profile.denials.v1";
const STATUS: &str = "experimental-prerelease";
const BINDING_SCHEMA: &str = "psyche.build_receipt_binding.v1";
const REPLAY_PROFILE: &str = "psyche.experimental.receipt-replay.v1";
const MANIFEST: &str = "manifest.json";
const DENIALS: &str = "denials.json";
const DOCUMENT_LIMIT: u64 = 1_048_576;
const MEMBER_LIMIT: u64 = 33_554_432;
const MAX_MEMBERS: usize = 64;
const SUBJECT_LIMIT: usize = 96;
const TEXT_LIMIT: usize = 128;

const TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];

/// Members every kit carries besides the manifest, the two executables and
/// the selected vectors.
const FIXED_MEMBERS: &[&str] = &[
    "LICENSE",
    "README.md",
    DENIALS,
    "binding/binding.schema.json",
    "binding/golden/allowed_signers",
    "binding/golden/binding.json",
    "binding/golden/binding.sig",
    "binding/golden/context.json",
    "binding/golden/receipt.json",
    "binding/golden/request.json",
    "receipt-replay/receipt-replay.pyz",
    "receipt-replay/consumer/action.json",
    "receipt-replay/consumer/check_inputs.py",
    "receipt-replay/consumer/check_witness.py",
    "receipt-replay/consumer/qualify.py",
    "receipt-replay/consumer/recovery.py",
    "receipt-replay/consumer/request.json",
    "receipt-replay/consumer/submit.py",
];

/// Cases `make_denials.py` generates for every selected kind and once
/// globally. The runner requires each, so a kit cannot drop coverage.
const KIND_CASES: &[&str] = &[
    "unknown_major_v2",
    "downgrade_major_v0",
    "unknown_field",
    "reordered_keys",
    "whitespace",
];
const GLOBAL_CASES: &[&str] = &[
    "unknown_schema_kind",
    "malformed_major",
    "excluded_kind_surface_effect_v2",
    "missing_schema_version",
    "duplicate_key",
    "trailing_data",
    "truncated",
    "not_an_object",
    "unknown_enum_value",
    "unsafe_integer",
];

/// The publication-surface selection (docs/protocol/PUBLICATION-SURFACE.md)
/// and each selected vector, compiled in. Excluded kinds carry no vector.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    StableTarget,
    Experimental,
    Excluded,
}

macro_rules! vector {
    ($kind:literal) => {
        Some(include_bytes!(concat!(
            "../../../psyche-core/tests/fixtures/vectors/psyche.",
            $kind,
            ".v1.json"
        )) as &[u8])
    };
}

/// One arm per registry variant and no wildcard: a new kind fails to compile
/// until it is classified here.
fn selection(kind: SchemaKind) -> (Class, Option<&'static [u8]>) {
    match kind {
        SchemaKind::IdentitySnapshot => (Class::StableTarget, vector!("identity_snapshot")),
        SchemaKind::Intent => (Class::StableTarget, vector!("intent")),
        SchemaKind::SurfaceEvent => (Class::StableTarget, vector!("surface_event")),
        SchemaKind::Graph => (Class::Experimental, vector!("graph")),
        SchemaKind::GraphNode => (Class::Experimental, vector!("graph_node")),
        SchemaKind::ExecutionBinding => (Class::Experimental, vector!("execution_binding")),
        SchemaKind::Delegation => (Class::Experimental, vector!("delegation")),
        SchemaKind::Budget => (Class::Experimental, vector!("budget")),
        SchemaKind::Approval => (Class::Experimental, vector!("approval")),
        SchemaKind::Evidence => (Class::Experimental, vector!("evidence")),
        SchemaKind::Verdict => (Class::Experimental, vector!("verdict")),
        SchemaKind::Recovery => (Class::Experimental, vector!("recovery")),
        SchemaKind::Addon => (Class::Experimental, vector!("addon")),
        SchemaKind::Error => (Class::Experimental, vector!("error")),
        SchemaKind::SurfaceEffect | SchemaKind::Delivery => (Class::Excluded, None),
    }
}

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

/// The generated cases, compiled in. A Windows checkout may have converted
/// the working-tree file's line endings; the generator writes LF only and no
/// case contains a raw carriage return.
const EMBEDDED_DENIALS: &[u8] =
    include_bytes!("../../../../profiles/first-build-profile/denials.json");

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
    components: Components,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KindEntry {
    proposed_class: ProposedClass,
    vector: String,
    sha256: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
enum ProposedClass {
    #[serde(rename = "stable-v1-target")]
    StableTarget,
    #[serde(rename = "experimental")]
    Experimental,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Components {
    #[serde(rename = "build-receipt-binding")]
    binding: BindingComponent,
    #[serde(rename = "receipt-replay")]
    replay: ReplayComponent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingComponent {
    schema_version: String,
    request_sha256: String,
    receipt_sha256: String,
    binding_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayComponent {
    profile: String,
    archive_sha256: String,
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

/// Parses any JSON value and fails on a duplicate object key at any depth.
struct NoDuplicates;

impl<'de> Deserialize<'de> for NoDuplicates {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NoDuplicatesVisitor)
    }
}

struct NoDuplicatesVisitor;

impl<'de> Visitor<'de> for NoDuplicatesVisitor {
    type Value = NoDuplicates;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate keys")
    }

    fn visit_bool<E>(self, _: bool) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_i64<E>(self, _: i64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_u64<E>(self, _: u64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_f64<E>(self, _: f64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_str<E>(self, _: &str) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_unit<E>(self) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NoDuplicates, A::Error> {
        while seq.next_element::<NoDuplicates>()?.is_some() {}
        Ok(NoDuplicates)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NoDuplicates, A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate key"));
            }
            map.next_value_seed(NoDuplicatesSeed)?;
        }
        Ok(NoDuplicates)
    }
}

struct NoDuplicatesSeed;

impl<'de> DeserializeSeed<'de> for NoDuplicatesSeed {
    type Value = NoDuplicates;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<NoDuplicates, D::Error> {
        NoDuplicates::deserialize(deserializer)
    }
}

fn strict<'a, T: Deserialize<'a>>(bytes: &'a [u8], code: &'static str) -> Result<T, Refusal> {
    serde_json::from_slice::<NoDuplicates>(bytes).map_err(|_| refuse(code))?;
    serde_json::from_slice(bytes).map_err(|_| refuse(code))
}

fn kind_name(kind: SchemaKind) -> String {
    let full = SchemaVersion { kind, major: 1 }.to_string();
    full.strip_prefix("psyche.")
        .and_then(|rest| rest.strip_suffix(".v1"))
        .unwrap_or_default()
        .to_owned()
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

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= TEXT_LIMIT
        && value.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// Lists every regular file below `dir` as a `/`-separated relative path.
/// Links, devices, empty directories and non-UTF-8 names fail closed.
fn walk(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) -> Result<(), Refusal> {
    let entries = fs::read_dir(dir).map_err(|_| refuse_at("member_unreadable", prefix))?;
    let mut empty = true;
    for entry in entries {
        empty = false;
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
    if empty && !prefix.is_empty() {
        return Err(refuse_at("unexpected_member", prefix));
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
    source: String,
    target: String,
}

/// `verify-profile ROOT --source SHA --target TRIPLE`. Both pins are required:
/// the consumer states which producer and target it expects.
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
            Some("--source") if source.is_none() && is_hex(&value, 40) => source = Some(value),
            Some("--target") if target.is_none() && TARGETS.contains(&value.as_str()) => {
                target = Some(value);
            }
            _ => return Err(refuse("usage")),
        }
    }
    match (source, target) {
        (Some(source), Some(target)) => Ok(Options {
            root,
            source,
            target,
        }),
        _ => Err(refuse("usage")),
    }
}

fn executable(name: &str, target: &str) -> String {
    let suffix = if target.ends_with("-windows-msvc") {
        ".exe"
    } else {
        ""
    };
    format!("bin/{name}{suffix}")
}

fn verify(options: &Options) -> Result<Value, Refusal> {
    let root = &options.root;
    let raw = read_bounded(&root.join(MANIFEST), DOCUMENT_LIMIT, MANIFEST)?;
    // Read the format alone first so an older or newer manifest is reported as
    // a version refusal and not as a shape error.
    let probe: Value = strict(&raw, "manifest_invalid")?;
    let format = probe
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| refuse("manifest_invalid"))?;
    check_format(format)?;
    let manifest: Manifest = strict(&raw, "manifest_invalid")?;
    if manifest.format != FORMAT
        || manifest.status != STATUS
        || manifest.repository != "OpenCoven/psyche"
        || manifest.build_profile != "release"
        || !manifest.cargo_locked
        || !is_text(&manifest.rustc)
        || !is_text(&manifest.cargo)
        || !is_text(&manifest.rust_channel)
        || !TARGETS.contains(&manifest.target.as_str())
        || !is_hex(&manifest.source_commit, 40)
        || !is_hex(&manifest.cargo_lock_sha256, 64)
    {
        return Err(refuse("manifest_invalid"));
    }
    if options.source != manifest.source_commit || options.target != manifest.target {
        return Err(refuse("provenance_mismatch"));
    }

    // The selection is the runner's, not the manifest's.
    let mut expected_members: BTreeSet<String> = FIXED_MEMBERS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    for name in ["verify-profile", "verify-build-binding"] {
        expected_members.insert(executable(name, &manifest.target));
    }
    let mut selected = 0_usize;
    let (mut stable_targets, mut experimental) = (0_u32, 0_u32);
    for kind in REGISTRY {
        let name = kind_name(*kind);
        match (selection(*kind), manifest.kinds.get(&name)) {
            ((Class::Excluded, _), None) if manifest.excluded_kinds.contains_key(&name) => {}
            ((class, Some(_)), Some(entry)) if !manifest.excluded_kinds.contains_key(&name) => {
                let proposed = match class {
                    Class::StableTarget => ProposedClass::StableTarget,
                    _ => ProposedClass::Experimental,
                };
                let vector = format!("vectors/psyche.{name}.v1.json");
                if entry.proposed_class != proposed || entry.vector != vector {
                    return Err(refuse_at("selection_mismatch", &name));
                }
                match proposed {
                    ProposedClass::StableTarget => stable_targets += 1,
                    ProposedClass::Experimental => experimental += 1,
                }
                expected_members.insert(vector);
                selected += 1;
            }
            _ => return Err(refuse_at("selection_mismatch", &name)),
        }
    }
    if manifest.kinds.len() != selected
        || manifest.excluded_kinds.len() != REGISTRY.len() - selected
        || manifest
            .excluded_kinds
            .values()
            .any(|reason| !is_text(reason))
    {
        return Err(refuse("selection_mismatch"));
    }

    // Members: exactly the expected set, each read once and hashed; every
    // later check uses these bytes.
    let manifest_names: BTreeSet<String> = manifest.files.keys().cloned().collect();
    if manifest_names != expected_members {
        let odd = manifest_names
            .symmetric_difference(&expected_members)
            .next()
            .cloned()
            .unwrap_or_default();
        return Err(refuse_at("manifest_invalid", &odd));
    }
    let mut present = BTreeSet::new();
    walk(root, "", &mut present)?;
    present.remove(MANIFEST);
    if let Some(extra) = present.difference(&expected_members).next() {
        return Err(refuse_at("unexpected_member", extra));
    }
    let mut members = BTreeMap::new();
    for (name, expected) in &manifest.files {
        if !is_hex(expected, 64) {
            return Err(refuse_at("manifest_invalid", name));
        }
        if !present.contains(name) {
            return Err(refuse_at("missing_member", name));
        }
        let bytes = read_bounded(&root.join(name), MEMBER_LIMIT, name)?;
        if sha256_hex(&bytes) != *expected {
            return Err(refuse_at("member_digest_mismatch", name));
        }
        members.insert(name.clone(), bytes);
    }
    let member = |name: &str| {
        members
            .get(name)
            .ok_or_else(|| refuse_at("missing_member", name))
    };

    // Component pins agree with the member bytes they name.
    let binding = &manifest.components.binding;
    let replay = &manifest.components.replay;
    if binding.schema_version != BINDING_SCHEMA || replay.profile != REPLAY_PROFILE {
        return Err(refuse("component_mismatch"));
    }
    for (pin, name) in [
        (&binding.request_sha256, "binding/golden/request.json"),
        (&binding.receipt_sha256, "binding/golden/receipt.json"),
        (&binding.binding_sha256, "binding/golden/binding.json"),
        (&replay.archive_sha256, "receipt-replay/receipt-replay.pyz"),
    ] {
        if *pin != sha256_hex(member(name)?) {
            return Err(refuse_at("component_mismatch", name));
        }
    }

    let mut vectors = BTreeMap::new();
    for kind in REGISTRY {
        let (_, Some(embedded)) = selection(*kind) else {
            continue;
        };
        let name = kind_name(*kind);
        let path = format!("vectors/psyche.{name}.v1.json");
        let bytes = member(&path)?;
        let document = decode_document(bytes).map_err(|_| refuse_at("vector_rejected", &name))?;
        if document.schema_version().kind != *kind {
            return Err(refuse_at("kind_mismatch", &name));
        }
        let encoded =
            canonical_bytes(&document).map_err(|_| refuse_at("vector_rejected", &name))?;
        if encoded != *bytes {
            return Err(refuse_at("canonical_bytes_mismatch", &name));
        }
        let pinned = manifest.kinds.get(&name).map(|entry| entry.sha256.as_str());
        if pinned != Some(sha256_hex(bytes).as_str()) {
            return Err(refuse_at("vector_digest_mismatch", &name));
        }
        if bytes.as_slice() != embedded {
            return Err(refuse_at("vector_not_in_profile", &name));
        }
        vectors.insert(name, bytes.clone());
    }

    let raw = member(DENIALS)?;
    let denials: Denials = strict(raw, "denials_invalid")?;
    if denials.format != DENIALS_FORMAT {
        return Err(refuse("denials_invalid"));
    }
    let mut required: BTreeSet<String> = GLOBAL_CASES
        .iter()
        .map(|case| format!("global/{case}"))
        .collect();
    for name in vectors.keys() {
        required.extend(KIND_CASES.iter().map(|case| format!("{name}/{case}")));
    }
    let mut names = BTreeSet::new();
    for case in &denials.cases {
        if !names.insert(case.name.clone()) {
            return Err(refuse_at("denials_invalid", &case.name));
        }
    }
    if let Some(missing) = required.symmetric_difference(&names).next() {
        return Err(refuse_at("denials_incomplete", missing));
    }
    let (mut rejected, mut canonicalized) = (0_u32, 0_u32);
    for case in &denials.cases {
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
                let kind = case.expect.get("vector").and_then(Value::as_str);
                let vector = kind
                    .filter(|kind| {
                        case.expect == json!({"result": "canonicalizes", "vector": kind})
                    })
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
    let embedded: Vec<u8> = String::from_utf8_lossy(EMBEDDED_DENIALS)
        .replace("\r\n", "\n")
        .into_bytes();
    if *raw != embedded {
        return Err(refuse("denials_not_in_profile"));
    }

    Ok(json!({
        "result": "passed",
        "code": "profile_verified",
        "profile": FORMAT,
        "source_commit": manifest.source_commit,
        "target": manifest.target,
        "members_verified": members.len(),
        "vectors_verified": vectors.len(),
        "proposed_stable_targets": stable_targets,
        "experimental_kinds": experimental,
        "excluded_kinds": manifest.excluded_kinds.len(),
        "components_verified": 2,
        "denials_verified": rejected,
        "canonicalizations_verified": canonicalized,
        "redispatch": false,
    }))
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
