//! Reusable surface-boundary assertions.

use psyche_core::contracts::surface::SurfaceEffect;
use psyche_core::contracts::{RecordKind, SchemaVersion};
use psyche_core::digest::digest;
use psyche_core::id::RecordId;
use psyche_surfaces::{DeliveryDisposition, SurfacePort};

use crate::surface::FakeSurface;

/// Builds the deterministic surface fixture used by the G2 wrapper.
pub fn scripted_surface() -> FakeSurface {
    match FakeSurface::builder()
        .delivery(DeliveryDisposition::Unknown)
        .build()
    {
        Ok(surface) => surface,
        Err(error) => panic!("static surface fixture is valid: {error}"),
    }
}

/// Verifies that an ambiguous delivery remains explicitly unknown and replay-safe.
pub async fn assert_surface_unknown_delivery(port: &dyn SurfacePort) {
    let effect = surface_effect();
    assert_eq!(
        port.apply(effect.clone()).await,
        Ok(DeliveryDisposition::Unknown)
    );
    assert_eq!(port.apply(effect).await, Ok(DeliveryDisposition::Unknown));
}

fn surface_effect() -> SurfaceEffect {
    let effect = serde_json::json!({"method": "send_message", "text": "hello"});
    SurfaceEffect {
        schema_version: schema("psyche.surface_effect.v1"),
        surface_effect_id: record_id(RecordKind::SurfaceEffect, 1),
        intent_id: record_id(RecordKind::Intent, 2),
        graph_id: record_id(RecordKind::Graph, 3),
        node_id: record_id(RecordKind::GraphNode, 4),
        attempt_id: record_id(RecordKind::Attempt, 5),
        familiar_snapshot_id: record_id(RecordKind::IdentitySnapshot, 6),
        project_id: "project:sha256:abc".to_owned(),
        action_class: "send_message".to_owned(),
        account_id: "account-1".to_owned(),
        locator: serde_json::json!({"chat_id": "chat-1"}),
        effect_digest: match digest(&effect) {
            Ok(value) => value,
            Err(error) => panic!("static surface effect is canonical: {error}"),
        },
        effect,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
    }
}

fn schema(value: &str) -> SchemaVersion {
    match SchemaVersion::parse(value) {
        Ok(value) => value,
        Err(error) => panic!("static schema is valid: {error}"),
    }
}

fn record_id(kind: RecordKind, value: u8) -> RecordId {
    let suffix = format!("01J000000000000000000000{value:02}");
    match RecordId::parse(kind, &format!("{}{suffix}", kind.prefix())) {
        Ok(value) => value,
        Err(error) => panic!("static record identity is valid: {error}"),
    }
}
