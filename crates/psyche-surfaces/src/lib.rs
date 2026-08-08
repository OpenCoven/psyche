//! Behavior-level surface acceptance and delivery boundary.

pub mod error;
pub mod port;

pub use error::PortError;
pub use port::{DeliveryDisposition, SurfaceAcceptance, SurfacePort};
