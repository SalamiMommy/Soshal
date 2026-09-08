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
    let mut filter = serde_json::json!({
        "kinds": [20030, 20031, 20032],
        "limit": limit.min(100),
    });
    if let Some(p) = author {
        filter["authors"] = serde_json::json!([p]);
    }
    let raw = super::network::network_query_events(filter.to_string()).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    #[derive(serde::Serialize)]
    struct ChatRandomItem<'a> {
        id: String,
        pubkey: String,
        content: &'a str,
        created_at: u64,
    }

    let mut out = Vec::with_capacity(events.len().min(100));
    for e in &events {
        if !soshal_nostr_core::models::verify_event(e) {
            continue;
        }
        let k = e.kind.as_u16();
        if k == 20031 || k == 20032 {
            let addresses_me = e.tags.iter().any(|t| {
                let s = t.as_slice();
                s.first().map(|v| v == "p").unwrap_or(false)
                    && s.get(1).map(|v| v == &my_pubkey).unwrap_or(false)
            });
            let authored_by_me = e.pubkey.to_hex() == my_pubkey;
            if !addresses_me && !authored_by_me {
                continue;
            }
        }
        out.push(ChatRandomItem {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            content: &e.content,
            created_at: e.created_at.as_secs(),
        });
    }
    out.sort_unstable_by_key(|item| std::cmp::Reverse(item.created_at));
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
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = chatrandom_send(
            "request".to_string(),
            vec![keys.public_key().to_hex()],
            "{}".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("relay client not initialized"));
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    async fn test_fetch_requires_initialized_relay_client() {
        let pk = "deadbeef".repeat(8);
        let err = chatrandom_fetch(pk.clone(), Some(pk), 50)
            .await
            .unwrap_err();
        assert!(err.contains("relay client not initialized"), "got {err}");
    }
}
