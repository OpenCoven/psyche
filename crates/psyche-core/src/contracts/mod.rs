//! Canonical contract primitives and the strict Psyche v1 document decoder.
//!
//! This module is the sole authority mapping a [`SchemaKind`] to the
//! [`RecordKind`] it produces (if any), and the sole authority for which
//! `psyche.<kind>.v<major>` strings this build accepts as a [`SchemaVersion`].
//! Task 2 stops at these primitives: no `records`, no `CanonicalDocument`, no
//! store validation, and no `QuarantineId` — those are store-owned and land
//! in later tasks (`QuarantineId` explicitly in Task 7).
use std::fmt;
use std::str::FromStr;

use serde::Serialize;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::id::RecordId;
use crate::serde_json_number;

macro_rules! validated_struct {
    (
        pub struct $name:ident, $wire:ident {
            $($(#[$field_meta:meta])* pub $field:ident: $ty:ty),+ $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq, serde::Serialize)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: $ty),+
        }

        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $wire {
            $($(#[$field_meta])* $field: $ty),+
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let wire = <$wire as serde::Deserialize>::deserialize(deserializer)?;
                let value = Self {
                    $($field: wire.$field),+
                };
                value.validate().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
    };
}

pub mod error;
pub mod execution;
pub mod foundation;
pub mod graph;
pub mod identity;
pub mod intent;
pub mod surface;

pub use error::ErrorEnvelope;
pub use execution::ExecutionBinding;
pub use foundation::{Addon, Approval, Budget, Delegation, Evidence, Recovery, Verdict};
pub use graph::{Graph, GraphNode};
pub use identity::IdentitySnapshot;
pub use intent::Intent;
pub use surface::{Delivery, SurfaceEffect, SurfaceEvent};

/// Maximum accepted encoded or embedded canonical document size.
pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_JSON_DEPTH: usize = 128;

/// Reasons a contract primitive failed to validate.
///
/// Every variant is deliberately payload-light: an invalid `RecordId`,
/// `RequestId`, `Sha256Digest`, or `SchemaVersion` is untrusted input, and the
/// error carries only what a caller needs to react to — not enough to
/// reconstruct the rejected value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// A `psyche.<kind>.v<n>` string named a kind this build does not define.
    /// The rejected string is intentionally not retained: attacker-controlled
    /// schema text must not propagate into error payloads or logs.
    #[error("schema version names an unknown kind")]
    UnknownSchema,
    /// The kind is known but this build does not accept the declared major.
    /// The rejected string is intentionally not retained: attacker-controlled
    /// schema text must not propagate into error payloads or logs.
    #[error("schema version declares an unsupported major")]
    UnsupportedMajor,
    /// A record identifier did not carry the exact prefix its requested
    /// `RecordKind` requires. The required prefix is [`RecordKind::prefix`],
    /// not stored redundantly on this error.
    #[error("record id does not have the {kind:?} prefix {:?}", kind.prefix())]
    WrongRecordPrefix {
        /// The kind whose prefix was required.
        kind: RecordKind,
    },
    /// An identifier was not shaped as `<prefix><26-character ULID>`, either
    /// because of a wrong-length suffix or trailing data after it.
    #[error("identifier is not shaped as <prefix> followed by a 26-character ULID")]
    MalformedIdentifier,
    /// The ULID suffix was not a canonical uppercase Crockford Base32 ULID.
    #[error("identifier suffix is not a canonical uppercase ULID")]
    InvalidUlid,
    /// A digest did not begin with the required `sha256:` prefix.
    #[error("digest does not start with \"sha256:\"")]
    UnsupportedDigestPrefix,
    /// A digest was not exactly 64 lowercase hex characters after the prefix.
    #[error("digest is not exactly 64 lowercase hex characters")]
    MalformedDigest,
    /// The value could not be serialized into canonical JSON. Serializer error
    /// text is intentionally not retained because custom serializers can emit
    /// attacker-controlled messages.
    #[error("value could not be canonicalized")]
    CanonicalizationFailed,
    /// A JSON integer was outside the exact range interoperable with IEEE-754
    /// implementations under I-JSON.
    #[error("JSON number is outside the interoperable safe-integer range")]
    NonInteroperableNumber,
    /// A known schema did not have its exact v1 field shape or valid values.
    #[error("invalid {schema:?} document shape at {field}")]
    InvalidShape {
        /// Schema whose document was rejected.
        schema: SchemaKind,
        /// Stable field name or validation category.
        field: &'static str,
    },
    /// A string did not name a member of a frozen enum vocabulary.
    #[error("unknown enum value for {schema:?}.{field}")]
    UnknownEnumValue {
        /// Schema containing the enum.
        schema: SchemaKind,
        /// Enum field.
        field: &'static str,
    },
    /// Cancellation evidence did not authorize the declared cancellation state.
    #[error("cancellation evidence does not match the execution binding")]
    CancellationEvidenceMismatch,
    /// The encoded document exceeded [`MAX_DOCUMENT_BYTES`].
    #[error("document exceeds the maximum encoded size")]
    DocumentTooLarge,
}

/// The fifteen kinds of record this build persists, each identified by a
/// stable four-character prefix baked into every [`crate::id::RecordId`] of
/// that kind.
///
/// `ExecutionBinding` is deliberately absent: [`SchemaKind::ExecutionBinding`]
/// maps onto `RecordKind::Attempt` (an execution binding *is* an attempt
/// record), so adding a separate variant here would give one record shape two
/// competing identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// `ids_` — a snapshot of an identity at a point in time.
    IdentitySnapshot,
    /// `int_` — a user or system intent.
    Intent,
    /// `grf_` — a graph of nodes.
    Graph,
    /// `nod_` — a single node within a graph.
    GraphNode,
    /// `att_` — an execution attempt (including execution bindings).
    Attempt,
    /// `dlg_` — a delegation of authority.
    Delegation,
    /// `bud_` — a budget allocation.
    Budget,
    /// `apr_` — an approval decision.
    Approval,
    /// `evd_` — evidence gathered in support of a decision.
    Evidence,
    /// `vrd_` — a verdict reached from evidence.
    Verdict,
    /// `rcv_` — a recovery action.
    Recovery,
    /// `adn_` — an addon registration.
    Addon,
    /// `sev_` — a surface event.
    SurfaceEvent,
    /// `sfx_` — a surface effect.
    SurfaceEffect,
    /// `del_` — a delivery record. Not to be confused with `dly_`, which this
    /// build never accepts.
    Delivery,
}

impl RecordKind {
    /// Every [`RecordKind`] this build defines, in declaration order.
    pub const ALL: [RecordKind; 15] = [
        RecordKind::IdentitySnapshot,
        RecordKind::Intent,
        RecordKind::Graph,
        RecordKind::GraphNode,
        RecordKind::Attempt,
        RecordKind::Delegation,
        RecordKind::Budget,
        RecordKind::Approval,
        RecordKind::Evidence,
        RecordKind::Verdict,
        RecordKind::Recovery,
        RecordKind::Addon,
        RecordKind::SurfaceEvent,
        RecordKind::SurfaceEffect,
        RecordKind::Delivery,
    ];

    /// The stable four-character prefix every `RecordId` of this kind begins
    /// with.
    pub const fn prefix(self) -> &'static str {
        match self {
            RecordKind::IdentitySnapshot => "ids_",
            RecordKind::Intent => "int_",
            RecordKind::Graph => "grf_",
            RecordKind::GraphNode => "nod_",
            RecordKind::Attempt => "att_",
            RecordKind::Delegation => "dlg_",
            RecordKind::Budget => "bud_",
            RecordKind::Approval => "apr_",
            RecordKind::Evidence => "evd_",
            RecordKind::Verdict => "vrd_",
            RecordKind::Recovery => "rcv_",
            RecordKind::Addon => "adn_",
            RecordKind::SurfaceEvent => "sev_",
            RecordKind::SurfaceEffect => "sfx_",
            RecordKind::Delivery => "del_",
        }
    }
}

/// The sixteen record/document shapes this build's schema registry knows
/// about, one of which (`Error`) never round-trips through a `RecordKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaKind {
    /// `psyche.identity_snapshot.vN`
    IdentitySnapshot,
    /// `psyche.intent.vN`
    Intent,
    /// `psyche.surface_event.vN`
    SurfaceEvent,
    /// `psyche.graph.vN`
    Graph,
    /// `psyche.graph_node.vN`
    GraphNode,
    /// `psyche.delegation.vN`
    Delegation,
    /// `psyche.budget.vN`
    Budget,
    /// `psyche.approval.vN`
    Approval,
    /// `psyche.execution_binding.vN` — maps to [`RecordKind::Attempt`].
    ExecutionBinding,
    /// `psyche.evidence.vN`
    Evidence,
    /// `psyche.verdict.vN`
    Verdict,
    /// `psyche.recovery.vN`
    Recovery,
    /// `psyche.addon.vN`
    Addon,
    /// `psyche.surface_effect.vN`
    SurfaceEffect,
    /// `psyche.delivery.vN`
    Delivery,
    /// `psyche.error.vN` — an error document. Never a stored record, so it
    /// has no corresponding `RecordKind`.
    Error,
}

impl SchemaKind {
    /// All sixteen kinds, in the order their canonical strings are listed in
    /// the registry.
    const ALL: [SchemaKind; 16] = [
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

    /// The `<kind>` segment of this kind's canonical `psyche.<kind>.vN`
    /// string.
    const fn name(self) -> &'static str {
        match self {
            SchemaKind::IdentitySnapshot => "identity_snapshot",
            SchemaKind::Intent => "intent",
            SchemaKind::SurfaceEvent => "surface_event",
            SchemaKind::Graph => "graph",
            SchemaKind::GraphNode => "graph_node",
            SchemaKind::Delegation => "delegation",
            SchemaKind::Budget => "budget",
            SchemaKind::Approval => "approval",
            SchemaKind::ExecutionBinding => "execution_binding",
            SchemaKind::Evidence => "evidence",
            SchemaKind::Verdict => "verdict",
            SchemaKind::Recovery => "recovery",
            SchemaKind::Addon => "addon",
            SchemaKind::SurfaceEffect => "surface_effect",
            SchemaKind::Delivery => "delivery",
            SchemaKind::Error => "error",
        }
    }

    /// The kind named by a `psyche.<kind>.vN` string's `<kind>` segment, if
    /// this build recognises it.
    fn from_name(name: &str) -> Option<SchemaKind> {
        SchemaKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The [`RecordKind`] this schema kind is stored as, or `None` for
    /// [`SchemaKind::Error`], which is never a stored record.
    ///
    /// This is the *sole* mapping from schema kind to record kind: nothing
    /// else in this crate re-derives it, so there is exactly one place that
    /// can disagree with itself.
    pub const fn record_kind(self) -> Option<RecordKind> {
        match self {
            SchemaKind::IdentitySnapshot => Some(RecordKind::IdentitySnapshot),
            SchemaKind::Intent => Some(RecordKind::Intent),
            SchemaKind::SurfaceEvent => Some(RecordKind::SurfaceEvent),
            SchemaKind::Graph => Some(RecordKind::Graph),
            SchemaKind::GraphNode => Some(RecordKind::GraphNode),
            SchemaKind::Delegation => Some(RecordKind::Delegation),
            SchemaKind::Budget => Some(RecordKind::Budget),
            SchemaKind::Approval => Some(RecordKind::Approval),
            SchemaKind::ExecutionBinding => Some(RecordKind::Attempt),
            SchemaKind::Evidence => Some(RecordKind::Evidence),
            SchemaKind::Verdict => Some(RecordKind::Verdict),
            SchemaKind::Recovery => Some(RecordKind::Recovery),
            SchemaKind::Addon => Some(RecordKind::Addon),
            SchemaKind::SurfaceEffect => Some(RecordKind::SurfaceEffect),
            SchemaKind::Delivery => Some(RecordKind::Delivery),
            SchemaKind::Error => None,
        }
    }
}

/// The major version this build accepts for every [`SchemaKind`] today.
///
/// A single constant, not a per-kind table: every kind in the registry is
/// presently at major 1, and a kind reaching major 2 is a deliberate,
/// reviewed change to this file rather than a silent range extension.
const SUPPORTED_MAJOR: u16 = 1;

/// A validated `psyche.<kind>.v<major>` contract schema version.
///
/// Fields are public and directly constructible: unlike [`crate::id::RecordId`]
/// there is no encoded invariant beyond "this kind and this major are both
/// individually meaningful values", so a caller building a schema version to
/// serialize (e.g. `SchemaVersion { kind: SchemaKind::Graph, major: 1 }`) does
/// not need to round-trip through string parsing to do it. [`SchemaVersion::parse`]
/// and the `serde` implementation are what enforce that the *string form* is
/// one this build's registry actually accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SchemaVersion {
    /// Which of the sixteen registry kinds this version names.
    pub kind: SchemaKind,
    /// The major version declared. This build only accepts `1`.
    pub major: u16,
}

impl SchemaVersion {
    /// Parses a `psyche.<kind>.v<major>` string against the registry.
    ///
    /// An unrecognised `<kind>` segment is [`ContractError::UnknownSchema`].
    /// A recognised kind whose major is not the one this build supports —
    /// including a malformed major segment, such as a leading zero or
    /// non-digit content — is [`ContractError::UnsupportedMajor`]: the
    /// registry has exactly two failure modes, and a garbled major on an
    /// otherwise-known kind is a version problem, not an unknown-kind one.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let unknown = || ContractError::UnknownSchema;
        let segments: Vec<&str> = value.split('.').collect();
        let [namespace, kind_segment, major_segment] = segments.as_slice() else {
            return Err(unknown());
        };
        if *namespace != "psyche" {
            return Err(unknown());
        }

        let kind = SchemaKind::from_name(kind_segment).ok_or_else(unknown)?;

        let unsupported_major = || ContractError::UnsupportedMajor;
        let digits = major_segment
            .strip_prefix('v')
            .ok_or_else(unsupported_major)?;
        // Reject a leading zero on a multi-digit major ("v01"): it parses to
        // the same integer as "v1" but is not the canonical string, and the
        // registry only accepts the canonical form.
        if digits.is_empty()
            || (digits.len() > 1 && digits.starts_with('0'))
            || !digits.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(unsupported_major());
        }
        let major: u16 = digits.parse().map_err(|_| unsupported_major())?;
        if major != SUPPORTED_MAJOR {
            return Err(unsupported_major());
        }
        Ok(SchemaVersion { kind, major })
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "psyche.{}.v{}", self.kind.name(), self.major)
    }
}

