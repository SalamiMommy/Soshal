//! Call signaling FFI module (WebRTC voice/video)
//!
//! NIP-style signaling over relays: kind 20001 (offer), 20002 (answer),
//! 20003 (ICE), 20004 (end). SDP/candidates are redacted of private IPs
//! before publishing; fetched signals are p-tag filtered and
//! signature-verified. Media itself runs app-side (Flutter WebRTC).

use flutter_rust_bridge::frb;

fn signal_kind(signal_type: &str) -> Result<u16, String> {
    match signal_type {
        "offer" => Ok(20001),
        "answer" => Ok(20002),
        "ice" => Ok(20003),
        "end" => Ok(20004),
        _ => Err("invalid signal type, use offer/answer/ice/end".into()),
    }
}

/// Send a call signal to `target_pubkey`. SDP and candidates are redacted of
/// private IP literals before relay publish. Returns the event id.
#[frb(serialize)]
pub async fn calls_send_signal(
    signal_type: String,
    target_pubkey: String,
    call_id: String,
    sdp: Option<String>,
    candidate: Option<String>,
    media_type: Option<String>,
) -> Result<String, String> {
    let kind = signal_kind(&signal_type)?;
    let mut content = serde_json::json!({
        "call_id": call_id,
        "type": signal_type,
    });
    if let Some(s) = sdp {
        content["sdp"] = serde_json::json!(soshal_webrtc_core::ice::redact_private_ips(&s));
    }
    if let Some(c) = candidate {
        content["candidate"] = serde_json::json!(soshal_webrtc_core::ice::redact_private_ips(&c));
    }
    if let Some(m) = media_type {
        content["media_type"] = serde_json::json!(m);
    }
    let content_str = serde_json::to_string(&content).map_err(|e| format!("serialize: {e}"))?;
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(kind), content_str);
    for tag in [
        vec!["p".to_string(), target_pubkey],
        vec!["call".to_string(), call_id],
    ] {
        if let Ok(t) = nostr::event::Tag::parse(tag) {
            builder = builder.tag(t);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch call signals addressed to me (kinds 20001-20004, `#p` tag filter).
/// Returns JSON array of `{id, pubkey, content, created_at, kind, p_tags,
/// call_id}` — only verified events, p-tag verified to `my_pubkey`.
#[frb(serialize)]
pub async fn calls_fetch_signals(my_pubkey: String) -> Result<String, String> {
    let filter = serde_json::json!({
        "kinds": [20001, 20002, 20003, 20004],
        "#p": [my_pubkey],
        "limit": 100,
    })
    .to_string();
    let raw = super::network::network_query_events(filter).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    let now = soshal_common_core::format::now_secs() as u64;
    for e in events {
        let p_tags: Vec<String> = e
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(|k| k == "p").unwrap_or(false))
            .filter_map(|t| t.as_slice().get(1).cloned())
            .collect();
        if !p_tags.iter().any(|p| p == &my_pubkey) {
            continue;
        }
        if e.verify().is_err() {
            continue;
        }
        let call_id = e
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|k| k == "call").unwrap_or(false))
            .and_then(|t| t.as_slice().get(1).cloned())
            .unwrap_or_default();
        let created = e.created_at.as_secs();
        if now.saturating_sub(created) > 300 {
            continue; // ignore stale signals (>5 min)
        }
        out.push(serde_json::json!({
            "id": e.id.to_hex(),
            "pubkey": e.pubkey.to_string(),
            "content": e.content,
            "created_at": created,
            "kind": e.kind.as_u16(),
            "p_tags": p_tags,
            "call_id": call_id,
        }));
    }
    out.sort_by(|a, b| {
        b["created_at"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["created_at"].as_u64().unwrap_or(0))
    });
    super::util::json_ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CALLS_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn signal_kind_maps_valid_types() {
        assert_eq!(signal_kind("offer"), Ok(20001));
        assert_eq!(signal_kind("answer"), Ok(20002));
        assert_eq!(signal_kind("ice"), Ok(20003));
        assert_eq!(signal_kind("end"), Ok(20004));
    }

    #[test]
    fn signal_kind_rejects_invalid_type() {
        assert!(signal_kind("hangup").is_err());
        assert!(signal_kind("").is_err());
    }

    #[test]
    fn sanitize_sdp_redacts_private_conn_line() {
        let sdp = "v=0\r\nc=IN IP4 192.168.1.50\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n";
        let out = super::super::webrtc::webrtc_sanitize_sdp(sdp.to_string(), false).unwrap();
        assert!(out.contains("c=IN IP4 127.0.0.1"));
        assert!(!out.contains("192.168.1.50"));
    }

    #[test]
    fn sanitize_sdp_force_relay_drops_srflx_keeps_relay() {
        let sdp = "v=0\r\na=candidate:2 1 UDP 1686052607 203.0.113.9 5000 typ srflx\r\n\
a=candidate:3 1 UDP 1694498815 8.8.8.8 5000 typ relay\r\n";
        let out = super::super::webrtc::webrtc_sanitize_sdp(sdp.to_string(), true).unwrap();
        assert!(!out.contains("srflx"));
        assert!(out.contains("typ relay"));
    }

    #[test]
    fn sanitize_sdp_keeps_public_conn_line() {
        let sdp = "v=0\r\nc=IN IP4 8.8.8.8\r\n";
        let out = super::super::webrtc::webrtc_sanitize_sdp(sdp.to_string(), false).unwrap();
        assert!(out.contains("8.8.8.8"));
    }

    #[test]
    fn ice_config_public_level_all_candidates() {
        let cfg: serde_json::Value = serde_json::from_str(
            &super::super::webrtc::webrtc_get_ice_config("public".into()).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg["iceTransportPolicy"], "all");
        assert_eq!(cfg["forceRelay"], false);
        assert_eq!(cfg["iceServers"][0]["urls"], "stun:stun.l.google.com:19302");
    }

    #[tokio::test]
    async fn send_signal_requires_unlocked_signer() {
        let _g = CALLS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        super::super::signer::signer_lock().unwrap();
        let err = calls_send_signal(
            "offer".into(),
            "deadbeef".into(),
            "call-1".into(),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(err.contains("signer locked"));
    }

    #[tokio::test]
    async fn send_signal_publishes_signed_event_via_network() {
        let _g = CALLS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = calls_send_signal(
            "offer".into(),
            keys.public_key().to_hex(),
            "call-1".into(),
            Some("v=0\r\nc=IN IP4 192.168.1.50\r\n".into()),
            None,
            Some("audio".into()),
        )
        .await
        .unwrap_err();
        assert!(err.contains("relay client not initialized"));
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    async fn fetch_signals_requires_initialized_relay_client() {
        let err = calls_fetch_signals("deadbeef".into()).await.unwrap_err();
        assert!(err.contains("relay client not initialized"));
    }
}
