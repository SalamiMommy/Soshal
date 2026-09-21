//! WebRTC core: ICE candidate handling and SDP sanitization/validation.

// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod ice;
pub mod sdp;
