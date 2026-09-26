//! Spatial core: geohash encode/decode, distance, and the WGPU mesh-layout
//! engine.

// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod distance;
pub mod geohash;
pub mod index;
pub mod polygon;
pub mod wgpu_engine;

pub use index::{SpatialElement, SpatialIndex};
pub use polygon::GeofencePolygon;