impl TryFrom<String> for SchemaVersion {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<SchemaVersion> for String {
    fn from(value: SchemaVersion) -> Self {
        value.to_string()
    }
}

impl FromStr for SchemaVersion {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// A persistable v1 domain record with a schema and durable record identifier.
pub trait VersionedRecord: Serialize {
    /// The record's declared schema.
    fn schema_version(&self) -> SchemaVersion;
    /// The record's durable identifier.
    fn record_id(&self) -> &RecordId;
}

/// Every canonical document accepted by this build.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum CanonicalDocument {
    /// Identity snapshot.
    IdentitySnapshot(IdentitySnapshot),
    /// Intent.
    Intent(Intent),
    /// Surface event.
    SurfaceEvent(SurfaceEvent),
    /// Graph.
    Graph(Graph),
    /// Graph node.
    GraphNode(GraphNode),
    /// Delegation.
    Delegation(Delegation),
    /// Budget.
    Budget(Budget),
    /// Approval.
    Approval(Approval),
    /// Execution binding, persisted as an attempt.
    ExecutionBinding(ExecutionBinding),
    /// Evidence.
    Evidence(Evidence),
    /// Verdict.
    Verdict(Verdict),
    /// Recovery.
    Recovery(Recovery),
    /// Add-on.
    Addon(Addon),
    /// Surface effect.
    SurfaceEffect(SurfaceEffect),
    /// Delivery.
    Delivery(Delivery),
    /// Non-persistable typed error envelope.
    Error(ErrorEnvelope),
}

impl CanonicalDocument {
    /// Revalidates a decoded or directly constructed value.
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::IdentitySnapshot(v) => v.validate(),
            Self::Intent(v) => v.validate(),
            Self::SurfaceEvent(v) => v.validate(),
            Self::Graph(v) => v.validate(),
            Self::GraphNode(v) => v.validate(),
            Self::Delegation(v) => v.validate(),
            Self::Budget(v) => v.validate(),
            Self::Approval(v) => v.validate(),
            Self::ExecutionBinding(v) => v.validate(),
            Self::Evidence(v) => v.validate(),
            Self::Verdict(v) => v.validate(),
            Self::Recovery(v) => v.validate(),
            Self::Addon(v) => v.validate(),
            Self::SurfaceEffect(v) => v.validate(),
            Self::Delivery(v) => v.validate(),
            Self::Error(v) => v.validate(),
        }?;
        if crate::digest::canonical_bytes(self)?.len() > MAX_DOCUMENT_BYTES {
            return Err(ContractError::DocumentTooLarge);
        }
        Ok(())
    }

    /// Declared schema version.
    pub fn schema_version(&self) -> SchemaVersion {
        match self {
            Self::IdentitySnapshot(v) => v.schema_version(),
            Self::Intent(v) => v.schema_version(),
            Self::SurfaceEvent(v) => v.schema_version(),
            Self::Graph(v) => v.schema_version(),
            Self::GraphNode(v) => v.schema_version(),
            Self::Delegation(v) => v.schema_version(),
            Self::Budget(v) => v.schema_version(),
            Self::Approval(v) => v.schema_version(),
            Self::ExecutionBinding(v) => v.schema_version(),
            Self::Evidence(v) => v.schema_version(),
            Self::Verdict(v) => v.schema_version(),
            Self::Recovery(v) => v.schema_version(),
            Self::Addon(v) => v.schema_version(),
            Self::SurfaceEffect(v) => v.schema_version(),
            Self::Delivery(v) => v.schema_version(),
            Self::Error(v) => v.schema_version,
        }
    }

    /// Durable record ID, or `None` for an error envelope.
    pub fn persistable_record_id(&self) -> Option<&RecordId> {
        match self {
            Self::IdentitySnapshot(v) => Some(v.record_id()),
            Self::Intent(v) => Some(v.record_id()),
            Self::SurfaceEvent(v) => Some(v.record_id()),
            Self::Graph(v) => Some(v.record_id()),
            Self::GraphNode(v) => Some(v.record_id()),
            Self::Delegation(v) => Some(v.record_id()),
            Self::Budget(v) => Some(v.record_id()),
            Self::Approval(v) => Some(v.record_id()),
            Self::ExecutionBinding(v) => Some(v.record_id()),
            Self::Evidence(v) => Some(v.record_id()),
            Self::Verdict(v) => Some(v.record_id()),
            Self::Recovery(v) => Some(v.record_id()),
            Self::Addon(v) => Some(v.record_id()),
            Self::SurfaceEffect(v) => Some(v.record_id()),
            Self::Delivery(v) => Some(v.record_id()),
            Self::Error(_) => None,
        }
    }
}

