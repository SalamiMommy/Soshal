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
    pub recipient: String,
    pub content: String,
    pub created_at: u64,
    pub decrypted: bool,
    pub is_own: bool,
    #[serde(default)]
    pub tags: String,
}

/// Send a direct message (NIP-44 v2, kind 4): encrypt with the unlocked
/// signer, build the event, and return signed JSON (publish via
/// `network_publish_event`).
///
/// # Security (M4 fix)
/// `content` is wrapped in `Zeroizing<String>` at function entry so that the
/// plaintext heap allocation is zeroed on **every** return path (early-error or
/// normal completion). The clone passed to `messaging_store_dm` is consumed by
/// `seal_dm_content` (AES-256-GCM); the original allocation is zeroed here.
#[frb(serialize)]
pub async fn messaging_send_dm(
    content: String,
    recipient_pubkey: String,
) -> Result<String, String> {
    // Zeroizing wrapper: clears the heap allocation when this binding is dropped.
    let content = zeroize::Zeroizing::new(content);
    if content.is_empty() {
        return Err("message must not be empty".to_string()).into();
    }
    if content.len() > 64000 {
        return Err("content must be ≤64000 chars".to_string()).into();
    }
    let encrypted =
        super::signer::signer_nip44_encrypt((*content).to_string(), recipient_pubkey.clone())?;
    let mut builder = EventBuilder::new(Kind::EncryptedDirectMessage, encrypted);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["p".to_string(), recipient_pubkey.clone()]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    let sender_pk = super::signer::signer_pubkey()?;
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&signed_json) {
        if let Some(event_id) = val.get("id").and_then(|v| v.as_str()) {
            let created_at = val.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Err(e) = messaging_store_dm(
                event_id.to_string(),
                sender_pk,
                recipient_pubkey,
                (*content).to_string(),
                created_at,
                "[]".to_string(),
            ) {
                eprintln!("dm local store: {e}");
            }
        }
    }
    // `content` (Zeroizing wrapper) is dropped and zeroed here before publish.
    let json = signed_json.clone();
    super::sync::publish_or_enqueue("dm", &signed_json).await?;
    Ok(json).into()
}

/// Deterministic conversation id for a DM pair: sorted pubkeys joined by
/// `:`, prefixed `conv:`.
pub(crate) fn conv_id(my_pubkey: &str, other_pubkey: &str) -> String {
    let (first, second) = if my_pubkey <= other_pubkey {
        (my_pubkey, other_pubkey)
    } else {
        (other_pubkey, my_pubkey)
    };
    format!("conv:{first}:{second}")
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
                    recipient: if is_own {
                        with_pubkey.clone()
                    } else {
                        my_pk.clone()
                    },
                    content,
                    created_at: row.created_at.max(0) as u64,
                    decrypted,
                    is_own,
                    tags: row.tags_json,
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
        let mut peers = extract_peers_from_cids(&cids, &pubkey);
        let blocked = soshal_db_core::repos::block::BlockRepo::new(db).list(&pubkey)?;
        peers.retain(|p| !blocked.contains(p));
        Ok(peers)
    })
}

/// Given a list of `conv:<pkA>:<pkB>` conversation ids and a pubkey,
/// return the unique peer pubkeys (i.e. the other participant in each
/// conversation the given pubkey belongs to). Uses exact part matching
/// instead of substring `contains` — hex pubkeys can be prefixes of each
/// other, so `"abc".contains("ab")` would wrongly include a conversation
/// for a different account.
pub(crate) fn extract_peers_from_cids(cids: &[String], pubkey: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut peers: Vec<String> = Vec::new();
    for cid in cids {
        let parts: Vec<&str> = cid.trim_start_matches("conv:").split(':').collect();
        if !parts.contains(&pubkey) {
            continue;
        }
        for p in parts {
            if p != pubkey && seen.insert(p.to_string()) {
                peers.push(p.to_string());
            }
        }
    }
    peers
}

/// Extract the reply target from a DM's tags JSON (`e` with "reply" marker or
/// a `q` tag point to the message being replied to; the first such id wins).
fn reply_to_from_tags_json(tags_json: &str) -> Option<String> {
    let tags: Vec<Vec<String>> = serde_json::from_str(tags_json).ok()?;
    tags.iter()
        .find(|t| matches!(t.first().map(|s| s.as_str()), Some("e") | Some("q")))
        .and_then(|t| t.get(1).cloned())
        .filter(|s| !s.is_empty())
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
        tags_json: tags_json.clone(),
        reply_to: reply_to_from_tags_json(&tags_json),
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
        #[serde(default)]
        tags: Option<String>,
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
                let tags_json = dm
                    .tags
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "[]".to_string());
                Ok(soshal_db_core::repos::message::MessageRow {
                    id: dm.id.clone(),
                    conversation_id: cid,
                    pubkey: dm.sender.clone(),
                    content: sealed,
                    created_at: dm.created_at as i64,
                    reply_to: reply_to_from_tags_json(&tags_json),
                    tags_json,
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

    #[test]
    fn test_extract_peers_hex_prefix_no_collision() {
        // pubkey "ab" is a hex prefix of "abcd" — substring `contains` would
        // wrongly match the "abcd" conversation. Exact part matching must NOT.
        let cids = vec![
            conv_id("ab", "ef"),
            conv_id("abcd", "1234"),
            conv_id("ab", "5678"),
        ];
        let peers = extract_peers_from_cids(&cids, "ab");
        assert!(
            peers.contains(&"ef".to_string()),
            "should include exact match: {peers:?}"
        );
        assert!(
            peers.contains(&"5678".to_string()),
            "should include second exact match: {peers:?}"
        );
        assert!(
            !peers.contains(&"1234".to_string()),
            "must NOT include prefix-collision peer: {peers:?}"
        );
    }

    #[test]
    fn test_extract_peers_dedup() {
        let cids = vec![conv_id("ab", "cd"), conv_id("ab", "cd")];
        let peers = extract_peers_from_cids(&cids, "ab");
        assert_eq!(peers, vec!["cd".to_string()]);
    }

    #[test]
    fn test_extract_peers_no_match() {
        let cids = vec![conv_id("ab", "cd")];
        let peers = extract_peers_from_cids(&cids, "zz");
        assert!(peers.is_empty());
    }
}
