//! Typed public error envelope.
#![allow(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::contracts::{
    ContractError, SchemaKind, SchemaVersion, bounded, invalid, require_schema,
};

/// Maximum UTF-8 bytes in a public error message.
pub const MAX_ERROR_MESSAGE_BYTES: usize = 4096;
/// Maximum UTF-8 bytes in a public error correlation identifier.
pub const MAX_ERROR_CORRELATION_ID_BYTES: usize = 255;
/// Maximum public classification entries in an error details map.
pub const MAX_ERROR_DETAILS_ENTRIES: usize = 128;
/// Maximum UTF-8 bytes in a nonempty error details key.
pub const MAX_ERROR_DETAIL_KEY_BYTES: usize = 256;
/// Maximum UTF-8 bytes in a nonempty error details value.
pub const MAX_ERROR_DETAIL_VALUE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    ConfigInvalid,
    SecretUnavailable,
    TelegramUnauthorized,
    TelegramBotIdentityMismatch,
    TelegramConflict,
    TelegramRateLimited,
    TelegramUnavailable,
    WebhookAuthFailed,
    StorageUnavailable,
    EventSchemaUnsupported,
    PrincipalMappingInvalid,
    GraphInvalid,
    DelegationWidened,
    BudgetUnenforceable,
    EvidenceIncomplete,
    VerdictInvalid,
    RouteNotFound,
    RouteAmbiguous,
    SenderUnauthorized,
    IdentityInvalid,
    IdentityChanged,
    CovenUnavailable,
    CovenVersionUnsupported,
    CovenCapabilityMissing,
    CovenPolicyDenied,
    CovenExecutionBindingInvalid,
    CovenBindingMismatch,
    CovenArtifactRejected,
    CovenIntentConflict,
    CovenAdoptionUnknown,
    CovenCancellationUnknown,
    CovenSessionFailed,
    DeliveryUnknown,
    PreviewFinalizeBlocked,
    MediaRejected,
    CallbackInvalid,
}

