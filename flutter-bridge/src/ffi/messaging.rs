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
    seal_dm_content_with_key(content, &key)
}

/// `seal_dm_content` with an already-fetched at-rest key (batch callers).
fn seal_dm_content_with_key(content: String, key: &[u8; 32]) -> Result<String, String> {
    let sealed = soshal_crypto_core::at_rest::seal_at_rest(key, content.as_bytes())?;
    Ok(format!("seal1:{sealed}"))
}

/// Unseal DM content with an already-fetched at-rest key (callers should
/// fetch the key once per batch, not per row). Legacy plaintext rows
/// (pre-seal) pass through unchanged. The `seal1:` prefix is stripped
/// repeatedly (bounded): rows written by the old sync path were sealed
/// twice (`seal1:seal1:…`) and heal on first fetch.
fn unseal_dm_content_with_key(stored: String, key: &[u8; 32]) -> Result<String, String> {
    let mut current = stored;
    for _ in 0..3 {
        let Some(sealed) = current.strip_prefix("seal1:") else {
            // No seal prefix — legacy plaintext row, return as-is.
            return Ok(current);
        };
        match soshal_crypto_core::at_rest::open_at_rest(key, sealed) {
            Ok(plain) => match String::from_utf8(plain) {
                Ok(text) => current = text,
                Err(_) => {
                    // Decrypted bytes are not valid UTF-8: the row is corrupt.
                    // Surface a clear sentinel rather than the raw ciphertext.
                    return Err("decrypted content is not valid UTF-8 (corrupt row)".to_string());
                }
            },
            Err(_) => {
                // Decryption failed: wrong key or tampered ciphertext.
                // Return an error so callers (and the UI) can show a warning
                // instead of silently surfacing the raw sealed blob.
                return Err("message decryption failed (tampered or wrong key)".to_string());
            }
        }
    }
    Ok(current)
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
    let key = super::signer::signer_at_rest_key()?;
    super::db::with_db_result(|db| {
        let repo = MessageRepo::new(db);
        let rows = repo.get_conversation(&cid, limit, None)?;
        let dms: Vec<DirectMessage> = rows
            .into_iter()
            .map(|row| {
                let is_own = row.pubkey == my_pk;
                // Individual decryption failures produce a sentinel instead of
                // aborting the whole batch: the UI can show an inline warning.
                let (content, decrypted) = match unseal_dm_content_with_key(row.content, &key) {
                    Ok(text) => (text, true),
                    Err(_) => ("[message could not be decrypted]".to_string(), false),
                };
                Ok::<DirectMessage, soshal_db_core::error::DbError>(DirectMessage {
                    id: row.id,
                    sender: row.pubkey,
                    content,
                    created_at: row.created_at.max(0) as u64,
                    decrypted,
                    is_own,
                })
            })
            .collect::<Result<Vec<DirectMessage>, soshal_db_core::error::DbError>>()?;
        Ok(dms)
    })
    .map(super::util::json_ok)?
}

/// Fetch all conversation partner pubkeys for the active account, most
/// recent first. Reads the maintained `conversations` table (v011) instead
/// of scanning + grouping every message row.
#[frb(sync, serialize)]
pub fn messaging_fetch_conversations(pubkey: String) -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let cids: Vec<String> = soshal_db_core::query::query(
            &conn,
            "SELECT conversation_id FROM conversations WHERE conversation_id LIKE 'conv:%' \
             ORDER BY last_message_at DESC LIMIT 100",
            (),
            |r| r.get(0),
        )?;
        let mut seen = std::collections::HashSet::new();
        let mut peers: Vec<String> = Vec::new();
        for cid in cids {
            for p in cid.trim_start_matches("conv:").split(':') {
                if p != pubkey && seen.insert(p.to_string()) {
                    peers.push(p.to_string());
                }
            }
        }
        Ok(peers)
    })
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

/// Store a batch of received DM rows (live-stream bursts). One FFI crossing
/// + one transaction-ish upsert loop instead of one call per message.
#[frb(sync, serialize)]
pub fn messaging_store_dms(dms_json: String) -> Result<bool, String> {
    #[derive(serde::Deserialize)]
    struct DmIn {
        id: String,
        sender: String,
        recipient: String,
        content: String,
        created_at: u64,
    }
    let dms: Vec<DmIn> =
        serde_json::from_str(&dms_json).map_err(|e| format!("invalid DMs JSON: {e}"))?;
    if dms.is_empty() {
        return Ok(true).into();
    }
    let key = super::signer::signer_at_rest_key()?;
    super::db::with_db_result(|db| {
        let repo = MessageRepo::new(db);
        let rows: Vec<soshal_db_core::repos::message::MessageRow> = dms
            .iter()
            .map(|dm| {
                let sealed = seal_dm_content_with_key(dm.content.clone(), &key)
                    .map_err(soshal_db_core::error::DbError::Migration)?;
                let cid = conv_id(&dm.sender, &dm.recipient);
                Ok(soshal_db_core::repos::message::MessageRow {
                    id: dm.id.clone(),
                    conversation_id: cid,
                    pubkey: dm.sender.clone(),
                    content: sealed,
                    created_at: dm.created_at as i64,
                    tags_json: "[]".to_string(),
                    reply_to: None,
                    sync_status: "synced".to_string(),
                    is_deleted: false,
                })
            })
            .collect::<Result<Vec<_>, soshal_db_core::error::DbError>>()?;
        // One transaction for the whole burst instead of N autocommits.
        repo.upsert_batch(&rows)?;
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
    let participants: Vec<String> = serde_json::from_str(&participant_pubkeys_json)
        .map_err(|e| format!("invalid participants JSON: {e}"))?;
    let payload = serde_json::json!({ "text": content, "groupId": group_id }).to_string();

    // Check if a group key exists in local DB
    let sealed_payload = super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::group::GroupRepo::new(db);
        let key = repo.get_shared_key(&group_id)?;
        Ok(match key {
            Some(k) => {
                let k = match k.strip_prefix("seal1:") {
                    Some(sealed) => {
                        let plain = soshal_crypto_core::at_rest::open_at_rest(
                            &super::signer::signer_at_rest_key()
                                .map_err(soshal_db_core::error::DbError::Migration)?,
                            sealed,
                        )
                        .map_err(soshal_db_core::error::DbError::Migration)?;
                        hex::encode(plain)
                    }
                    None => k,
                };
                Some(
                    soshal_groups_core::group_enc::seal::group_message_envelope(&payload, Some(&k))
                        .map_err(soshal_db_core::error::DbError::Migration)?,
                )
            }
            None => None,
        })
    })?;

    // Sealed path tags only the group; p-tagging every participant would
    // leak the full participant list to relays. The unsealed fallback tags
    // the single NIP-44 recipient so the relay can route it.
    let fallback_recipient = participants.first().cloned();
    let event_content = if let Some(sp) = sealed_payload.clone() {
        sp
    } else if let Some(first_peer) = fallback_recipient.clone() {
        super::signer::signer_nip44_encrypt(payload, first_peer)?
    } else {
        soshal_groups_core::group_enc::seal::group_message_envelope(&payload, None)?
    };

    let mut builder = EventBuilder::new(Kind::EncryptedDirectMessage, event_content);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["g".to_string(), group_id]) {
        builder = builder.tag(tag);
    }
    if sealed_payload.is_none() {
        if let Some(peer) = fallback_recipient {
            if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), peer]) {
                builder = builder.tag(tag);
            }
        }
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
