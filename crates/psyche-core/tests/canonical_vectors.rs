//! Byte-exact canonical JSON and SHA-256 digest vectors for every registered
//! Psyche v1 document kind.
//!
//! Each file under `tests/fixtures/vectors/` holds exactly the RFC 8785 bytes
//! `psyche_core::digest::canonical_bytes` produces for one typed document
//! built in [`construct`]. The digest literals in [`pinned_digest`] are the
//! pin: a change to a vector file, a typed construction, or the encoder fails
//! here. The vectors are experimental and unpublished (issue #11); see the
//! `README.md` next to them for the regeneration command.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;

use psyche_core::contracts::error::{ErrorBody, ErrorCode, ErrorEnvelope};
use psyche_core::contracts::execution::{
    AdoptionState, CancellationAcknowledgementEvidence, CancellationAcknowledgementKind,
    CancellationState, ExecutionBinding, TerminationRequestCorrelation,
};
use psyche_core::contracts::foundation::{
    Addon, Approval, Budget, Delegation, Evidence, Recovery, Verdict,
};
use psyche_core::contracts::graph::{Graph, GraphNode, GraphState, NodeState};
use psyche_core::contracts::identity::{IdentityProvenance, IdentitySnapshot};
use psyche_core::contracts::intent::Intent;
use psyche_core::contracts::surface::{
    Delivery, DeliveryDecisionState, DeliveryRelationship, DeliveryState, DeliverySurfaceDecision,
    DeliveryTopic, SurfaceEffect, SurfaceEvent,
};
use psyche_core::contracts::{
    CanonicalDocument, ContractError, RecordKind, SchemaKind, SchemaVersion, decode_document,
};
use psyche_core::digest::{Sha256Digest, canonical_bytes, digest};
use psyche_core::id::{RecordId, RequestId};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

/// Setting this to exactly `1` regenerates every vector file from
/// [`construct`] before the assertions run. A file whose bytes already match
/// is not written; if any file is created or changed, every test in this file
/// fails, naming those files, so a run never verifies bytes it wrote itself.
/// Unset, no test in this file writes anything.
const REGENERATE_ENV: &str = "PSYCHE_REGENERATE_VECTORS";
const README: &str = "README.md";
/// Operating-system metadata files that can appear in a checkout without
/// being tracked. The directory test skips exactly these names; any other
/// unclaimed entry still fails it.
const IGNORED_OS_METADATA: &[&str] = &[".DS_Store", "Thumbs.db"];
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// `SchemaKind::ALL` is private to `psyche-core`, so the registry is restated
/// here. The array and the wildcard-free match are generated from one token
/// list: a `SchemaKind` variant missing from the list fails to compile, and a
/// variant in the list is always iterated.
macro_rules! registered_schema_kinds {
    ($($variant:ident),+ $(,)?) => {
        const SCHEMA_KINDS: &[SchemaKind] = &[$(SchemaKind::$variant),+];

        const fn is_registered(kind: SchemaKind) -> bool {
            match kind {
                $(SchemaKind::$variant)|+ => true,
            }
        }
    };
}

registered_schema_kinds!(
    IdentitySnapshot,
    Intent,
    SurfaceEvent,
    Graph,
    GraphNode,
    Delegation,
    Budget,
    Approval,
    ExecutionBinding,
    Evidence,
    Verdict,
    Recovery,
    Addon,
    SurfaceEffect,
    Delivery,
    Error,
);

/// The pin. A literal per kind, no wildcard arm.
fn pinned_digest(kind: SchemaKind) -> &'static str {
    match kind {
        SchemaKind::IdentitySnapshot => {
            "sha256:1da8dfa1d333edef27cbb3e1e2c384951d32299bda8d75f5712f690f07dde619"
        }
        SchemaKind::Intent => {
            "sha256:8ef6700f6d72a7666b57cc051342bc04491c47782f73950cc60fb2666071f5ed"
        }
        SchemaKind::SurfaceEvent => {
            "sha256:c458776df557249dbfba1e3d9e876e5f22f1353a0a1d2488bd822777d77761d2"
        }
        SchemaKind::Graph => {
            "sha256:fd10742cb0633589ed8e150a8b865bebcf7115ac96502ca097a5a39e28a54e93"
        }
        SchemaKind::GraphNode => {
            "sha256:e54a359eaa131b62da6ef743ae45be5a0bc9e203ce13b607c1cb9ccc209bf1cf"
        }
        SchemaKind::Delegation => {
            "sha256:0893d64053a40fbd6d7ac09c36719a801a4967ba70d376c83efccf6b84c396f7"
        }
        SchemaKind::Budget => {
            "sha256:f69fac29e8176690c0c4998649d92248a9a5f3e84ba510f6d02d2986acbf48be"
        }
        SchemaKind::Approval => {
            "sha256:32913ed310cd15bf99796c2ef780334c555303001ccc328a28780e134006665f"
        }
        SchemaKind::ExecutionBinding => {
            "sha256:1a3496c1483e958f399ba2fc23386396c1b8fa6e38d6a1538307843b2844b705"
        }
        SchemaKind::Evidence => {
            "sha256:4e16da1b930ac3632a48139d9ab93e803adcb603d742736ae45c5e86263955b0"
        }
        SchemaKind::Verdict => {
            "sha256:6f468f27a6705b1f5dc69193d276d6c796ed7c72e879ed6f41aa4750e7a784cb"
        }
        SchemaKind::Recovery => {
            "sha256:cc34e94d665cb945d2af6404675c06266e6892d5ff60e0cb76c106ea068c810d"
        }
        SchemaKind::Addon => {
            "sha256:73a3d18b2dceb2e307ef5038e1466493b46fba4962962ceac3c8e7b1ed57df2f"
        }
        SchemaKind::SurfaceEffect => {
            "sha256:30b6d32aa65134fb5e52cdcbf0955bea49fd2e74cbbf58bf40661106d4b5e0c4"
        }
        SchemaKind::Delivery => {
            "sha256:6ed2946ff8b51396c6c4bead2ccf7768256e201cca0436be6697d026a5a1ca54"
        }
        SchemaKind::Error => {
            "sha256:9b33c0f2d972ce9496ff2c4d02938712572ac68b87eb2a54c6d375b5a3035f97"
        }
    }
}