impl ErrorCode {
    pub const ALL: [Self; 36] = [
        Self::ConfigInvalid,
        Self::SecretUnavailable,
        Self::TelegramUnauthorized,
        Self::TelegramBotIdentityMismatch,
        Self::TelegramConflict,
        Self::TelegramRateLimited,
        Self::TelegramUnavailable,
        Self::WebhookAuthFailed,
        Self::StorageUnavailable,
        Self::EventSchemaUnsupported,
        Self::PrincipalMappingInvalid,
        Self::GraphInvalid,
        Self::DelegationWidened,
        Self::BudgetUnenforceable,
        Self::EvidenceIncomplete,
        Self::VerdictInvalid,
        Self::RouteNotFound,
        Self::RouteAmbiguous,
        Self::SenderUnauthorized,
        Self::IdentityInvalid,
        Self::IdentityChanged,
        Self::CovenUnavailable,
        Self::CovenVersionUnsupported,
        Self::CovenCapabilityMissing,
        Self::CovenPolicyDenied,
        Self::CovenExecutionBindingInvalid,
        Self::CovenBindingMismatch,
        Self::CovenArtifactRejected,
        Self::CovenIntentConflict,
        Self::CovenAdoptionUnknown,
        Self::CovenCancellationUnknown,
        Self::CovenSessionFailed,
        Self::DeliveryUnknown,
        Self::PreviewFinalizeBlocked,
        Self::MediaRejected,
        Self::CallbackInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInvalid => "config_invalid",
            Self::SecretUnavailable => "secret_unavailable",
            Self::TelegramUnauthorized => "telegram_unauthorized",
            Self::TelegramBotIdentityMismatch => "telegram_bot_identity_mismatch",
            Self::TelegramConflict => "telegram_conflict",
            Self::TelegramRateLimited => "telegram_rate_limited",
            Self::TelegramUnavailable => "telegram_unavailable",
            Self::WebhookAuthFailed => "webhook_auth_failed",
            Self::StorageUnavailable => "storage_unavailable",
            Self::EventSchemaUnsupported => "event_schema_unsupported",
            Self::PrincipalMappingInvalid => "principal_mapping_invalid",
            Self::GraphInvalid => "graph_invalid",
            Self::DelegationWidened => "delegation_widened",
            Self::BudgetUnenforceable => "budget_unenforceable",
            Self::EvidenceIncomplete => "evidence_incomplete",
            Self::VerdictInvalid => "verdict_invalid",
            Self::RouteNotFound => "route_not_found",
            Self::RouteAmbiguous => "route_ambiguous",
            Self::SenderUnauthorized => "sender_unauthorized",
            Self::IdentityInvalid => "identity_invalid",
            Self::IdentityChanged => "identity_changed",
            Self::CovenUnavailable => "coven_unavailable",
            Self::CovenVersionUnsupported => "coven_version_unsupported",
            Self::CovenCapabilityMissing => "coven_capability_missing",
            Self::CovenPolicyDenied => "coven_policy_denied",
            Self::CovenExecutionBindingInvalid => "coven_execution_binding_invalid",
            Self::CovenBindingMismatch => "coven_binding_mismatch",
            Self::CovenArtifactRejected => "coven_artifact_rejected",
            Self::CovenIntentConflict => "coven_intent_conflict",
            Self::CovenAdoptionUnknown => "coven_adoption_unknown",
            Self::CovenCancellationUnknown => "coven_cancellation_unknown",
            Self::CovenSessionFailed => "coven_session_failed",
            Self::DeliveryUnknown => "delivery_unknown",
            Self::PreviewFinalizeBlocked => "preview_finalize_blocked",
            Self::MediaRejected => "media_rejected",
            Self::CallbackInvalid => "callback_invalid",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| serde::de::Error::custom("unknown error code"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: String,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorEnvelope {
    pub schema_version: SchemaVersion,
    pub error: ErrorBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelopeWire {
    schema_version: SchemaVersion,
    error: ErrorBodyWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorBodyWire {
    code: String,
    message: String,
    retryable: bool,
    correlation_id: String,
    #[serde(deserialize_with = "super::strict_string_map::deserialize")]
    details: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedErrorEnvelopeWire {
    schema_version: SchemaVersion,
    error: TypedErrorBodyWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedErrorBodyWire {
    code: ErrorCode,
    message: String,
    retryable: bool,
    correlation_id: String,
    #[serde(deserialize_with = "super::strict_string_map::deserialize")]
    details: BTreeMap<String, String>,
}

impl<'de> Deserialize<'de> for ErrorEnvelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = TypedErrorEnvelopeWire::deserialize(deserializer)?;
        let envelope = Self {
            schema_version: wire.schema_version,
            error: ErrorBody {
                code: wire.error.code,
                message: wire.error.message,
                retryable: wire.error.retryable,
                correlation_id: wire.error.correlation_id,
                details: wire.error.details,
            },
        };
        envelope.validate().map_err(serde::de::Error::custom)?;
        Ok(envelope)
    }
}

impl ErrorEnvelope {
    pub(crate) fn decode(value: Value) -> Result<Self, ContractError> {
        let wire: ErrorEnvelopeWire =
            serde_json::from_value(value).map_err(|_| invalid(SchemaKind::Error, "document"))?;
        let code = ErrorCode::parse(&wire.error.code).ok_or(ContractError::UnknownEnumValue {
            schema: SchemaKind::Error,
            field: "code",
        })?;
        let envelope = Self {
            schema_version: wire.schema_version,
            error: ErrorBody {
                code,
                message: wire.error.message,
                retryable: wire.error.retryable,
                correlation_id: wire.error.correlation_id,
                details: wire.error.details,
            },
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Error;
        require_schema(self.schema_version, s)?;
        bounded(&self.error.message, MAX_ERROR_MESSAGE_BYTES, s, "message")?;
        bounded(
            &self.error.correlation_id,
            MAX_ERROR_CORRELATION_ID_BYTES,
            s,
            "correlation_id",
        )?;
        if self.error.details.len() > MAX_ERROR_DETAILS_ENTRIES {
            return Err(invalid(s, "details"));
        }
        for (key, value) in &self.error.details {
            bounded(key, MAX_ERROR_DETAIL_KEY_BYTES, s, "details.key")?;
            bounded(value, MAX_ERROR_DETAIL_VALUE_BYTES, s, "details.value")?;
        }
        Ok(())
    }
}
