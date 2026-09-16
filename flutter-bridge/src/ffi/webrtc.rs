//! WebRTC FFI module
//!
//! ICE configuration with privacy controls (STUN, TURN),
//! SDP sanitization, and peer connection helpers.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_webrtc_core::ice::ice_config;
use soshal_webrtc_core::sdp::{extract_candidates, sanitize_sdp, validate_sdp};

/// Extract the host from a `turn:`/`stuns:`/`turns:`/`stun:` endpoint and
/// refuse loopback/private literals (the TURN endpoint is settings-driven,
/// so a compromised DB or typo'd config must not exfiltrate STUN/TURN
/// credentials to an internal address).
fn validate_ice_endpoint_host(endpoint: &str) -> Result<String, String> {
    let lower = endpoint.trim().to_lowercase();
    let mut rest: &str = endpoint.trim();
    for scheme in [
        "turn://", "turns://", "stun://", "stuns://", "turn:", "turns:", "stun:", "stuns:",
    ] {
        if let Some(stripped) = lower.strip_prefix(scheme) {
            rest = &endpoint.trim()[endpoint.trim().len() - stripped.len()..];
            break;
        }
    }
    let rest = rest.split('@').next_back().unwrap_or(rest);
    let host = if rest.starts_with('[') {
        rest.split(']')
            .nth(1)
            .map(|_| rest[1..rest.find(']').unwrap_or(1)].to_string())
            .unwrap_or_else(|| rest.to_string())
    } else {
        rest.split(':').next().unwrap_or(rest).to_string()
    };
    let host = host.trim().trim_matches('.').to_string();
    if host.is_empty() {
        return Err(format!("ice endpoint {endpoint:?} has no host"));
    }
    if host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host
            .parse::<std::net::IpAddr>()
            .map(soshal_network_core::lan::is_private_ip)
            .unwrap_or(false)
    {
        return Err(format!(
            "ice endpoint targets a loopback or private host: {host}"
        ));
    }
    Ok(host)
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

/// Get TURN servers (if enabled). Reads the configured TURN endpoint from
/// the `turn_endpoint` setting (`turn:host:port`, plus optional
/// `turn_username`/`turn_credential`); errors when unconfigured —
/// server-side TURN provisioning is backend-gated (roadmap).
#[frb(sync, serialize)]
pub fn webrtc_get_turn_servers(_auth_token: Option<String>) -> Result<String, String> {
    let settings = super::db::with_db_result(|db| {
        SettingsRepo::new(db).get_many(&["turn_endpoint", "turn_username", "turn_credential"])
    })?;
    let endpoint = match settings.get("turn_endpoint").filter(|s| !s.trim().is_empty()) {
        Some(e) => e,
        None => {
            return Err(
                "turn provisioning unavailable: no turn_endpoint configured (server endpoint on roadmap)"
                    .to_string(),
            )
            .into()
        }
    };
    let username = settings.get("turn_username").filter(|s| !s.is_empty());
    let credential = settings.get("turn_credential").filter(|s| !s.is_empty());
    validate_ice_endpoint_host(endpoint)?;
    let mut server = serde_json::json!({
        "urls": [endpoint],
    });
    if let Some(user) = username {
        server["username"] = serde_json::json!(user);
    }
    if let Some(cred) = credential {
        server["credential"] = serde_json::json!(cred);
        server["credentialType"] = serde_json::json!("password");
    }
    Ok(super::util::json_ok_or_empty(&vec![server])).into()
}

/// Sanitize SDP to remove private IPs. `force_relay` true drops non-relay
/// candidates (matches `friends`/`private` privacy levels).
#[frb(sync, serialize)]
pub fn webrtc_sanitize_sdp(sdp: String, force_relay: bool) -> Result<String, String> {
    Ok(sanitize_sdp(&sdp, force_relay)).into()
}

#[derive(serde::Serialize)]
struct PeerConfigDto<'a> {
    #[serde(rename = "iceServers")]
    ice_servers: &'a serde_json::Value,
    #[serde(rename = "iceTransportPolicy")]
    ice_transport_policy: &'a str,
    #[serde(rename = "bundlePolicy")]
    bundle_policy: &'static str,
    #[serde(rename = "rtcpMuxPolicy")]
    rtcp_mux_policy: &'static str,
}

static EMPTY_SERVERS: std::sync::LazyLock<serde_json::Value> =
    std::sync::LazyLock::new(|| serde_json::json!([]));

/// Create peer connection config
#[frb(sync, serialize)]
pub fn webrtc_create_peer_config(privacy_level: String) -> Result<String, String> {
    let ice_value = ice_config(&privacy_level, "stun:stun.l.google.com:19302");
    let (servers, policy) = match ice_value.as_object() {
        Some(obj) => {
            let s = obj.get("iceServers").unwrap_or(&EMPTY_SERVERS);
            let p = obj
                .get("iceTransportPolicy")
                .and_then(|v| v.as_str())
                .unwrap_or("all");
            (s, p)
        }
        None => (&*EMPTY_SERVERS, "all"),
    };
    let dto = PeerConfigDto {
        ice_servers: servers,
        ice_transport_policy: policy,
        bundle_policy: "max-bundle",
        rtcp_mux_policy: "require",
    };
    serde_json::to_string(&dto)
        .map_err(|e| format!("serialize peer config: {e}"))
        .into()
}

