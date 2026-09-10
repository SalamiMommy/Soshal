//! Binary delta patching (VCDIFF / RFC 3284) for P2P feed sync.
//!
//! Instead of shipping a full event payload every time a peer syncs, the
//! responder sends a small VCDIFF delta derived from the base event's raw
//! bytes. The requester applies it against its stored `raw_json`, then
//! verifies the reconstructed bytes hash to the target event id (SHA-256 for
//! Nostr events) before persisting — a corrupt or malicious patch can never
//! poison the store.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use oxidelta::compress::encoder::CompressOptions;
use oxidelta::compress::{decoder, encoder};
use serde::{Deserialize, Serialize};

/// Wire representation of a binary patch between two event payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventPatch {
    /// Event id whose raw bytes are the patch base (must be stored locally).
    pub base_id: String,
    /// Expected event id of the reconstructed payload.
    pub new_id: String,
    /// VCDIFF delta bytes, base64 for text-safe P2P envelopes.
    pub patch_b64: String,
    /// Full payload fallback when the delta would be larger than the data
    /// (e.g. the base is unrelated or tiny).
    pub full_b64: Option<String>,
}

/// Computes a delta `old -> new` for the given event ids. Falls back to a
/// full payload when the delta is not a win (close to or bigger than `new`).
pub fn compute_event_patch(
    base_id: &str,
    new_id: &str,
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<EventPatch, String> {
    let mut delta_buf = Vec::with_capacity(new_bytes.len().min(4096));
    encoder::encode_all(
        &mut delta_buf,
        old_bytes,
        new_bytes,
        CompressOptions::default(),
    )
    .map_err(|e| format!("vcdiff encode failed: {e}"))?;
    let (patch_b64, full_b64) = if delta_buf.len() >= new_bytes.len() {
        (String::new(), Some(B64.encode(new_bytes)))
    } else {
        (B64.encode(&delta_buf), None)
    };
    Ok(EventPatch {
        base_id: base_id.to_string(),
        new_id: new_id.to_string(),
        patch_b64,
        full_b64,
    })
}

/// Applies `patch` to `old_bytes`, verifying the result hashes to
/// `patch.new_id` (SHA-256 hex, the Nostr event id scheme).
pub fn apply_event_patch(old_bytes: &[u8], patch: &EventPatch) -> Result<Vec<u8>, String> {
    if let Some(full) = &patch.full_b64 {
        let bytes = B64.decode(full).map_err(|e| format!("full b64: {e}"))?;
        return verify_new_id(bytes, &patch.new_id);
    }
    let delta = B64
        .decode(&patch.patch_b64)
        .map_err(|e| format!("patch b64: {e}"))?;
    let new_bytes =
        decoder::decode_all(old_bytes, &delta).map_err(|e| format!("vcdiff apply failed: {e}"))?;
    verify_new_id(new_bytes, &patch.new_id)
}

fn verify_new_id(bytes: Vec<u8>, expected: &str) -> Result<Vec<u8>, String> {
    let mut expected_bytes = [0u8; 32];
    if expected.len() != 64 || hex::decode_to_slice(expected, &mut expected_bytes).is_err() {
        return Err(format!("invalid expected hash hex: {expected}"));
    }
    let actual_bytes = soshal_crypto_core::hash::sha256(&bytes);
    if actual_bytes == expected_bytes {
        Ok(bytes)
    } else {
        Err(format!(
            "patch result hash mismatch: got {}, expected {expected}",
            hex::encode(actual_bytes)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_roundtrip_tiny_change() {
        let old: Vec<u8> = (0..8192).map(|i| (i % 251) as u8).collect();
        let mut new = old.clone();
        new[4096] = 99;
        let new_id = soshal_crypto_core::hash::sha256_hex(&new);

        let patch = compute_event_patch("base", &new_id, &old, &new).unwrap();
        assert!(patch.full_b64.is_none());
        let applied = apply_event_patch(&old, &patch).unwrap();
        assert_eq!(applied, new);
    }

    #[test]
    fn real_json_mutation_wins() {
        let old = br#"{"id":"x","content":"this is a feed post body","pubkey":"abc","created_at":100,"tags":[["t","a"],["t","b"]]}"#.to_vec();
        let new = br#"{"id":"x","content":"this is a feed post body","pubkey":"abc","created_at":100,"tags":[["t","a"],["t","b"],["t","c"]]}"#.to_vec();
        let new_id = soshal_crypto_core::hash::sha256_hex(&new);
        let patch = compute_event_patch("base", &new_id, &old, &new).unwrap();
        assert!(patch.full_b64.is_none());
        assert!(B64.decode(&patch.patch_b64).unwrap().len() < new.len());
        assert_eq!(apply_event_patch(&old, &patch).unwrap(), new);
    }

    #[test]
    fn patch_larger_falls_back_to_full() {
        // Incompressible target unrelated to the 1-byte base: delta encoding
        // cannot win, so the full payload is used.
        let old = b"x".to_vec();
        let mut seed: u64 = 0x9e3779b97f4a7c15;
        let mut new = Vec::with_capacity(4096);
        for _ in 0..4096 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            new.push(seed as u8);
        }
        let new_id = soshal_crypto_core::hash::sha256_hex(&new);
        let patch = compute_event_patch("base", &new_id, &old, &new).unwrap();
        assert!(patch.full_b64.is_some());
        assert_eq!(apply_event_patch(&old, &patch).unwrap(), new);
    }

    #[test]
    fn corrupt_patch_rejected() {
        let old = br#"{"content":"before"}"#.to_vec();
        let new = br#"{"content":"after"}"#.to_vec();
        let new_id = soshal_crypto_core::hash::sha256_hex(&new);
        let mut patch = compute_event_patch("base", &new_id, &old, &new).unwrap();
        // Corrupt the fallback: decodes fine but hash mismatches.
        patch.full_b64 = Some(B64.encode([1u8; 64]));
        assert!(apply_event_patch(&old, &patch).is_err());
    }

    #[test]
    fn mismatched_new_id_rejected() {
        let old = br#"{"content":"before"}"#.to_vec();
        let new = br#"{"content":"after"}"#.to_vec();
        let mut patch = compute_event_patch("base", &"00".repeat(32), &old, &new).unwrap();
        patch.new_id = "ff".repeat(32);
        assert!(apply_event_patch(&old, &patch).is_err());
    }
}
