//! WebRTC FFI module
//!
//! ICE configuration with privacy controls (STUN, TURN),
//! SDP sanitization, and peer connection helpers.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_webrtc_core::ice::ice_config;
use soshal_webrtc_core::sdp::sanitize_sdp;

/// ICE server configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceConfig {
    pub ice_servers: Vec<IceServer>,
    pub ice_transport_policy: String, // "all" or "relay"
}

/// Individual ICE server (STUN or TURN)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
    pub credential_type: Option<String>,
}

/// Get ICE configuration based on privacy settings.
/// `privacy_level`: "public" (all), "friends" (relay only).
#[frb(sync, serialize)]
pub fn webrtc_get_ice_config(privacy_level: String) -> Result<String, String> {
    let value = ice_config(&privacy_level, "stun:stun.l.google.com:19302");
    Ok(value.to_string()).into()
}

/// Get STUN servers
#[frb(sync, serialize)]
pub fn webrtc_get_stun_servers() -> Result<Vec<String>, String> {
    Ok(vec![
        "stun:stun.l.google.com:19302".to_string(),
        "stun:stun1.l.google.com:19302".to_string(),
    ])
    .into()
}

/// Get TURN servers (if enabled)
#[frb(sync, serialize)]
pub fn webrtc_get_turn_servers(auth_token: Option<String>) -> Result<String, String> {
    if auth_token.is_some() {
        return Err("TURN provisioning requires a server endpoint".to_string()).into();
    }
    Ok("[]".to_string()).into()
}

/// Sanitize SDP to remove private IPs. `force_relay` true drops non-relay
/// candidates (matches `friends`/`private` privacy levels).
#[frb(sync, serialize)]
pub fn webrtc_sanitize_sdp(sdp: String, force_relay: bool) -> Result<String, String> {
    Ok(sanitize_sdp(&sdp, force_relay)).into()
}

/// Create peer connection config
#[frb(sync, serialize)]
pub fn webrtc_create_peer_config(privacy_level: String) -> Result<String, String> {
    let ice_value = ice_config(&privacy_level, "stun:stun.l.google.com:19302");
    let config_json = match ice_value.as_object() {
        Some(obj) => {
            let servers = obj
                .get("iceServers")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let policy = obj
                .get("iceTransportPolicy")
                .and_then(|v| v.as_str())
                .unwrap_or("all");
            serde_json::json!({
                "iceServers": servers,
                "iceTransportPolicy": policy,
                "bundlePolicy": "max-bundle",
                "rtcpMuxPolicy": "require",
            })
        }
        None => serde_json::json!({
            "iceServers": [],
            "iceTransportPolicy": "all",
            "bundlePolicy": "max-bundle",
            "rtcpMuxPolicy": "require",
        }),
    };
    Ok(config_json.to_string()).into()
}

/// Extract candidates from SDP
#[frb(sync, serialize)]
pub fn webrtc_extract_candidates(sdp: String) -> Result<Vec<String>, String> {
    let candidates: Vec<String> = sdp
        .lines()
        .filter(|line| line.starts_with("a=candidate:"))
        .map(|line| line.to_string())
        .collect();
    Ok(candidates).into()
}

/// Add candidate to SDP
#[frb(sync, serialize)]
pub fn webrtc_add_candidate_to_sdp(sdp: String, candidate: String) -> Result<String, String> {
    let mut result = sdp;
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&candidate);
    Ok(result).into()
}

/// Validate SDP
#[frb(sync, serialize)]
pub fn webrtc_validate_sdp(sdp: String) -> Result<bool, String> {
    // Basic SDP format validation
    Ok(sdp.contains("v=0") && sdp.contains("o=")).into()
}