/// One fragment of each vector and a replacement that differs from it in
/// exactly one byte of a value while leaving the document valid.
fn single_byte_mutation(kind: SchemaKind) -> (&'static str, &'static str) {
    match kind {
        SchemaKind::IdentitySnapshot => (r#""revision":3"#, r#""revision":4"#),
        SchemaKind::Intent => (r#"synthetic change.""#, r#"synthetic change!""#),
        SchemaKind::SurfaceEvent => (
            r#""account_id":"account-synthetic-1""#,
            r#""account_id":"account-synthetic-2""#,
        ),
        SchemaKind::Graph => (r#""version":4"#, r#""version":5"#),
        SchemaKind::GraphNode => (r#""version":2"#, r#""version":3"#),
        SchemaKind::Delegation => (r#""scope_digest":"sha256:a"#, r#""scope_digest":"sha256:b"#),
        SchemaKind::Budget => (r#""consumed":125"#, r#""consumed":126"#),
        SchemaKind::Approval => (
            r#""requester_principal_id":"principal:synthetic-1""#,
            r#""requester_principal_id":"principal:synthetic-2""#,
        ),
        SchemaKind::ExecutionBinding => (r#""revision":2"#, r#""revision":3"#),
        SchemaKind::Evidence => (r#""size":2048"#, r#""size":2049"#),
        SchemaKind::Verdict => (
            r#""reviewer_id":"reviewer:synthetic-1""#,
            r#""reviewer_id":"reviewer:synthetic-2""#,
        ),
        SchemaKind::Recovery => (r#""reconciliation_count":2"#, r#""reconciliation_count":3"#),
        SchemaKind::Addon => (r#""version":"1.0.0""#, r#""version":"1.0.1""#),
        SchemaKind::SurfaceEffect => (
            r#""account_id":"account-synthetic-1""#,
            r#""account_id":"account-synthetic-2""#,
        ),
        SchemaKind::Delivery => (r#""attempt_count":1"#, r#""attempt_count":2"#),
        SchemaKind::Error => (
            r#""correlation_id":"corr-synthetic-1""#,
            r#""correlation_id":"corr-synthetic-2""#,
        ),
    }
}

fn schema(kind: SchemaKind) -> SchemaVersion {
    SchemaVersion { kind, major: 1 }
}

/// `psyche.<kind>.v1.json`, derived from the registry's public string form.
fn vector_file_name(kind: SchemaKind) -> String {
    format!("{}.json", schema(kind))
}

fn vector_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vectors")
}

fn regenerate_requested() -> bool {
    match std::env::var(REGENERATE_ENV) {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "1" => true,
        other => panic!("{REGENERATE_ENV} must be unset or exactly \"1\", got {other:?}"),
    }
}

/// The documented generator: typed construction -> validation -> the crate's
/// RFC 8785 encoder -> file. Reached only when [`REGENERATE_ENV`] is `1`.
/// Each file is compared with the encoder output before anything is written,
/// and only a missing or different file is written. Returns one line per file
/// created or changed, saying whether its digest literal is now stale.
fn regenerate() -> Vec<String> {
    std::fs::create_dir_all(vector_dir()).unwrap();
    let mut written = Vec::new();
    println!("regenerated canonical vectors (file, bytes, digest, action):");
    for kind in SCHEMA_KINDS.iter().copied() {
        let document = construct(kind);
        document.validate().unwrap();
        let bytes = canonical_bytes(&document).unwrap();
        let pin = digest(&document).unwrap();
        let file = vector_file_name(kind);
        let path = vector_dir().join(&file);
        let action = match std::fs::read(&path) {
            Ok(existing) if existing == bytes => "unchanged",
            Ok(_) => "changed",
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "created",
            Err(error) => panic!("cannot read {file} before regenerating it: {error}"),
        };
        if action != "unchanged" {
            std::fs::write(&path, &bytes).unwrap();
            let literal = if pin.as_str() == pinned_digest(kind) {
                "matches"
            } else {
                "is stale"
            };
            written.push(format!("{action} {file} (pinned_digest literal {literal})"));
        }
        println!("{file} {} {pin} {action}", bytes.len());
    }
    println!("pinned_digest arms:");
    for kind in SCHEMA_KINDS {
        let pin = digest(&construct(*kind)).unwrap();
        println!("        SchemaKind::{kind:?} => \"{pin}\",");
    }
    written
}

/// Every read goes through here so a requested regeneration happens exactly
/// once, before any assertion, whichever test the harness schedules first.
/// A regeneration that created or changed a file fails every caller: the
/// rewritten bytes are reviewed and re-run, never passed by the run that
/// wrote them. One that changed nothing wrote nothing, so the assertions run
/// over the committed bytes exactly as in a run without the variable.
fn vector(kind: SchemaKind) -> Vec<u8> {
    static WRITTEN: OnceLock<Vec<String>> = OnceLock::new();
    let written = WRITTEN.get_or_init(|| {
        if regenerate_requested() {
            regenerate()
        } else {
            Vec::new()
        }
    });
    assert!(
        written.is_empty(),
        "{REGENERATE_ENV}=1 created or changed {} vector file(s); this run fails on purpose. \
         Review the diff, re-pin any stale pinned_digest literal under R4 review, and re-run \
         without the variable:\n  {}",
        written.len(),
        written.join("\n  ")
    );
    let file = vector_file_name(kind);
    std::fs::read(vector_dir().join(&file))
        .unwrap_or_else(|error| panic!("no vector file {file} for {kind:?}: {error}"))
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("vector bytes are UTF-8")
}

/// Asserts that two UTF-8 JSON texts are the same bytes. A mismatch reports
/// both lengths, the first differing offset and a short window from each side
/// instead of both documents in full.
fn assert_same_bytes(actual: &[u8], expected: &[u8], context: impl std::fmt::Display) {
    let _ = (text(actual), text(expected));
    if actual == expected {
        return;
    }
    let offset = actual
        .iter()
        .zip(expected)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| actual.len().min(expected.len()));
    let window = |bytes: &[u8]| {
        let end = (offset + 24).min(bytes.len());
        String::from_utf8_lossy(&bytes[offset.saturating_sub(24)..end]).into_owned()
    };
    panic!(
        "{context}: bytes differ at offset {offset} (actual {} bytes, expected {} bytes)\n  \
         actual near offset:   {:?}\n  expected near offset: {:?}",
        actual.len(),
        expected.len(),
        window(actual),
        window(expected),
    );
}

/// SHA-256 over raw bytes with the workspace's one SHA-256 implementation,
/// independent of `psyche_core::digest::digest`.
fn raw_sha256(bytes: &[u8]) -> String {
    let mut out = String::from(Sha256Digest::PREFIX);
    for byte in Sha256::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn replace_once(source: &[u8], from: &str, to: &str) -> Vec<u8> {
    let source = text(source);
    assert_eq!(source.matches(from).count(), 1, "fragment {from}");
    source.replacen(from, to, 1).into_bytes()
}

fn schema_fragment(schema_version: &str) -> String {
    format!(r#""schema_version":"{schema_version}""#)
}

fn timestamp(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
}

fn ulid(n: u8) -> String {
    format!("01J000000000000000000000{n:02}")
}

fn record_id(kind: RecordKind, n: u8) -> RecordId {
    RecordId::parse(kind, &format!("{}{}", kind.prefix(), ulid(n))).unwrap()
}

fn request_id(n: u8) -> RequestId {
    RequestId::parse(&format!("{}{}", RequestId::PREFIX, ulid(n))).unwrap()
}

/// A placeholder digest: 64 copies of one hex nibble. It binds no content.
fn hex_digest(nibble: char) -> Sha256Digest {
    Sha256Digest::parse(&format!("sha256:{}", nibble.to_string().repeat(64))).unwrap()
}

/// The typed, synthetic document each vector is generated from. Struct
/// literals are exhaustive, so adding a field to a record fails to compile
/// here before it can silently change a vector.
fn construct(kind: SchemaKind) -> CanonicalDocument {
    let schema_version = schema(kind);
    match kind {
        SchemaKind::IdentitySnapshot => CanonicalDocument::IdentitySnapshot(IdentitySnapshot {
            schema_version,
            snapshot_id: record_id(RecordKind::IdentitySnapshot, 1),
            familiar_id: "familiar:synthetic-1".into(),
            principal_id: "principal:synthetic-1".into(),
            revision: 3,
            declaration_digest: hex_digest('a'),
            identity_file_digest: hex_digest('b'),
            identity_digest: hex_digest('c'),
            soul_digest: hex_digest('d'),
            role_skill_digest: hex_digest('e'),
            provenance: IdentityProvenance {
                familiar_home_id: "home:synthetic-1".into(),
                resolver_version: "resolver-synthetic-1".into(),
            },
            resolved_at: timestamp("2026-08-01T00:00:00Z"),
        }),
        SchemaKind::Intent => {
            let Value::Object(constraints) = json!({
                "allowed_actions": ["read", "test"],
                "limits": {"max_attempts": 2, "dry_run": true, "escalation": null}
            }) else {
                unreachable!()
            };
            CanonicalDocument::Intent(Intent {
                schema_version,
                intent_id: record_id(RecordKind::Intent, 1),
                principal_id: "principal:synthetic-1".into(),
                familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, 2),
                project_id: "project:synthetic-1".into(),
                requested_outcome: "Review and verify the synthetic change.".into(),
                constraints,
                required_evidence: vec!["tests".into(), "diff_review".into()],
                surface_event_id: Some(record_id(RecordKind::SurfaceEvent, 3)),
                created_at: timestamp("2026-08-01T00:00:00Z"),
                digest: hex_digest('1'),
            })
        }
        SchemaKind::SurfaceEvent => CanonicalDocument::SurfaceEvent(SurfaceEvent {
            schema_version,
            surface_event_id: record_id(RecordKind::SurfaceEvent, 1),
            adapter_id: "adapter-synthetic".into(),
            account_id: "account-synthetic-1".into(),
            actor: json!({"type": "user", "id": "1001"}),
            locator: json!({
                "type": "message",
                "chat_id": "-1000000000001",
                "message_id": "42"
            }),
            adapter_event_digest: hex_digest('2'),
            received_at: timestamp("2026-08-01T00:00:00Z"),
            // Free-form adapter content is where RFC 8785 string, number and
            // key-ordering rules are observable: the two-character escapes
            // `\b \f \n \r \t`, `\u00XX` control escapes, non-ASCII text, a
            // fraction, a negative number, an exponent, `0.000001` (decimal
            // in RFC 8785, `1e-6` in plain serde_json), and two keys whose
            // UTF-16 code-unit order (U+1F600 before U+FFE6) differs from
            // code-point order.
            content: json!({
                "type": "text",
                "text": "Synthetic line one\nline two \"quoted\" \\ \u{20ac} \u{1F600}",
                "control": "\u{1}\u{1f}",
                "short_escapes": "\u{8}\u{c}\r\t",
                "micro": 0.000001,
                "negative": -1.5,
                "entities": [
                    {"kind": "bold", "offset": 0, "length": 9},
                    {"kind": "italic", "offset": 10, "length": 4}
                ],
                "ratio": 0.25,
                "scale": 1e-7,
                "\u{1F600}": "astral key",
                "\u{FFE6}": "bmp key",
                "empty_list": [],
                "empty_object": {},
                "flag": false,
                "nothing": null
            }),
        }),
        SchemaKind::Graph => CanonicalDocument::Graph(Graph {
            schema_version,
            graph_id: record_id(RecordKind::Graph, 1),
            root_intent_id: record_id(RecordKind::Intent, 2),
            owner_principal_id: "principal:synthetic-1".into(),
            policy_revision: "policy:synthetic-rev-1".into(),
            state: GraphState::WaitingEvidence,
            version: 4,
        }),
        SchemaKind::GraphNode => CanonicalDocument::GraphNode(GraphNode {
            schema_version,
            node_id: record_id(RecordKind::GraphNode, 1),
            graph_id: record_id(RecordKind::Graph, 2),
            familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, 3),
            dependencies: vec![
                record_id(RecordKind::GraphNode, 4),
                record_id(RecordKind::GraphNode, 5),
            ],
            delegation_id: Some(record_id(RecordKind::Delegation, 6)),
            budget_id: record_id(RecordKind::Budget, 7),
            required_evidence: vec!["tests".into(), "diff_review".into()],
            state: NodeState::AwaitingVerification,
            version: 2,
        }),
        SchemaKind::Delegation => CanonicalDocument::Delegation(Delegation {
            schema_version,
            delegation_id: record_id(RecordKind::Delegation, 1),
            parent_node_id: record_id(RecordKind::GraphNode, 2),
            child_node_id: record_id(RecordKind::GraphNode, 3),
            scope_digest: hex_digest('a'),
            budget_id: record_id(RecordKind::Budget, 4),
            evidence_scope_digest: hex_digest('b'),
            cancellation_policy: "cascade".into(),
        }),
        SchemaKind::Budget => CanonicalDocument::Budget(Budget {
            schema_version,
            budget_id: record_id(RecordKind::Budget, 1),
            graph_id: record_id(RecordKind::Graph, 2),
            resource_class: "tokens".into(),
            // The largest integer the contract accepts, to pin its spelling.
            limit: MAX_SAFE_INTEGER,
            reserved: 250,
            consumed: 125,
            released: 0,
        }),
        SchemaKind::Approval => CanonicalDocument::Approval(Approval {
            schema_version,
            approval_id: record_id(RecordKind::Approval, 1),
            node_id: record_id(RecordKind::GraphNode, 2),
            requester_principal_id: "principal:synthetic-1".into(),
            decision: Some("approved".into()),
            expires_at: timestamp("2026-08-01T00:30:00Z"),
        }),
        SchemaKind::ExecutionBinding => {
            // The shape of `valid_binding(AcknowledgedTerminated)` in
            // `contracts.rs`, at a second revision with every optional field
            // the acknowledged state permits.
            let request = request_id(1);
            let termination = request_id(2);
            let request_digest = hex_digest('1');
            CanonicalDocument::ExecutionBinding(ExecutionBinding {
                schema_version,
                attempt_id: record_id(RecordKind::Attempt, 1),
                revision: 2,
                previous_revision_digest: Some(hex_digest('3')),
                revision_created_at: timestamp("2026-08-01T00:02:30Z"),
                familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, 2),
                project_id: "project:synthetic-1".into(),
                request_id: request.clone(),
                request_digest: request_digest.clone(),
                request_created_at: timestamp("2026-08-01T00:00:00Z"),
                request_valid_until: timestamp("2026-08-01T00:10:00Z"),
                coven_contract_version: "coven.execution.v1".into(),
                coven_session_id: Some("session-synthetic-1".into()),
                adoption_state: AdoptionState::Adopted,
                event_cursor: Some("cursor-0007".into()),
                cancellation_state: CancellationState::AcknowledgedTerminated,
                termination_request: Some(TerminationRequestCorrelation {
                    termination_request_id: termination.clone(),
                    created_at: timestamp("2026-08-01T00:01:00Z"),
                    valid_until: timestamp("2026-08-01T00:05:00Z"),
                }),
                termination_reason_code: Some("operator_requested".into()),
                cancellation_acknowledgement: Some(CancellationAcknowledgementEvidence {
                    acknowledgement_id: "ack-synthetic-1".into(),
                    termination_request_id: termination,
                    session_id: "session-synthetic-1".into(),
                    execution_request_id: request,
                    execution_request_digest: request_digest,
                    kind: CancellationAcknowledgementKind::Terminated,
                    authority_evidence_digest: hex_digest('2'),
                    acknowledged_at: timestamp("2026-08-01T00:02:00Z"),
                }),
                cancellation_unresolved: None,
                terminal_state: Some("terminated".into()),
            })
        }
        SchemaKind::Evidence => CanonicalDocument::Evidence(Evidence {
            schema_version,
            evidence_id: record_id(RecordKind::Evidence, 1),
            node_id: record_id(RecordKind::GraphNode, 2),
            attempt_id: record_id(RecordKind::Attempt, 3),
            content_digest: hex_digest('a'),
            producer: "producer-synthetic".into(),
            collection_method: "test".into(),
            media_type: "text/plain".into(),
            size: 2048,
            created_at: timestamp("2026-08-01T00:03:00Z"),
            retention_policy: "default".into(),
        }),
        SchemaKind::Verdict => CanonicalDocument::Verdict(Verdict {
            schema_version,
            verdict_id: record_id(RecordKind::Verdict, 1),
            node_id: record_id(RecordKind::GraphNode, 2),
            sealed_evidence_digest: hex_digest('a'),
            policy_revision: "policy:synthetic-rev-1".into(),
            verdict_type: "review".into(),
            reviewer_id: "reviewer:synthetic-1".into(),
            outcome: "allow".into(),
            reason_codes: vec!["tests_passed".into(), "diff_reviewed".into()],
            created_at: timestamp("2026-08-01T00:04:00Z"),
        }),
        SchemaKind::Recovery => CanonicalDocument::Recovery(Recovery {
            schema_version,
            recovery_id: record_id(RecordKind::Recovery, 1),
            attempt_id: record_id(RecordKind::Attempt, 2),
            lease_id: "lease:synthetic-1".into(),
            fence_token: Some("fence-0001".into()),
            ambiguity: "adoption_unknown".into(),
            reconciliation_count: 2,
            operator_disposition: Some("reconcile".into()),
        }),
        SchemaKind::Addon => CanonicalDocument::Addon(Addon {
            schema_version,
            addon_id: record_id(RecordKind::Addon, 1),
            package: "addon-synthetic".into(),
            version: "1.0.0".into(),
            package_digest: hex_digest('a'),
            provenance_digest: hex_digest('b'),
            contributions_digest: hex_digest('c'),
            allowlist_digest: hex_digest('d'),
            revocation_state: "active".into(),
        }),
        SchemaKind::SurfaceEffect => {
            let effect = json!({"type": "message", "text": "Synthetic review complete."});
            CanonicalDocument::SurfaceEffect(SurfaceEffect {
                schema_version,
                surface_effect_id: record_id(RecordKind::SurfaceEffect, 1),
                intent_id: record_id(RecordKind::Intent, 2),
                graph_id: record_id(RecordKind::Graph, 3),
                node_id: record_id(RecordKind::GraphNode, 4),
                attempt_id: record_id(RecordKind::Attempt, 5),
                familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, 6),
                project_id: "project:synthetic-1".into(),
                action_class: "telegram.reply.send".into(),
                account_id: "account-synthetic-1".into(),
                locator: json!({"chat_id": "-1000000000001", "message_id": "42"}),
                effect_digest: digest(&effect).unwrap(),
                effect,
                created_at: timestamp("2026-08-01T00:01:00Z"),
            })
        }
        SchemaKind::Delivery => {
            // The effect shape of `delivery-ready.json`, in the `sent` state
            // so the optional message identifier is present.
            let effect = json!({
                "schema_version": "psyche.telegram_effect.v1",
                "type": "send_message",
                "format": "html",
                "text": "Synthetic review complete.",
                "reply_to_message_id": "314",
                "buttons": [],
                "link_preview": {"enabled": true}
            });
            CanonicalDocument::Delivery(Delivery {
                schema_version,
                delivery_id: record_id(RecordKind::Delivery, 1),
                intent_id: record_id(RecordKind::Intent, 2),
                action_class: "telegram.reply.send".into(),
                account_id: "account-synthetic-1".into(),
                chat_id: "-1000000000001".into(),
                topic: DeliveryTopic {
                    kind: "forum".into(),
                    id: "42".into(),
                },
                relationship: DeliveryRelationship::ReplySameTopic,
                effect_digest: digest(&effect).unwrap(),
                effect,
                surface_decision: DeliverySurfaceDecision {
                    decision_id: "decision-synthetic-1".into(),
                    request_digest: hex_digest('1'),
                    policy_revision: "policy:synthetic-rev-1".into(),
                    expires_at: timestamp("2026-08-01T00:05:00Z"),
                    state: DeliveryDecisionState::Consumed,
                },
                logical_response_id: "response-synthetic-1".into(),
                logical_part: 0,
                state: DeliveryState::Sent,
                attempt_count: 1,
                telegram_message_id: Some("315".into()),
            })
        }
        SchemaKind::Error => CanonicalDocument::Error(ErrorEnvelope {
            schema_version,
            error: ErrorBody {
                code: ErrorCode::StorageUnavailable,
                message: "Synthetic storage is temporarily unavailable.".into(),
                retryable: true,
                correlation_id: "corr-synthetic-1".into(),
                details: BTreeMap::from([
                    ("component".to_owned(), "store".to_owned()),
                    ("operation".to_owned(), "write".to_owned()),
                ]),
            },
        }),
    }
}

/// A deliberately non-canonical spelling of the same JSON data: object keys in
/// descending order, insignificant whitespace, non-ASCII characters written
/// as `\uXXXX` escapes, and a trailing newline. Numbers keep their spelling.
fn respell(value: &Value) -> Vec<u8> {
    respell_spelling(value, None).0
}

/// [`respell`], and every number inside the top-level member `member`
/// rewritten by [`respell_number`]. Typed integer fields cannot carry a
/// fraction or exponent, so only a free-form member is eligible. Returns the
/// bytes and each `(original, respelling)` number pair in output order.
fn respell_numbers_in(value: &Value, member: &str) -> (Vec<u8>, Vec<(String, String)>) {
    respell_spelling(value, Some(member))
}

fn respell_spelling(value: &Value, numbers_in: Option<&str>) -> (Vec<u8>, Vec<(String, String)>) {
    fn string(value: &str, out: &mut String) {
        for character in serde_json::to_string(value).unwrap().chars() {
            if character.is_ascii() {
                out.push(character);
            } else {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
        }
    }

    fn walk(
        value: &Value,
        out: &mut String,
        numbers_in: Option<&str>,
        respell_numbers: bool,
        numbers: &mut Vec<(String, String)>,
    ) {
        match value {
            Value::Object(members) => {
                let mut members: Vec<_> = members.iter().collect();
                members.sort_by(|left, right| right.0.cmp(left.0));
                out.push_str("{\n");
                for (index, (key, member)) in members.into_iter().enumerate() {
                    if index > 0 {
                        out.push_str(" ,\n");
                    }
                    out.push_str("  ");
                    string(key, out);
                    out.push_str(" : ");
                    let nested = respell_numbers || numbers_in == Some(key.as_str());
                    walk(member, out, None, nested, numbers);
                }
                out.push_str("\n}");
            }
            Value::Array(items) => {
                out.push_str("[ ");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(" , ");
                    }
                    walk(item, out, None, respell_numbers, numbers);
                }
                out.push_str(" ]");
            }
            Value::String(value) => string(value, out),
            Value::Number(number) if respell_numbers => {
                let original = number.to_string();
                let respelling = respell_number(&original, numbers.len());
                out.push_str(&respelling);
                numbers.push((original, respelling));
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                out.push_str(&serde_json::to_string(value).unwrap());
            }
        }
    }

    let mut out = String::new();
    let mut numbers = Vec::new();
    walk(value, &mut out, numbers_in, false, &mut numbers);
    out.push('\n');
    (out.into_bytes(), numbers)
}

/// Another spelling of the same JSON number value. Even `variant`s give a
/// plain decimal with one superfluous trailing zero (`9` -> `9.0`, `0.25` ->
/// `0.250`, `1e-7` -> `0.00000010`); odd ones give scientific notation with
/// one leading digit (`0.25` -> `2.5e-1`, `0.000001` -> `1.0e-6`, `9` ->
/// `9.0E+0`). Both parse to the same value as `text`.
fn respell_number(text: &str, variant: usize) -> String {
    let (sign, unsigned) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], unsigned[at + 1..].parse::<i64>().unwrap()),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    // The value is `digits * 10^scale`, with `digits` free of leading and
    // trailing zeros (or exactly `0`).
    let mut digits = format!("{whole}{fraction}")
        .trim_start_matches('0')
        .to_owned();
    let mut scale = exponent - i64::try_from(fraction.len()).unwrap();
    if digits.is_empty() {
        digits.push('0');
        scale = 0;
    }
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
        scale += 1;
    }

    if variant % 2 == 0 {
        let plain = if scale >= 0 {
            format!("{digits}{}.0", "0".repeat(usize::try_from(scale).unwrap()))
        } else {
            let places = usize::try_from(-scale).unwrap();
            if digits.len() > places {
                let (integral, fractional) = digits.split_at(digits.len() - places);
                format!("{integral}.{fractional}0")
            } else {
                format!("0.{}{digits}0", "0".repeat(places - digits.len()))
            }
        };
        format!("{sign}{plain}")
    } else {
        let (lead, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        let power = scale + i64::try_from(digits.len()).unwrap() - 1;
        if power < 0 {
            format!("{sign}{lead}.{rest}e{power}")
        } else {
            format!("{sign}{lead}.{rest}E+{power}")
        }
    }
}

#[test]
fn every_registered_kind_has_a_pinned_canonical_vector() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let pin = pinned_digest(kind);

        let document = decode_document(&file)
            .unwrap_or_else(|error| panic!("{kind:?} vector does not decode: {error:?}"));
        assert_eq!(document.schema_version(), schema(kind), "{kind:?}");
        match kind.record_kind() {
            Some(record_kind) => {
                let id = document
                    .persistable_record_id()
                    .unwrap_or_else(|| panic!("{kind:?} vector has no record ID"));
                assert_eq!(id.kind(), record_kind, "{kind:?}");
                assert!(id.as_str().starts_with(record_kind.prefix()), "{kind:?}");
            }
            None => {
                assert_eq!(kind, SchemaKind::Error);
                assert!(document.persistable_record_id().is_none());
            }
        }

        // The file is exactly what the encoder produces for the decoded
        // document, for the same data read untyped, and for the typed
        // construction the generator uses.
        assert_same_bytes(
            &canonical_bytes(&document).unwrap(),
            &file,
            format!("{kind:?} decoded"),
        );
        let untyped: Value = serde_json::from_slice(&file).unwrap();
        assert_same_bytes(
            &canonical_bytes(&untyped).unwrap(),
            &file,
            format!("{kind:?} untyped"),
        );
        let constructed = construct(kind);
        constructed.validate().unwrap();
        assert_eq!(constructed, document, "{kind:?}");
        assert_same_bytes(
            &canonical_bytes(&constructed).unwrap(),
            &file,
            format!("{kind:?} constructed"),
        );

        assert_eq!(digest(&document).unwrap().as_str(), pin, "{kind:?}");
        assert_eq!(raw_sha256(&file), pin, "{kind:?}");
    }
}

#[test]
fn vector_directory_holds_exactly_one_file_per_registered_kind() {
    for kind in SCHEMA_KINDS.iter().copied() {
        assert!(is_registered(kind));
        // Force any requested regeneration before the directory is listed.
        let _ = vector(kind);
        let name = schema(kind).to_string();
        assert_eq!(SchemaVersion::parse(&name), Ok(schema(kind)), "{name}");
    }
    let distinct: BTreeSet<String> = SCHEMA_KINDS.iter().map(|k| vector_file_name(*k)).collect();
    assert_eq!(distinct.len(), SCHEMA_KINDS.len(), "duplicate kind");
    assert_eq!(SCHEMA_KINDS.len(), 16);

    // Every durable record kind is claimed by exactly one vector, and the
    // error envelope is the only kind without one.
    let claimed: Vec<RecordKind> = SCHEMA_KINDS
        .iter()
        .filter_map(|kind| kind.record_kind())
        .collect();
    assert_eq!(claimed.len(), RecordKind::ALL.len());
    for record_kind in RecordKind::ALL {
        assert_eq!(
            claimed.iter().filter(|kind| **kind == record_kind).count(),
            1,
            "{record_kind:?}"
        );
    }
    let unstored: Vec<_> = SCHEMA_KINDS
        .iter()
        .filter(|kind| kind.record_kind().is_none())
        .collect();
    assert_eq!(unstored, [&SchemaKind::Error]);

    let mut expected = distinct;
    expected.insert(README.to_owned());
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(vector_dir()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        assert!(entry.file_type().unwrap().is_file(), "{name} is not a file");
        if IGNORED_OS_METADATA.contains(&name.as_str()) {
            continue;
        }
        found.insert(name);
    }
    assert_eq!(found, expected, "missing or unclaimed vector file");
}

// An acceptance control. That the decoder accepts insignificant whitespace,
// including the respelling's trailing newline, pins observed decoder
// behaviour, not a contract: a stricter decoder would fail this test and
// need a decision, not a weaker assertion.
#[test]
fn non_canonical_spellings_canonicalize_to_the_pinned_bytes_and_digest() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let pin = pinned_digest(kind);
        let respelled = respell(&serde_json::from_slice(&file).unwrap());

        assert_ne!(text(&respelled), text(&file), "{kind:?}");
        assert_ne!(raw_sha256(&respelled), pin, "{kind:?}");

        let document = decode_document(&respelled)
            .unwrap_or_else(|error| panic!("{kind:?} respelling does not decode: {error:?}"));
        assert_eq!(document, decode_document(&file).unwrap(), "{kind:?}");
        assert_same_bytes(
            &canonical_bytes(&document).unwrap(),
            &file,
            format!("{kind:?} respelled"),
        );
        assert_eq!(digest(&document).unwrap().as_str(), pin, "{kind:?}");
    }

    // Number spellings, confined to surface_event's free-form `content`: the
    // one vector whose free-form data holds integers, fractions, a negative
    // number and exponents, and where a typed integer field cannot be hit.
    let kind = SchemaKind::SurfaceEvent;
    let file = vector(kind);
    let pin = pinned_digest(kind);
    let (respelled, numbers) =
        respell_numbers_in(&serde_json::from_slice(&file).unwrap(), "content");
    // Every number in `content`, in the respelling's descending key order,
    // with the spelling it was given.
    let expected = [
        ("1e-7", "0.00000010"),
        ("0.25", "2.5e-1"),
        ("-1.5", "-1.50"),
        ("0.000001", "1.0e-6"),
        ("0", "0.0"),
        ("9", "9.0E+0"),
        ("10", "10.0"),
        ("4", "4.0E+0"),
    ]
    .map(|(original, respelling)| (original.to_owned(), respelling.to_owned()));
    assert_eq!(numbers, expected);
    assert_ne!(
        text(&respelled),
        text(&respell(&serde_json::from_slice(&file).unwrap()))
    );
    assert_ne!(raw_sha256(&respelled), pin);

    // Decoded values keep each number's source spelling (the workspace enables
    // serde_json's `arbitrary_precision`), so `2.5e-1` and `0.25` decode
    // unequal even though they canonicalize identically. The control is
    // therefore on the re-encoded bytes and digest, not on decoded values.
    let document = decode_document(&respelled)
        .unwrap_or_else(|error| panic!("number respelling does not decode: {error:?}"));
    assert_eq!(document.schema_version(), schema(kind));
    assert_same_bytes(
        &canonical_bytes(&document).unwrap(),
        &file,
        "SurfaceEvent number respelling",
    );
    assert_eq!(digest(&document).unwrap().as_str(), pin);
}

