//! Typed surface acceptance and delivery operations.

use psyche_core::contracts::RecordKind;
use psyche_core::contracts::surface::{SurfaceEffect, SurfaceEvent};
use psyche_core::id::RecordId;

use crate::PortError;

/// Durable acceptance of one normalized surface event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceAcceptance {
    /// Exact accepted event identity.
    pub surface_event_id: RecordId,
    /// Whether the event entered the surface-neutral pipeline.
    pub accepted: bool,
}

impl SurfaceAcceptance {
    /// Validates the accepted event identity.
    pub fn validate(&self) -> Result<(), PortError> {
        if self.surface_event_id.kind() == RecordKind::SurfaceEvent {
            Ok(())
        } else {
            Err(PortError::InvalidResponse)
        }
    }
}

/// Durable delivery outcome for one surface effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryDisposition {
    /// The effect was durably applied.
    Applied {
        /// Stable external delivery identity.
        external_id: String,
    },
    /// The effect was authoritatively rejected.
    Rejected {
        /// Stable bounded rejection code.
        code: String,
    },
    /// Delivery may have occurred and cannot be retried as rejected.
    Unknown,
}

impl DeliveryDisposition {
    /// Validates bounded opaque response fields.
    pub fn validate(&self) -> Result<(), PortError> {
        let value = match self {
            Self::Applied { external_id } => Some(external_id.as_str()),
            Self::Rejected { code } => Some(code.as_str()),
            Self::Unknown => None,
        };
        if value.is_some_and(|value| value.is_empty() || value.len() > 255) {
            Err(PortError::InvalidResponse)
        } else {
            Ok(())
        }
    }
}

/// Behavior-level surface acceptance and delivery boundary.
#[async_trait::async_trait]
pub trait SurfacePort: Send + Sync {
    /// Accepts a validated surface event.
    async fn accept(&self, event: SurfaceEvent) -> Result<SurfaceAcceptance, PortError>;
    /// Applies a validated surface effect.
    async fn apply(&self, effect: SurfaceEffect) -> Result<DeliveryDisposition, PortError>;
}
