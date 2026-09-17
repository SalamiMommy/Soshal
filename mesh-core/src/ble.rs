//! BLE offline sync: peer-name codec, chunked transfer framing, and the
//! PQC-KEM + NIP-44 + DSA sync envelope.
//!
//! Protocol parity with the legacy `BleSyncService` +
//! `P2PSyncCryptoProvider` (same device-name prefix, characteristic UUIDs,
//! `i:count:chunk` framing, and envelope fields) so Rust-native and legacy
//! devices interoperate. Safety caps mirror the rest of the codebase:
//! payloads are bounded before any crypto work and decompression is
//! capped.

use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use soshal_content_core::compress;
use soshal_crypto_core::nip44;
use soshal_pqc_core::{dsa, hybrid};

/// BLE GATT service UUID for Soshal sync (legacy-compatible).
pub const SERVICE_UUID: &str = "4fafc201-1fb5-459e-8fcc-c5c9c331914b";
/// BLE GATT characteristic UUID for chunked sync payloads (legacy-compatible).
pub const CHARACTERISTIC_UUID: &str = "beb5483e-36e1-4688-b7f5-ea07361b26a8";
/// Advertising/device-name prefix: `SOSHAL_<first 12 pubkey chars>`.
pub const DEVICE_NAME_PREFIX: &str = "SOSHAL_";
/// Number of pubkey characters carried in the device name (legacy parity).
pub const DEVICE_NAME_PUBKEY_CHARS: usize = 12;
/// BLE default MTU when negotiation fails (23 bytes, 20 user payload).
pub const DEFAULT_MTU: usize = 23;
/// Maximum negotiated MTU (legacy parity: 512).
pub const MAX_MTU: usize = 512;
/// Hard cap on a single sync payload before compression.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
/// Domain-separation context for the hybrid KEM (legacy parity:
/// `soshal-ble-sync-v1`).
pub const SYNC_CONTEXT: &str = "soshal-ble-sync-v1";

/// Builds the BLE device name for a pubkey:
/// `SOSHAL_<first 12 hex chars>`.
pub fn device_name(pubkey: &str) -> String {
    let frag: String = pubkey
        .trim()
        .to_ascii_lowercase()
        .chars()
        .take(DEVICE_NAME_PUBKEY_CHARS)
        .collect();
    format!("{DEVICE_NAME_PREFIX}{frag}")
}

/// Extracts the pubkey fragment from a Soshal BLE device name. Returns
/// `None` for unknown names or non-hex fragments.
pub fn pubkey_fragment_from_device_name(name: &str) -> Option<String> {
    if !name.starts_with(DEVICE_NAME_PREFIX) {
        return None;
    }
    let frag = &name[DEVICE_NAME_PREFIX.len()..];
    if frag.is_empty() || !frag.as_bytes().iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(frag.to_ascii_lowercase())
}

/// Usable chunk size for a given negotiated MTU. BLE default MTU carries
/// 20 user bytes; the framing adds `i:count:` → 3-byte overhead.
pub fn chunk_size(mtu: usize) -> usize {
    let mtu = mtu.clamp(DEFAULT_MTU, MAX_MTU);
    (mtu - 3).max(20)
}

/// Splits a payload string into base64 chunk lines framed as
/// `i:count:chunk` (legacy wire format). Chunk content is base64-encoded
/// to stay ASCII-safe.
pub fn split_chunks(data: &str, mtu: usize) -> Vec<String> {
    let size = chunk_size(mtu);
    let chars: Vec<char> = data.chars().collect();
    let chunk_count = chars.len().div_ceil(size);
    let mut chunks = Vec::with_capacity(chunk_count);
    for (i, piece) in chars.chunks(size).enumerate() {
        let piece_str: String = piece.iter().collect();
        let line = format!("{i}:{chunk_count}:{piece_str}");
        chunks.push(general_purpose::STANDARD.encode(line.as_bytes()));
    }
    chunks
}

/// Reassembles base64 chunk lines into the original payload string.
/// Returns `None` when the set is incomplete, duplicated, or ordered
/// inconsistently with the declared chunk count.
pub fn reassemble_chunks(lines: &[String]) -> Option<String> {
    let mut map: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    let mut declared = None;
    let mut total_len = 0usize;
    for line in lines {
        if line.len() > MAX_MTU * 4 {
            return None;
        }
        let decoded = general_purpose::STANDARD.decode(line).ok()?;
        let text = String::from_utf8(decoded).ok()?;
        let mut parts = text.splitn(3, ':');
        let idx: usize = parts.next()?.parse().ok()?;
        let count: usize = parts.next()?.parse().ok()?;
        if count == 0 || count > 4096 {
            return None;
        }
        let data = parts.next()?;
        if data.len() > MAX_MTU {
            return None;
        }
        if declared.map(|c: usize| c != count).unwrap_or(false) {
            return None;
        }
        declared = Some(count);
        total_len += data.len();
        if total_len > MAX_PAYLOAD_BYTES * 2 {
            return None;
        }
        if map.insert(idx, data.to_string()).is_some() {
            return None;
        }
    }
    let count = declared?;
    if count == 0 || map.len() != count {
        return None;
    }
    let mut out = String::new();
    for i in 0..count {
        out.push_str(map.get(&i)?);
    }
    Some(out)
}