/// Strictly decodes one known v1 canonical document.
pub fn decode_document(bytes: &[u8]) -> Result<CanonicalDocument, ContractError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(ContractError::DocumentTooLarge);
    }
    let value = strict_json(bytes)?;
    crate::digest::validate_json_domain(&value)?;
    let schema_text = value
        .as_object()
        .and_then(|v| v.get("schema_version"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(SchemaKind::Error, "schema_version"))?;
    let schema = SchemaVersion::parse(schema_text)?;
    let document = match schema.kind {
        SchemaKind::IdentitySnapshot => {
            decode(value, CanonicalDocument::IdentitySnapshot, schema.kind)?
        }
        SchemaKind::Intent => decode(value, CanonicalDocument::Intent, schema.kind)?,
        SchemaKind::SurfaceEvent => decode(value, CanonicalDocument::SurfaceEvent, schema.kind)?,
        SchemaKind::Graph => decode(value, CanonicalDocument::Graph, schema.kind)?,
        SchemaKind::GraphNode => decode(value, CanonicalDocument::GraphNode, schema.kind)?,
        SchemaKind::Delegation => decode(value, CanonicalDocument::Delegation, schema.kind)?,
        SchemaKind::Budget => decode(value, CanonicalDocument::Budget, schema.kind)?,
        SchemaKind::Approval => decode(value, CanonicalDocument::Approval, schema.kind)?,
        SchemaKind::ExecutionBinding => {
            CanonicalDocument::ExecutionBinding(ExecutionBinding::decode(value)?)
        }
        SchemaKind::Evidence => decode(value, CanonicalDocument::Evidence, schema.kind)?,
        SchemaKind::Verdict => decode(value, CanonicalDocument::Verdict, schema.kind)?,
        SchemaKind::Recovery => decode(value, CanonicalDocument::Recovery, schema.kind)?,
        SchemaKind::Addon => decode(value, CanonicalDocument::Addon, schema.kind)?,
        SchemaKind::SurfaceEffect => decode(value, CanonicalDocument::SurfaceEffect, schema.kind)?,
        SchemaKind::Delivery => decode(value, CanonicalDocument::Delivery, schema.kind)?,
        SchemaKind::Error => {
            return ErrorEnvelope::decode(value).map(CanonicalDocument::Error);
        }
    };
    document.validate()?;
    Ok(document)
}

