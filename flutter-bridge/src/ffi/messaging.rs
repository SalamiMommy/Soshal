//! Messaging FFI module
//!
//! Direct messages (NIP-44 v2 via the unlocked signer) and conversation
//! queries against the shared DB. Group DMs mirror the Tauri kind-1059 path
//! at the event level only; full group key rotation stays server-side.

use flutter_rust_bridge::frb;
use nostr::event::EventBuilder;
use nostr::event::Kind;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::message::MessageRepo;

/// Direct message info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DirectMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub created_at: u64,
    pub decrypted: bool,
    pub is_own: bool,
}

/// Send a direct message (NIP-44 v2, kind 4): encrypt with the unlocked
/// signer, build the event, and return signed JSON (publish via
/// `network_publish_event`).
#[frb(serialize)]
pub async fn messaging_send_dm(
    content: String,
    recipient_pubkey: String,
) -> Result<String, String> {
    if content.is_empty() {
        return Err("message must not be empty".to_string()).into();
    }
    if content.len() > 64000 {
        return Err("content must be ≤64000 chars".to_string()).into();
    }
    let encrypted = super::signer::signer_nip44_encrypt(content, recipient_pubkey.clone())?;
    let mut builder = EventBuilder::new(Kind::EncryptedDirectMessage, encrypted);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), recipient_pubkey]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("dm", &signed_json).await?;
    Ok(signed_json).into()
}

/// Deterministic conversation id for a DM pair: sorted pubkeys joined by
/// `:`, prefixed `conv:`.
pub(crate) fn conv_id(my_pubkey: &str, other_pubkey: &str) -> String {
    let mut pair = [my_pubkey, other_pubkey];
    pair.sort();
    format!("conv:{}", pair.join(":"))
}

/// Seal DM content at rest (AES-GCM under the signer-derived key) before DB
/// writes.
pub(crate) fn seal_dm_content(content: String) -> Result<String, String> {
    let key = super::signer::signer_at_rest_key()?;
    let sealed = soshal_crypto_core::at_rest::seal_at_rest(&key, content.as_bytes())?;
    Ok(format!("seal1:{sealed}"))
}

/// Unseal DM content stored via `seal_dm_content`; legacy plaintext rows
/// (pre-seal) pass through unchanged.
pub(crate) fn unseal_dm_content(stored: String) -> Result<String, String> {
    match stored.strip_prefix("seal1:") {
        Some(sealed) => {
            let key = super::signer::signer_at_rest_key()?;
            let plain = soshal_crypto_core::at_rest::open_at_rest(&key, sealed)?;
            String::from_utf8(plain).map_err(|e| format!("unseal utf8: {e}"))
        }
        None => Ok(stored),
    }
}

/// Fetch the most recent DMs with a peer from the local DB (both directions,
/// newest first).
#[frb(sync, serialize)]
pub fn messaging_fetch_dms(with_pubkey: String, limit: i32) -> Result<String, String> {
    let limit = limit.clamp(1, 500) as i64;
    let my_pk = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let cid = conv_id(&my_pk, &with_pubkey);
    super::db::with_db_result(|db| {
        let repo = MessageRepo::new(db);
        let rows = repo.get_conversation(&cid, limit, None)?;
        let dms: Vec<DirectMessage> = rows
            .into_iter()
            .map(|row| {
                let is_own = row.pubkey == my_pk;
                Ok(DirectMessage {
                    id: row.id,
                    sender: row.pubkey,
                    content: unseal_dm_content(row.content)?,
                    created_at: row.created_at.max(0) as u64,
                    decrypted: false,
                    is_own,
                })
            })
            .collect::<Result<Vec<DirectMessage>, String>>()
            .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(dms)
    })
    .map(super::util::json_ok)?
}

/// Fetch all conversation partner pubkeys for the active account, most
/// recent first.
#[frb(sync, serialize)]
pub fn messaging_fetch_conversations(pubkey: String) -> Result<Vec<String>, String> {
    let json = super::db::db_query_raw(
        "SELECT conversation_id FROM messages WHERE conversation_id LIKE 'conv:%' GROUP BY conversation_id ORDER BY MAX(created_at) DESC".to_string(),
    )?;
    let rows: Vec<serde_json::Value> = match serde_json::from_str(&json) {
        Ok(r) => r,
        Err(e) => return Err(format!("parse conversations: {e}")).into(),
    };
    let mut peers: Vec<String> = Vec::new();
    for row in rows {
        let cid = row["conversation_id"].as_str().unwrap_or("");
        let parts: Vec<&str> = cid.trim_start_matches("conv:").split(':').collect();
        for p in parts {
            if p != pubkey && !peers.iter().any(|x| x == p) {
                peers.push(p.to_string());
            }
        }
    }
    Ok(peers).into()
}

/// Store a received DM row locally (called by the sync loop after
/// `messaging_decrypt_dm` + verification).
#[frb(sync, serialize)]
pub fn messaging_store_dm(
    id: String,
    sender: String,
    recipient: String,
    content: String,
    created_at: u64,
    tags_json: String,
) -> Result<bool, String> {
    let content = seal_dm_content(content)?;
    let cid = conv_id(&sender, &recipient);
    let row = soshal_db_core::repos::message::MessageRow {
        id,
        conversation_id: cid,
        pubkey: sender,
        content,
        created_at: created_at as i64,
        tags_json,
        reply_to: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
    };
    super::db::with_db_result(|db| {
        MessageRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Encrypt a group DM payload for the legacy kind-1059 path (participants
/// handled server-side); signs and returns event JSON.
#[frb(sync, serialize)]
pub fn messaging_send_group_dm(
    content: String,
    group_id: String,
    participant_pubkeys_json: String,
) -> Result<String, String> {
    if content.is_empty() {
        return Err("message must not be empty".to_string()).into();
    }
    let _participants: Vec<String> = serde_json::from_str(&participant_pubkeys_json)
        .map_err(|e| format!("invalid participants JSON: {e}"))?;
    let payload = serde_json::json!({ "text": content, "groupId": group_id }).to_string();
    let mut builder = EventBuilder::new(Kind::EncryptedDirectMessage, payload);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["g".to_string(), group_id]) {
        builder = builder.tag(tag);
    }
    super::signer::sign_builder(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_send_dm_requires_unlocked_signer() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        super::super::signer::signer_lock().unwrap();
        let result = messaging_send_dm("hi".to_string(), "a".repeat(64)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("locked"));
    }

    #[tokio::test]
    async fn test_empty_content_rejected() {
        let result = messaging_send_dm("".to_string(), "a".repeat(64)).await;
        assert!(result.is_err());
    }
}