#[test]
fn changing_one_value_byte_changes_the_digest() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let pin = pinned_digest(kind);
        let (from, to) = single_byte_mutation(kind);
        assert_eq!(from.len(), to.len(), "{kind:?}");
        let changed = replace_once(&file, from, to);
        assert_eq!(
            file.iter().zip(&changed).filter(|(a, b)| a != b).count(),
            1,
            "{kind:?}"
        );

        // Still a valid, canonical document of the same kind: only the
        // digest tells the two apart.
        let document = decode_document(&changed)
            .unwrap_or_else(|error| panic!("{kind:?} mutation does not decode: {error:?}"));
        assert_eq!(document.schema_version(), schema(kind), "{kind:?}");
        assert_same_bytes(
            &canonical_bytes(&document).unwrap(),
            &changed,
            format!("{kind:?} mutated"),
        );
        assert_ne!(document, decode_document(&file).unwrap(), "{kind:?}");
        assert_ne!(digest(&document).unwrap().as_str(), pin, "{kind:?}");
        assert_ne!(raw_sha256(&changed), pin, "{kind:?}");
    }
}

#[test]
fn unknown_fields_majors_and_schema_names_are_structured_denials() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let name = schema(kind).to_string();

        let mut unknown_field = br#"{"aa_unknown_field":true,"#.to_vec();
        unknown_field.extend_from_slice(&file[1..]);
        assert_eq!(
            decode_document(&unknown_field),
            Err(ContractError::InvalidShape {
                schema: kind,
                field: "document",
            }),
            "{kind:?}"
        );

        let unknown_major = replace_once(
            &file,
            &schema_fragment(&name),
            &schema_fragment(&name.replace(".v1", ".v2")),
        );
        assert_eq!(
            decode_document(&unknown_major),
            Err(ContractError::UnsupportedMajor {
                found: 2,
                supported: 1,
            }),
            "{kind:?}"
        );

        let unknown_name = replace_once(
            &file,
            &schema_fragment(&name),
            &schema_fragment(&name.replace(".v1", "_unknown.v1")),
        );
        assert_eq!(
            decode_document(&unknown_name),
            Err(ContractError::UnknownSchema),
            "{kind:?}"
        );
    }
}