fn strict_json(bytes: &[u8]) -> Result<Value, ContractError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValueSeed { depth: 0 }
        .deserialize(&mut deserializer)
        .map_err(|_| invalid(SchemaKind::Error, "json"))?;
    deserializer
        .end()
        .map_err(|_| invalid(SchemaKind::Error, "json"))?;
    Ok(value)
}

#[derive(Clone, Copy)]
struct StrictValueSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        if self.depth > MAX_JSON_DEPTH {
            return Err(serde::de::Error::custom("JSON nesting limit exceeded"));
        }
        deserializer.deserialize_any(StrictValueVisitor { depth: self.depth })
    }
}

struct StrictValueVisitor {
    depth: usize,
}

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_i128<E: serde::de::Error>(self, value: i128) -> Result<Self::Value, E> {
        serde_json::Number::from_i128(value)
            .map(Value::Number)
            .ok_or_else(|| serde::de::Error::custom("invalid JSON number"))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u128<E: serde::de::Error>(self, value: u128) -> Result<Self::Value, E> {
        serde_json::Number::from_u128(value)
            .map(Value::Number)
            .ok_or_else(|| serde::de::Error::custom("invalid JSON number"))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| serde::de::Error::custom("non-finite JSON number"))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        StrictValueSeed { depth: self.depth }.deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValueSeed {
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        if serde_json_number::is_private_number_map::<A>() {
            let key = object.next_key::<String>()?;
            if key.as_deref() != Some(serde_json_number::TOKEN) {
                return Err(serde::de::Error::custom(
                    "malformed arbitrary-precision number",
                ));
            }
            let text = object.next_value::<String>()?;
            if object.next_key::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(
                    "malformed arbitrary-precision number",
                ));
            }
            return serde_json_number::parse_exact(&text)
                .map(Value::Number)
                .ok_or_else(|| serde::de::Error::custom("malformed arbitrary-precision number"));
        }

        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if key == serde_json_number::TOKEN || values.contains_key(&key) {
                return Err(serde::de::Error::custom("invalid JSON object key"));
            }
            let value = object.next_value_seed(StrictValueSeed {
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

fn decode<T: serde::de::DeserializeOwned>(
    value: Value,
    wrap: impl FnOnce(T) -> CanonicalDocument,
    schema: SchemaKind,
) -> Result<CanonicalDocument, ContractError> {
    serde_json::from_value(value)
        .map(wrap)
        .map_err(|_| invalid(schema, "document"))
}

pub(crate) fn invalid(schema: SchemaKind, field: &'static str) -> ContractError {
    ContractError::InvalidShape { schema, field }
}

pub(crate) fn require_schema(value: SchemaVersion, kind: SchemaKind) -> Result<(), ContractError> {
    if value.kind == kind && value.major == 1 {
        Ok(())
    } else {
        Err(invalid(kind, "schema_version"))
    }
}

pub(crate) fn require_id(
    id: &RecordId,
    kind: RecordKind,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    if id.kind() == kind {
        Ok(())
    } else {
        Err(invalid(schema, field))
    }
}

pub(crate) fn bounded(
    value: &str,
    max: usize,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    if !value.is_empty() && value.len() <= max {
        Ok(())
    } else {
        Err(invalid(schema, field))
    }
}

pub(crate) fn safe_integer(
    value: u64,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    if value <= MAX_SAFE_INTEGER {
        Ok(())
    } else {
        Err(invalid(schema, field))
    }
}

pub(crate) fn optional_bounded(
    value: &Option<String>,
    max: usize,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    value
        .as_deref()
        .map_or(Ok(()), |v| bounded(v, max, schema, field))
}

pub(crate) fn string_list(
    values: &[String],
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    if values.len() > 1024 {
        return Err(invalid(schema, field));
    }
    values
        .iter()
        .try_for_each(|v| bounded(v, 256, schema, field))
}

pub(crate) fn object(
    value: &Value,
    schema: SchemaKind,
    field: &'static str,
    nonempty: bool,
) -> Result<(), ContractError> {
    let Some(map) = value.as_object() else {
        return Err(invalid(schema, field));
    };
    if nonempty && map.is_empty() {
        return Err(invalid(schema, field));
    }
    if crate::digest::canonical_bytes(value)?.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid(schema, field));
    }
    Ok(())
}

