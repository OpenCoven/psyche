//! Surface observation, effect, and delivery contracts.
#![allow(missing_docs)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contracts::{
    ContractError, RecordKind, SchemaKind, SchemaVersion, VersionedRecord, bounded, object,
    require_id, require_schema,
};
use crate::digest::{Sha256Digest, digest};
use crate::id::RecordId;

validated_struct! {
    pub struct SurfaceEvent, SurfaceEventWire {
        pub schema_version: SchemaVersion,
        pub surface_event_id: RecordId,
        pub adapter_id: String,
        pub account_id: String,
        pub actor: Value,
        pub locator: Value,
        pub adapter_event_digest: Sha256Digest,
        #[serde(with = "time::serde::rfc3339")]
        pub received_at: time::OffsetDateTime,
        pub content: Value,
    }
}

impl SurfaceEvent {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::SurfaceEvent;
        require_schema(self.schema_version, s)?;
        require_id(
            &self.surface_event_id,
            RecordKind::SurfaceEvent,
            s,
            "surface_event_id",
        )?;
        bounded(&self.adapter_id, 256, s, "adapter_id")?;
        bounded(&self.account_id, 256, s, "account_id")?;
        object(&self.actor, s, "actor", false)?;
        object(&self.locator, s, "locator", false)?;
        object(&self.content, s, "content", false)?;
        Ok(())
    }
}

impl VersionedRecord for SurfaceEvent {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.surface_event_id
    }
}

validated_struct! {
    pub struct SurfaceEffect, SurfaceEffectWire {
        pub schema_version: SchemaVersion,
        pub surface_effect_id: RecordId,
        pub intent_id: RecordId,
        pub graph_id: RecordId,
        pub node_id: RecordId,
        pub attempt_id: RecordId,
        pub familiar_snapshot_id: RecordId,
        pub project_id: String,
        pub action_class: String,
        pub account_id: String,
        pub locator: Value,
        pub effect: Value,
        pub effect_digest: Sha256Digest,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: time::OffsetDateTime,
    }
}

impl SurfaceEffect {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::SurfaceEffect;
        require_schema(self.schema_version, s)?;
        for (id, kind, field) in [
            (
                &self.surface_effect_id,
                RecordKind::SurfaceEffect,
                "surface_effect_id",
            ),
            (&self.intent_id, RecordKind::Intent, "intent_id"),
            (&self.graph_id, RecordKind::Graph, "graph_id"),
            (&self.node_id, RecordKind::GraphNode, "node_id"),
            (&self.attempt_id, RecordKind::Attempt, "attempt_id"),
            (
                &self.familiar_snapshot_id,
                RecordKind::IdentitySnapshot,
                "familiar_snapshot_id",
            ),
        ] {
            require_id(id, kind, s, field)?;
        }
        for (value, field) in [
            (&self.project_id, "project_id"),
            (&self.action_class, "action_class"),
            (&self.account_id, "account_id"),
        ] {
            bounded(value, 256, s, field)?;
        }
        object(&self.locator, s, "locator", false)?;
        object(&self.effect, s, "effect", false)?;
        if digest(&self.effect)? != self.effect_digest {
            return Err(super::invalid(s, "effect_digest"));
        }
        Ok(())
    }
}

impl VersionedRecord for SurfaceEffect {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.surface_effect_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryTopic {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryRelationship {
    ReplySameDm,
    ReplySameGroup,
    ReplySameTopic,
    CrossChat,
    Broadcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryDecisionState {
    Reserved,
    Consumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliverySurfaceDecision {
    pub decision_id: String,
    pub request_digest: Sha256Digest,
    pub policy_revision: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: time::OffsetDateTime,
    pub state: DeliveryDecisionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Ready,
    Sending,
    Sent,
    Retryable,
    DeliveryUnknown,
    Failed,
    Abandoned,
    DeadLetter,
    ResolvingUnknown,
    Compensated,
}

validated_struct! {
    pub struct Delivery, DeliveryWire {
        pub schema_version: SchemaVersion,
        pub delivery_id: RecordId,
        pub intent_id: RecordId,
        pub action_class: String,
        pub account_id: String,
        pub chat_id: String,
        pub topic: DeliveryTopic,
        pub relationship: DeliveryRelationship,
        pub effect: Value,
        pub effect_digest: Sha256Digest,
        pub surface_decision: DeliverySurfaceDecision,
        pub logical_response_id: String,
        pub logical_part: u32,
        pub state: DeliveryState,
        pub attempt_count: u32,
        pub telegram_message_id: Option<String>,
    }
}

impl Delivery {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Delivery;
        require_schema(self.schema_version, s)?;
        require_id(&self.delivery_id, RecordKind::Delivery, s, "delivery_id")?;
        require_id(&self.intent_id, RecordKind::Intent, s, "intent_id")?;
        for (value, field) in [
            (&self.action_class, "action_class"),
            (&self.account_id, "account_id"),
            (&self.topic.kind, "topic.kind"),
            (&self.topic.id, "topic.id"),
            (
                &self.surface_decision.policy_revision,
                "surface_decision.policy_revision",
            ),
            (&self.logical_response_id, "logical_response_id"),
            (
                &self.surface_decision.decision_id,
                "surface_decision.decision_id",
            ),
        ] {
            bounded(value, 256, s, field)?;
        }
        decimal(&self.chat_id, true, s, "chat_id")?;
        if let Some(id) = &self.telegram_message_id {
            decimal(id, false, s, "telegram_message_id")?;
        }
        object(&self.effect, s, "effect", true)?;
        if digest(&self.effect)? != self.effect_digest {
            return Err(super::invalid(s, "effect_digest"));
        }
        if (self.state == DeliveryState::Sent) != self.telegram_message_id.is_some() {
            return Err(super::invalid(s, "telegram_message_id"));
        }
        Ok(())
    }
}

fn decimal(
    value: &str,
    signed: bool,
    schema: SchemaKind,
    field: &'static str,
) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > 32 {
        return Err(super::invalid(schema, field));
    }
    let digits = if signed {
        value.strip_prefix('-').unwrap_or(value)
    } else {
        value
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        Err(super::invalid(schema, field))
    } else {
        Ok(())
    }
}

impl VersionedRecord for Delivery {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.delivery_id
    }
}