#[test]
fn a_duplicate_key_is_rejected_before_the_schema_is_dispatched() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let fragment = schema_fragment(&schema(kind).to_string());
        assert_eq!(text(&file).matches(&fragment).count(), 1, "{kind:?}");
        let mut duplicate = format!("{{{fragment},");
        duplicate.push_str(text(&file[1..]));
        assert_eq!(duplicate.matches(&fragment).count(), 2, "{kind:?}");

        // Strict JSON parsing fails before the version probe, so the denial
        // is attributed to the error envelope kind, not the document's own.
        assert_eq!(
            decode_document(duplicate.as_bytes()),
            Err(ContractError::InvalidShape {
                schema: SchemaKind::Error,
                field: "json",
            }),
            "{kind:?}"
        );
    }
}

#[test]
fn a_trailing_newline_decodes_but_is_not_the_canonical_spelling() {
    for kind in SCHEMA_KINDS.iter().copied() {
        let file = vector(kind);
        let pin = pinned_digest(kind);
        assert_ne!(file.last(), Some(&b'\n'), "{kind:?}");

        let mut with_newline = file.clone();
        with_newline.push(b'\n');
        assert_ne!(raw_sha256(&with_newline), pin, "{kind:?}");

        // The decoder accepts insignificant trailing whitespace; only the
        // re-encoded bytes, not the received ones, carry the pinned digest.
        // That acceptance pins observed decoder behaviour, not a contract.
        let document = decode_document(&with_newline)
            .unwrap_or_else(|error| panic!("{kind:?} with newline: {error:?}"));
        assert_eq!(document, decode_document(&file).unwrap(), "{kind:?}");
        assert_same_bytes(
            &canonical_bytes(&document).unwrap(),
            &file,
            format!("{kind:?} with newline"),
        );
        assert_eq!(digest(&document).unwrap().as_str(), pin, "{kind:?}");
    }
}