pub(crate) fn reason_code(
    value: &str,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    let mut segments = value.split('_');
    let first = segments.next().unwrap_or_default();
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    };
    if value.len() <= 128
        && first.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && valid_segment(first)
        && segments.all(valid_segment)
    {
        Ok(())
    } else {
        Err(invalid(schema, field))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Attacker-input-redaction tests: these pin the payload-light contract for
    // UnknownSchema and UnsupportedMajor. A nearly-1-MiB schema string with a
    // unique control marker must NOT appear in Debug or Display output.
    #[test]
    fn unknown_schema_error_does_not_expose_attacker_input() {
        let marker = "SENTINEL_UNKNOWN_XYZ";
        // Large unknown kind segment with embedded control marker → UnknownSchema
        let attacker = format!("psyche.{}{}.v1", marker, "a".repeat(900_000));
        let err = SchemaVersion::parse(&attacker).unwrap_err();
        assert!(
            matches!(err, ContractError::UnknownSchema),
            "expected UnknownSchema, got {err:?}"
        );
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert!(
            !debug.contains(marker),
            "Debug must not contain attacker marker (output len = {})",
            debug.len()
        );
        assert!(
            !display.contains(marker),
            "Display must not contain attacker marker (output len = {})",
            display.len()
        );
        assert!(debug.len() < 256);
        assert!(display.len() < 256);
    }

    #[test]
    fn unsupported_major_error_does_not_expose_attacker_input() {
        let marker = "SENTINEL_MAJOR_XYZ";
        // Known kind, non-digit marker in major segment → UnsupportedMajor
        let attacker = format!("psyche.intent.v{}{}", marker, "9".repeat(900_000));
        let err = SchemaVersion::parse(&attacker).unwrap_err();
        assert!(
            matches!(err, ContractError::UnsupportedMajor),
            "expected UnsupportedMajor, got {err:?}"
        );
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert!(
            !debug.contains(marker),
            "Debug must not contain attacker marker (output len = {})",
            debug.len()
        );
        assert!(
            !display.contains(marker),
            "Display must not contain attacker marker (output len = {})",
            display.len()
        );
        assert!(debug.len() < 256);
        assert!(display.len() < 256);
    }

    #[test]
    fn record_kind_all_has_exactly_fifteen_entries() {
        assert_eq!(RecordKind::ALL.len(), 15);
    }

    #[test]
    fn execution_binding_maps_to_attempt_only() {
        assert_eq!(
            SchemaKind::ExecutionBinding.record_kind(),
            Some(RecordKind::Attempt)
        );
        assert_eq!(RecordKind::Attempt.prefix(), "att_");
    }

    #[test]
    fn schema_kind_error_has_no_record_kind() {
        assert_eq!(SchemaKind::Error.record_kind(), None);
    }

    #[test]
    fn schema_version_accepts_exactly_the_sixteen_known_strings() {
        for known in [
            "psyche.identity_snapshot.v1",
            "psyche.intent.v1",
            "psyche.surface_event.v1",
            "psyche.graph.v1",
            "psyche.graph_node.v1",
            "psyche.delegation.v1",
            "psyche.budget.v1",
            "psyche.approval.v1",
            "psyche.execution_binding.v1",
            "psyche.evidence.v1",
            "psyche.verdict.v1",
            "psyche.recovery.v1",
            "psyche.addon.v1",
            "psyche.surface_effect.v1",
            "psyche.delivery.v1",
            "psyche.error.v1",
        ] {
            let parsed = SchemaVersion::parse(known).unwrap_or_else(|err| {
                panic!("expected {known:?} to parse, got {err:?}");
            });
            assert_eq!(parsed.to_string(), known);
        }
    }

    #[test]
    fn schema_version_rejects_an_unknown_kind() {
        let err = SchemaVersion::parse("psyche.unknown_kind.v1").unwrap_err();
        assert!(matches!(err, ContractError::UnknownSchema));
    }

    #[test]
    fn schema_version_rejects_a_known_kind_with_the_wrong_major() {
        let err = SchemaVersion::parse("psyche.intent.v2").unwrap_err();
        assert!(matches!(err, ContractError::UnsupportedMajor));
    }

    #[test]
    fn schema_version_try_from_string_validates() {
        let ok = SchemaVersion::try_from("psyche.intent.v1".to_string()).unwrap();
        assert_eq!(ok.kind, SchemaKind::Intent);
        assert_eq!(ok.major, 1);
        assert!(SchemaVersion::try_from("psyche.intent.v9".to_string()).is_err());
    }

    #[test]
    fn schema_version_serde_round_trips_and_rejects() {
        let json =
            serde_json::to_string(&SchemaVersion::parse("psyche.graph.v1").unwrap()).unwrap();
        assert_eq!(json, "\"psyche.graph.v1\"");
        let back: SchemaVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_string(), "psyche.graph.v1");

        assert!(serde_json::from_str::<SchemaVersion>("\"psyche.graph.v2\"").is_err());
        assert!(serde_json::from_str::<SchemaVersion>("\"psyche.nope.v1\"").is_err());
    }

    // Mirrors schema.rs's denies_near_misses and secret.rs's rejects_near_misses:
    // without these, someone "helpfully" adding .trim(), case-insensitive
    // matching, or leading-zero tolerance would break registry strictness
    // silently.
    #[test]
    fn schema_version_denies_near_misses() {
        for (near, expect_unknown) in [
            ("", true),
            ("psyche.intent", true),       // missing major segment
            ("psyche.intent.v1.v2", true), // extra segment
            ("Psyche.intent.v1", true),    // wrong namespace case
            ("psyche.Intent.v1", true),    // wrong kind case
            ("psyche.intent.V1", false),   // wrong major case: known kind, bad major
            (" psyche.intent.v1", true),   // leading whitespace on namespace
            ("psyche.intent.v1 ", false),  // trailing whitespace: known kind, bad major
            ("psyche.intent.v01", false),  // leading zero: known kind, bad major
            ("psyche.intent.v", false),    // empty digits: known kind, bad major
            ("psyche.intent.v1x", false),  // trailing junk: known kind, bad major
            ("psyche.intent.v-1", false),  // negative: known kind, bad major
        ] {
            let err = SchemaVersion::parse(near).unwrap_err();
            if expect_unknown {
                assert!(
                    matches!(err, ContractError::UnknownSchema),
                    "expected UnknownSchema for {near:?}, got {err:?}"
                );
            } else {
                assert!(
                    matches!(err, ContractError::UnsupportedMajor),
                    "expected UnsupportedMajor for {near:?}, got {err:?}"
                );
            }
        }
    }
}
