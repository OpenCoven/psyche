//! ADR 4 wire record for the first Build profile.
//!
//! A binding correlates evidence. Decoding it does not establish the issuer or
//! authenticate the separately obtained execution request or native receipt.

use std::{fmt, marker::PhantomData};

use serde::de::{MapAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};

use crate::digest::Sha256Digest;
use crate::id::RequestId;

/// Negotiated wire version, distinct from the historic G2 store registry.
pub const SCHEMA_VERSION: &str = "psyche.build_receipt_binding.v1";
/// Only this externally trusted signing principal can issue the v1 binding.
pub const ISSUER: &str = "psyche-receipt-issuer";
/// SSHSIG domain dedicated to canonical bytes of this binding version.
pub const SIGNATURE_NAMESPACE: &str = "psyche.build-receipt-binding.v1";

/// Native journal disposition, independent of whether a terminal action succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", rename_all = "snake_case")]
pub enum NativeOutcome {
    /// The authoritative effect path produced a durable terminal receipt.
    Terminal,
    /// The outcome requires canonical reconciliation.
    Ambiguous,
}

impl TryFrom<String> for NativeOutcome {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "terminal" => Ok(Self::Terminal),
            "ambiguous" => Ok(Self::Ambiguous),
            _ => Err("unknown native outcome"),
        }
    }
}

/// Opaque Build identifier. It cannot confer authority or replace a Psyche ID.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct BuildReference(String);

impl TryFrom<String> for BuildReference {
    type Error = BindingError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if (1..=255).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic()) {
            Ok(Self(value))
        } else {
            Err(BindingError::InvalidReference)
        }
    }
}

impl<'de> Deserialize<'de> for BuildReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl BuildReference {
    /// Deserialize an optional reference that must be a string when present.
    pub fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Self>, D::Error> {
        Self::deserialize(d).map(Some)
    }
}

/// Decode a named-field profile object, retaining duplicate fields so the
/// target's derived deserializer can reject them.
pub fn deserialize_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Serde's ordinary struct visitor also accepts positional arrays.
    struct ObjectVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a named-field profile object")
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(PhantomData))
}

validated_struct! {
    /// Closed v1 wire fields. Deserialization validates the profile shape.
    /// Authenticating the issuer and correlating external evidence are separate steps.
    pub struct BuildReceiptBinding, BuildReceiptBindingWire using deserialize_object {
        /// Exact negotiated schema version.
        pub schema_version: String,
        /// Issuer principal authenticated using independently provisioned trust.
        pub issuer: String,
        /// Psyche execution request identity.
        pub request_id: RequestId,
        /// Digest of the canonical typed execution request.
        pub request_digest: Sha256Digest,
        /// Opaque native task reference.
        pub task_id: BuildReference,
        /// Opaque native action reference, when supplied.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "BuildReference::present"
        )]
        pub action_id: Option<BuildReference>,
        /// Opaque native idempotency reference, when supplied.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "BuildReference::present"
        )]
        pub idempotency_key: Option<BuildReference>,
        /// SHA-256 of the exact native receipt bytes.
        pub receipt_digest: Sha256Digest,
        /// Disposition attested by the issuer.
        pub outcome: NativeOutcome,
    }
}

/// Structural validation errors for the first Build receipt-binding profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BindingError {
    /// The wire version is outside this profile.
    #[error("unsupported binding version")]
    UnsupportedVersion,
    /// The declared issuer differs from the fixed profile principal.
    #[error("unexpected binding issuer")]
    UnexpectedIssuer,
    /// Both native action references were omitted.
    #[error("missing Build action or idempotency reference")]
    MissingReference,
    /// A Build reference exceeded the profile limits or contained invalid bytes.
    #[error("invalid Build reference")]
    InvalidReference,
}

impl BuildReceiptBinding {
    /// Validate the profile shape before signing a locally constructed record.
    pub fn validate(&self) -> Result<(), BindingError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(BindingError::UnsupportedVersion);
        }
        if self.issuer != ISSUER {
            return Err(BindingError::UnexpectedIssuer);
        }
        if self.action_id.is_none() && self.idempotency_key.is_none() {
            return Err(BindingError::MissingReference);
        }
        Ok(())
    }
}
