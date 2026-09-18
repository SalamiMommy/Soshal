//! Vouch FFI module (web-of-trust)
//!
//! Kind-31989 vouches signed in-process and published through the shared
//! relay client (`network` module). Fetched vouches are signature-verified.

use flutter_rust_bridge::frb;

/// Publish a vouch (kind 31989) for `target_pubkey`. Returns the event id.
#[frb(serialize)]
pub async fn vouch_publish(target_pubkey: String, content: String) -> Result<String, String> {
    let target_pubkey = target_pubkey.trim().to_ascii_lowercase();
    if target_pubkey.len() != 64 || !target_pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid target_pubkey: must be 64-character hex string".to_string());
    }
    let my_pk = super::signer::signer_pubkey()?;
    if target_pubkey.eq_ignore_ascii_case(&my_pk) {
        return Err("cannot vouch for yourself".to_string());
    }
    if content.len() > 1000 {
        return Err("content too long: max 1000 characters".to_string());
    }
    let tag = nostr::event::Tag::parse(vec!["p".to_string(), target_pubkey])
        .map_err(|e| format!("invalid p-tag: {e}"))?;
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31989), content).tag(tag);
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch vouches (kind 31989) addressed to `target_pubkey`. Only verified
/// signatures are returned. Returns JSON array of relation entries.
#[frb(serialize)]
pub async fn vouch_fetch(target_pubkey: String) -> Result<String, String> {
    let target_pubkey = target_pubkey.trim().to_ascii_lowercase();
    if target_pubkey.len() != 64 || !target_pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid target_pubkey: must be 64-character hex string".to_string());
    }
    let filter = serde_json::json!({
        "kinds": [31989],
        "#p": [target_pubkey],
        "limit": 30,
    })
    .to_string();
    let raw = super::network::network_query_events(filter).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    for e in events {
        if e.kind.as_u16() != 31989 {
            continue;
        }
        if e.pubkey.to_hex().eq_ignore_ascii_case(&target_pubkey) {
            // Drop invalid self-attestations
            continue;
        }
        if !soshal_nostr_core::models::verify_event(&e) {
            continue;
        }
        out.push(serde_json::json!({
            "id": e.id.to_hex(),
            "pubkey": e.pubkey.to_hex(),
            "content": e.content,
            "created_at": e.created_at.as_secs(),
        }));
    }
    super::util::json_ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_publish_locked_signer_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        super::super::signer::signer_lock().unwrap();
        let result = vouch_publish("a".repeat(64), "trusted".to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("locked"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_publish_signs_then_missing_relay() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let target_pk = "b".repeat(64);
        let result = vouch_publish(target_pk, "trusted".to_string()).await;
        let err = result.unwrap_err();
        assert!(err.contains("relay client not initialized"), "{err}");
        assert!(!err.contains("signer locked"));
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_publish_self_vouch_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let result = vouch_publish(keys.public_key().to_hex(), "trusted".to_string()).await;
        let err = result.unwrap_err();
        assert!(err.contains("cannot vouch for yourself"), "{err}");
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_fetch_requires_relay_client() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        super::super::signer::signer_lock().unwrap();
        let result = vouch_fetch("a".repeat(64)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("relay client not initialized"));
    }

    #[test]
    fn test_vouch_fixture_verify_and_mapping() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let target = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let mut builder = nostr::event::EventBuilder::new(
            nostr::event::Kind::from_u16(31989),
            "trusted".to_string(),
        );
        builder = builder.tag(
            nostr::event::Tag::parse(vec!["p".to_string(), target.public_key().to_hex()]).unwrap(),
        );
        let signed = crate::ffi::signer::sign_builder(builder).unwrap();
        let ev: nostr::event::Event = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev.kind.as_u16(), 31989);
        assert!(soshal_nostr_core::models::verify_event(&ev));
        let mut tampered: serde_json::Value = serde_json::from_str(&signed).unwrap();
        tampered["sig"] = serde_json::json!("0".repeat(128));
        let bad: nostr::event::Event = serde_json::from_value(tampered).unwrap();
        assert!(!soshal_nostr_core::models::verify_event(&bad));
        let entry = soshal_social_core::relations::relation_entry_from_event(
            &soshal_nostr_core::models::NostrEvent::from(&ev),
        );
        assert_eq!(entry["id"], serde_json::json!(ev.id.to_hex()));
        assert_eq!(entry["pubkey"], serde_json::json!(ev.pubkey.to_hex()));
        assert_eq!(entry["content"], serde_json::json!("trusted"));
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_publish_garbage_target_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let result = vouch_publish("zzz-not-a-pubkey".to_string(), "trusted".to_string()).await;
        let err = result.unwrap_err();
        assert!(err.contains("invalid target_pubkey"), "{err}");
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_vouch_publish_self_vouch_case_insensitive_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let uppercase_my_pk = keys.public_key().to_hex().to_uppercase();
        let result = vouch_publish(uppercase_my_pk, "trusted".to_string()).await;
        let err = result.unwrap_err();
        assert!(err.contains("cannot vouch for yourself"), "{err}");
        super::super::signer::signer_lock().unwrap();
    }
}