/// Extract candidates from SDP
#[frb(sync, serialize)]
pub fn webrtc_extract_candidates(sdp: String) -> Result<Vec<String>, String> {
    Ok(extract_candidates(&sdp)).into()
}

/// Add a single ICE candidate attribute line to an SDP blob.
///
/// # Security (H3 fix)
/// `candidate` is validated before concatenation to prevent SDP injection:
/// - Must start with `a=` (only SDP attribute lines carry ICE candidates).
/// - Must not contain bare CR (`\r`) or LF (`\n`) inside the line body;
///   an attacker-supplied multi-line string could inject arbitrary SDP
///   sections that bypass `webrtc_sanitize_sdp`.
#[frb(sync, serialize)]
pub fn webrtc_add_candidate_to_sdp(sdp: String, candidate: String) -> Result<String, String> {
    // Enforce SDP attribute line prefix.
    if !candidate.starts_with("a=") {
        return Err("candidate must be an SDP attribute line starting with 'a='".to_string())
            .into();
    }
    // Strip the expected trailing CRLF/LF terminator (if any) and check the
    // remainder for embedded newlines (injection attempt).
    let body = candidate.trim_end_matches("\r\n").trim_end_matches('\n');
    if body.contains('\r') || body.contains('\n') {
        return Err(
            "candidate contains an embedded line break (SDP injection rejected)".to_string(),
        )
        .into();
    }
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
    Ok(validate_sdp(&sdp)).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    #[test]
    fn test_ice_config_public() {
        let json = webrtc_get_ice_config("public".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["forceRelay"], false);
        assert_eq!(v["iceServers"][0]["urls"], "stun:stun.l.google.com:19302");
    }

    #[test]
    fn test_ice_config_force_relay_levels() {
        for level in ["friends", "private"] {
            let json = webrtc_get_ice_config(level.to_string()).unwrap();
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(v["iceTransportPolicy"], "relay");
            assert_eq!(v["forceRelay"], true);
        }
    }

    #[test]
    fn test_ice_config_empty_defaults_all() {
        let json = webrtc_get_ice_config(String::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["forceRelay"], false);
    }

    #[test]
    fn test_ice_config_unknown_level_forces_relay() {
        let json = webrtc_get_ice_config("garbage".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "relay");
        assert_eq!(v["forceRelay"], true);
    }

    #[test]
    fn test_stun_servers() {
        let servers = webrtc_get_stun_servers().unwrap();
        assert_eq!(servers.len(), 2);
        assert!(servers.iter().all(|s| s.starts_with("stun:")));
    }

    #[test]
    fn test_turn_servers_err_when_unconfigured() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("turn_empty", "webrtc");
        let expected = "turn provisioning unavailable: no turn_endpoint configured (server endpoint on roadmap)".to_string();
        assert_eq!(webrtc_get_turn_servers(None).unwrap_err(), expected);
        assert_eq!(
            webrtc_get_turn_servers(Some("token".to_string())).unwrap_err(),
            expected
        );
    }

    #[test]
    fn test_turn_servers_err_when_endpoint_empty() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("turn_empty_str", "webrtc");
        db::db_set_setting("turn_endpoint".to_string(), "  ".to_string()).unwrap();
        assert_eq!(
            webrtc_get_turn_servers(None).unwrap_err(),
            "turn provisioning unavailable: no turn_endpoint configured (server endpoint on roadmap)"
        );
    }

    #[test]
    fn test_turn_servers_from_settings() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = db::tmp_db("turn", "webrtc");
        db::db_set_setting(
            "turn_endpoint".to_string(),
            "turn:turn.example.com:3478".to_string(),
        )
        .unwrap();
        let json = webrtc_get_turn_servers(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["urls"][0], "turn:turn.example.com:3478");
        assert!(v[0].get("username").is_none());
        assert!(v[0].get("credential").is_none());
        db::db_set_setting("turn_username".to_string(), "u1".to_string()).unwrap();
        db::db_set_setting("turn_credential".to_string(), "s3cret".to_string()).unwrap();
        let json = webrtc_get_turn_servers(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["username"], "u1");
        assert_eq!(v[0]["credential"], "s3cret");
        assert_eq!(v[0]["credentialType"], "password");
    }

    #[test]
    fn test_sanitize_sdp_drops_host_candidates() {
        let sdp = "a=candidate:1 1 UDP 2130706431 192.168.1.5 54321 typ host";
        assert_eq!(webrtc_sanitize_sdp(sdp.to_string(), false).unwrap(), "");
        assert_eq!(webrtc_sanitize_sdp(sdp.to_string(), true).unwrap(), "");
    }

    #[test]
    fn test_sanitize_sdp_gates_srflx_by_force_relay() {
        let srflx = "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx";
        assert_eq!(
            webrtc_sanitize_sdp(srflx.to_string(), false).unwrap(),
            srflx
        );
        assert_eq!(webrtc_sanitize_sdp(srflx.to_string(), true).unwrap(), "");
    }

    #[test]
    fn test_sanitize_sdp_keeps_relay_drops_private_srflx() {
        let relay = "a=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay";
        assert_eq!(webrtc_sanitize_sdp(relay.to_string(), true).unwrap(), relay);
        let private_srflx = "a=candidate:3 1 UDP 2130706431 10.0.0.5 54323 typ srflx";
        assert_eq!(
            webrtc_sanitize_sdp(private_srflx.to_string(), false).unwrap(),
            ""
        );
    }

    #[test]
    fn test_sanitize_sdp_rewrites_private_c_lines() {
        assert_eq!(
            webrtc_sanitize_sdp("c=IN IP4 192.168.1.5".to_string(), false).unwrap(),
            "c=IN IP4 127.0.0.1"
        );
        assert_eq!(
            webrtc_sanitize_sdp("c=IN IP6 fd00::1".to_string(), false).unwrap(),
            "c=IN IP6 ::1"
        );
        assert_eq!(
            webrtc_sanitize_sdp("c=IN IP4 8.8.8.8".to_string(), false).unwrap(),
            "c=IN IP4 8.8.8.8"
        );
    }

    #[test]
    fn test_sanitize_sdp_rewrites_origin_address() {
        assert_eq!(
            webrtc_sanitize_sdp("o=- 0 0 IN IP4 192.168.1.5".to_string(), false).unwrap(),
            "o=- 0 0 IN IP4 0.0.0.0"
        );
        assert_eq!(
            webrtc_sanitize_sdp("o=- 0 0 IN IP4 8.8.8.8".to_string(), false).unwrap(),
            "o=- 0 0 IN IP4 0.0.0.0"
        );
    }

    #[test]
    fn test_sanitize_sdp_joins_lines_crlf() {
        assert_eq!(
            webrtc_sanitize_sdp(
                "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\nc=IN IP4 192.168.1.5"
                    .to_string(),
                false
            )
            .unwrap(),
            "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\r\nc=IN IP4 127.0.0.1"
        );
    }

    #[test]
    fn test_sanitize_sdp_empty() {
        assert_eq!(webrtc_sanitize_sdp(String::new(), false).unwrap(), "");
    }

    #[test]
    fn test_create_peer_config() {
        let json = webrtc_create_peer_config("public".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["bundlePolicy"], "max-bundle");
        assert_eq!(v["rtcpMuxPolicy"], "require");
        assert!(!v["iceServers"].as_array().unwrap().is_empty());
        let json = webrtc_create_peer_config("friends".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "relay");
    }

    #[test]
    fn test_extract_candidates() {
        let sdp = "v=0\r\na=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\r\nm=audio 0 RTP/AVP 0\r\na=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay";
        let candidates = webrtc_extract_candidates(sdp.to_string()).unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].starts_with("a=candidate:1"));
        assert!(candidates[1].starts_with("a=candidate:2"));
        assert_eq!(
            webrtc_extract_candidates("v=0\nm=audio 0 RTP/AVP 0".to_string()).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_add_candidate_to_sdp() {
        // Valid candidates (a= prefix, no embedded newlines)
        assert_eq!(
            webrtc_add_candidate_to_sdp("v=0".to_string(), "a=candidate:1".to_string()).unwrap(),
            "v=0\na=candidate:1"
        );
        assert_eq!(
            webrtc_add_candidate_to_sdp("v=0\n".to_string(), "a=candidate:2".to_string()).unwrap(),
            "v=0\na=candidate:2"
        );
        // H3 fix: must start with a=
        assert!(
            webrtc_add_candidate_to_sdp("v=0".to_string(), "candidate:1 …".to_string()).is_err(),
            "non a= prefix must be rejected"
        );
        assert!(
            webrtc_add_candidate_to_sdp("v=0".to_string(), "m=audio 0 RTP/AVP 0".to_string())
                .is_err(),
            "non a= prefix must be rejected"
        );
        // H3 fix: embedded CR or LF inside line body must be rejected
        assert!(
            webrtc_add_candidate_to_sdp(
                "v=0".to_string(),
                "a=candidate:1\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111".to_string()
            )
            .is_err(),
            "embedded CRLF must be rejected"
        );
        assert!(
            webrtc_add_candidate_to_sdp(
                "v=0".to_string(),
                "a=candidate:1\na=candidate:2".to_string()
            )
            .is_err(),
            "embedded LF must be rejected"
        );
    }

    #[test]
    fn test_validate_sdp() {
        assert!(webrtc_validate_sdp("v=0\r\no=- 0 0 IN IP4 127.0.0.1".to_string()).unwrap());
        assert!(!webrtc_validate_sdp("v=0 only".to_string()).unwrap());
        assert!(!webrtc_validate_sdp("o=- 0 0 IN IP4 127.0.0.1".to_string()).unwrap());
        assert!(!webrtc_validate_sdp(String::new()).unwrap());
    }
}