/// Encrypted sync envelope (legacy wire shape):
/// `{pqc_ct, inner, senderPubkey, dsaSig}` with the DSA signature bound to
/// `"<ciphertext>:<context>"`.
#[derive(Deserialize)]
pub struct SyncEnvelope {
    #[serde(rename = "pqc_ct")]
    pub pqc_ct: String,
    pub inner: String,
    #[serde(rename = "senderPubkey")]
    pub sender_pubkey: String,
    #[serde(rename = "dsaSig")]
    pub dsa_sig: String,
}

fn hex_to_key32(hex_str: &str) -> Option<[u8; nip44::KEY_LEN]> {
    let raw = hex::decode(hex_str).ok()?;
    if raw.len() != nip44::KEY_LEN {
        return None;
    }
    let mut key = [0u8; nip44::KEY_LEN];
    key.copy_from_slice(&raw);
    Some(key)
}

/// Encrypts a payload for a peer: compress → hybrid KEM (domain
/// `soshal-ble-sync-v1`) → NIP-44, then DSA-signs `ciphertext:context` with
/// the sender's ML-DSA key. Returns the envelope JSON or an error.
pub fn encrypt_envelope(
    payload: &str,
    peer_pk_hex: &str,
    dsa_sk_hex: &str,
    sender_pubkey: &str,
) -> Result<String, String> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(format!("payload exceeds {MAX_PAYLOAD_BYTES} bytes"));
    }
    let compressed = compress::compress_json(payload);
    let (ct, ss_hex) = hybrid::hybrid_encapsulate(peer_pk_hex, SYNC_CONTEXT.as_bytes())
        .map_err(|e| format!("kem encapsulate: {e}"))?;
    let key = hex_to_key32(&ss_hex).ok_or("kem shared secret decode")?;
    let inner = nip44::encrypt(compressed.as_bytes(), &key).map_err(|e| format!("nip44: {e}"))?;
    let sign_msg = format!("{ct}:{SYNC_CONTEXT}");
    let dsa_sig = dsa::dsa_sign(sign_msg.as_bytes(), dsa_sk_hex).ok_or("dsa sign")?;
    let envelope = serde_json::json!({
        "pqc_ct": ct,
        "inner": inner,
        "senderPubkey": sender_pubkey,
        "dsaSig": dsa_sig,
    });
    Ok(envelope.to_string())
}

/// Decrypts a sync envelope with the local hybrid secret key. When the
/// sender's ML-DSA public key is provided, the signature is verified first
/// (rejects forgeries); otherwise authentication relies on KEM + NIP-44
/// integrity. Returns the original payload string.
pub fn decrypt_envelope(
    envelope_json: &str,
    own_sk_hex: &str,
    sender_dsa_pk_hex: Option<&str>,
) -> Result<String, String> {
    if envelope_json.len() > 256 * 1024 {
        return Err("envelope JSON too large (max 256KB)".to_string());
    }
    let envelope: SyncEnvelope =
        serde_json::from_str(envelope_json).map_err(|e| format!("envelope parse: {e}"))?;
    if envelope.pqc_ct.len() != hybrid::HYBRID_CT_LEN * 2 || !envelope.pqc_ct.starts_with("01") {
        return Err("invalid hybrid ciphertext".to_string());
    }
    let sign_msg = format!("{}:{SYNC_CONTEXT}", envelope.pqc_ct);
    if let Some(pk) = sender_dsa_pk_hex {
        if !dsa::dsa_verify_hex(&envelope.dsa_sig, sign_msg.as_bytes(), pk) {
            return Err("sender signature verification failed".to_string());
        }
    }
    let ss = hybrid::hybrid_decapsulate(&envelope.pqc_ct, own_sk_hex, SYNC_CONTEXT.as_bytes())
        .map_err(|e| format!("kem decapsulate: {e}"))?;
    let key = hex_to_key32(&ss).ok_or("kem shared secret decode")?;
    let inner = nip44::decrypt(&envelope.inner, &key).map_err(|e| format!("nip44: {e}"))?;
    let inner_str = String::from_utf8(inner).map_err(|e| format!("inner utf8: {e}"))?;
    let payload = compress::decompress_json(&inner_str);
    if payload.is_empty() {
        return Err("payload decompress failed".to_string());
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_name_and_fragment_case_normalization() {
        let pk = "ABCDEF1234567890";
        let dev = device_name(pk);
        assert_eq!(dev, "SOSHAL_abcdef123456");
        let frag = pubkey_fragment_from_device_name(&dev).unwrap();
        assert_eq!(frag, "abcdef123456");

        // Non-hex or bad prefix
        assert_eq!(pubkey_fragment_from_device_name("OTHER_abcdef123456"), None);
        assert_eq!(
            pubkey_fragment_from_device_name("SOSHAL_NOT_HEX_CHARS"),
            None
        );
    }

    #[test]
    fn test_chunks_roundtrip_and_caps() {
        let payload = "hello world over ble sync!";
        let chunks = split_chunks(payload, 23);
        assert!(!chunks.is_empty());
        let restored = reassemble_chunks(&chunks).unwrap();
        assert_eq!(restored, payload);

        // Hostile chunk count (> 4096)
        let fake_chunk = general_purpose::STANDARD.encode(b"0:5000:data");
        assert_eq!(reassemble_chunks(&[fake_chunk]), None);

        // Zero chunk count
        let zero_chunk = general_purpose::STANDARD.encode(b"0:0:data");
        assert_eq!(reassemble_chunks(&[zero_chunk]), None);
    }

    #[test]
    fn test_decrypt_envelope_oversized_rejected() {
        let huge_env = "x".repeat(256 * 1024 + 1);
        assert!(decrypt_envelope(&huge_env, "00", None).is_err());
    }
}
