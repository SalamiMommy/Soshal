//! Chat Random FFI module
//!
//! Random-interest pairing: kind-20030 availability announcements, kind-20031
//! requests, kind-20032 accepts — signed in-process, relay-published, and
//! signature-verified on fetch.

use flutter_rust_bridge::frb;
use soshal_streaming_core::events as streaming_events;

/// Build the availability content JSON for an announcement.
#[frb(sync, serialize)]
pub fn chatrandom_available_content(
    interests: Vec<String>,
    media_type: String,
    mode: String,
) -> Result<String, String> {
    Ok(streaming_events::chatrandom_available_content(
        &interests,
        &media_type,
        &mode,
    ))
    .into()
}

/// Sign + publish a chatrandom event for `request_type` (`request`/`accept`)
/// addressed to `peers` (p-tags). Returns the event id.
#[frb(serialize)]
pub async fn chatrandom_send(
    request_type: String,
    peers: Vec<String>,
    content_json: String,
) -> Result<String, String> {
    let (kind, mut content) = streaming_events::chatrandom_request_parts(&request_type)?;
    if peers.is_empty() {
        return Err("Recipient peer list cannot be empty for chatrandom handshake".to_string());
    }
    if peers.len() > 50 {
        return Err("Recipient peer list exceeds maximum of 50 peers".to_string());
    }
    if content_json.len() > 65536 {
        return Err("Content exceeds maximum length of 64KB".to_string());
    }
    let my_pk = super::signer::signer_pubkey()?;
    for p in &peers {
        super::session::validate_pubkey_hex(p)?;
        if p.eq_ignore_ascii_case(&my_pk) {
            return Err("Cannot pair with yourself".to_string());
        }
    }
    if !content_json.trim().is_empty() {
        content = content_json;
    }
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(kind), content);
    for p in peers {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), p]) {
            builder = builder.tag(tag);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch chatrandom events (kinds 20030/20031/20032) addressed to me or by
/// `author`. Returns JSON array of peer entries.
#[frb(serialize)]
pub async fn chatrandom_fetch(
    my_pubkey: String,
    author: Option<String>,
    limit: u64,
) -> Result<String, String> {
    super::session::validate_pubkey_hex(&my_pubkey)?;
    if let Some(ref auth) = author {
        super::session::validate_pubkey_hex(auth)?;
    }
    super::signer::require_identity(&my_pubkey)?;
    let my_pubkey_lower = my_pubkey.trim().to_ascii_lowercase();
    let lim = limit.clamp(1, 100);
    let events: Vec<nostr::event::Event> = if let Some(p) = author {
        let p_clean = p.trim().to_ascii_lowercase();
        let filter = serde_json::json!({
            "kinds": [20030, 20031, 20032],
            "authors": [p_clean],
            "limit": lim,
        });
        let raw = super::network::network_query_events(filter.to_string()).await?;
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?
    } else {
        let avail_filter = serde_json::json!({
            "kinds": [20030],
            "limit": lim,
        });
        let direct_filter = serde_json::json!({
            "kinds": [20031, 20032],
            "#p": [&my_pubkey_lower],
            "limit": lim,
        });
        let (raw_avail, raw_direct) = tokio::join!(
            super::network::network_query_events(avail_filter.to_string()),
            super::network::network_query_events(direct_filter.to_string())
        );
        let mut ev_list = Vec::new();
        if let Ok(raw) = raw_avail {
            if let Ok(parsed) = serde_json::from_str::<Vec<nostr::event::Event>>(&raw) {
                ev_list.extend(parsed);
            }
        }
        if let Ok(raw) = raw_direct {
            if let Ok(parsed) = serde_json::from_str::<Vec<nostr::event::Event>>(&raw) {
                ev_list.extend(parsed);
            }
        }
        ev_list
    };

    #[derive(serde::Serialize)]
    struct ChatRandomItem<'a> {
        id: String,
        pubkey: String,
        content: &'a str,
        created_at: u64,
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(events.len().min(100));
    for e in &events {
        let id_hex = e.id.to_hex();
        if !seen_ids.insert(id_hex.clone()) {
            continue;
        }
        if !soshal_nostr_core::models::verify_event(e) {
            continue;
        }
        let k = e.kind.as_u16();
        if k == 20031 || k == 20032 {
            let addresses_me = e.tags.iter().any(|t| {
                let s = t.as_slice();
                s.first().map(|v| v == "p").unwrap_or(false)
                    && s.get(1)
                        .map(|v| v.eq_ignore_ascii_case(&my_pubkey_lower))
                        .unwrap_or(false)
            });
            let authored_by_me = e.pubkey.to_hex().eq_ignore_ascii_case(&my_pubkey_lower);
            if !addresses_me && !authored_by_me {
                continue;
            }
        }
        out.push(ChatRandomItem {
            id: id_hex,
            pubkey: e.pubkey.to_hex(),
            content: &e.content,
            created_at: e.created_at.as_secs(),
        });
    }
    out.sort_unstable_by_key(|item| std::cmp::Reverse(item.created_at));
    if out.len() > lim as usize {
        out.truncate(lim as usize);
    }
    super::util::json_ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CHATRANDOM_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_available_content_roundtrip() {
        let content = chatrandom_available_content(
            vec!["nostr".to_string(), "music".to_string()],
            "video".to_string(),
            "random".to_string(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["interests"][0], "nostr");
        assert_eq!(v["interests"][1], "music");
        assert_eq!(v["media_type"], "video");
        assert_eq!(v["mode"], "random");
    }

    #[test]
    fn test_available_content_empty_inputs() {
        let content =
            chatrandom_available_content(Vec::new(), String::new(), String::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v["interests"].as_array().unwrap().is_empty());
        assert_eq!(v["media_type"], "");
        assert_eq!(v["mode"], "");
    }

    #[tokio::test]
    async fn test_send_invalid_request_type_errors() {
        let err = chatrandom_send(
            "bogus".to_string(),
            vec!["peers".to_string()],
            "{}".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("invalid request type"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_send_publishes_signed_event_via_network() {
        let _g = crate::ffi::util::lock(&CHATRANDOM_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let peer_keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = chatrandom_send(
            "request".to_string(),
            vec![peer_keys.public_key().to_hex()],
            "{}".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("relay client not initialized"));
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_send_validates_peers_and_self_pairing() {
        let _g = crate::ffi::util::lock(&CHATRANDOM_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Empty peers
        let err = chatrandom_send("request".to_string(), Vec::new(), "{}".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("cannot be empty"), "{err}");

        // Self-pairing
        let err = chatrandom_send(
            "request".to_string(),
            vec![keys.public_key().to_hex()],
            "{}".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("Cannot pair with yourself"), "{err}");

        // Invalid hex
        let err = chatrandom_send(
            "request".to_string(),
            vec!["not-a-valid-hex-pubkey".to_string()],
            "{}".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("invalid pubkey"), "{err}");

        // Oversized content
        let peer = soshal_nostr_core::keys::generate_keys()
            .public_key()
            .to_hex();
        let big = "x".repeat(70000);
        let err = chatrandom_send("request".to_string(), vec![peer], big)
            .await
            .unwrap_err();
        assert!(err.contains("exceeds maximum length"), "{err}");

        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_requires_initialized_relay_client() {
        let _g = crate::ffi::util::lock(&CHATRANDOM_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = chatrandom_fetch(pk.clone(), Some(pk), 50)
            .await
            .unwrap_err();
        assert!(err.contains("relay client not initialized"), "got {err}");
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_fetch_unauthorized_rejected() {
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _ = super::super::signer::signer_lock();
        let pk = "deadbeef".repeat(8);
        let err = chatrandom_fetch(pk.clone(), None, 50).await.unwrap_err();
        assert!(err.contains("signer locked") || err.contains("signer key mismatch"));
    }

    #[tokio::test]
    async fn test_fetch_rejects_invalid_hex() {
        let err = chatrandom_fetch("invalid-hex".to_string(), None, 50)
            .await
            .unwrap_err();
        assert!(err.contains("invalid pubkey"), "{err}");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_send_validates_peer_limit() {
        let _g = crate::ffi::util::lock(&CHATRANDOM_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Too many peers (> 50)
        let peer_pk = soshal_nostr_core::keys::generate_keys()
            .public_key()
            .to_hex();
        let many_peers = vec![peer_pk; 51];
        let err = chatrandom_send("request".to_string(), many_peers, "{}".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("exceeds maximum of 50 peers"), "{err}");

        super::super::signer::signer_lock().unwrap();
    }
}
