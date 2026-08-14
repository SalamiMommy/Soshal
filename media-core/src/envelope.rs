//! Envelope encryption for Blossom media.
//!
//! Media ciphertext is stored on Blossom servers; the envelope holds the
//! decryption material so plaintext never leaves the client:
//! - DM media: hybrid (X25519 + ML-KEM-768) one-shot seal to the recipient's
//!   published hybrid public key.
//! - Group media: NIP-44 under the group's shared key.

const MEDIA_ENVELOPE_DOMAIN: &[u8] = b"soshal-media-envelope-v1";

/// Seals media for a DM recipient. Returns `(hybrid_ct_hex, nonce_hex,
/// payload_b64)` — publish the payload (with the ct/nonce) to Blossom.
pub fn seal_dm(data: &[u8], peer_pk_hex: &str) -> Result<(String, String, String), String> {
    soshal_pqc_core::seal::hybrid_seal(data, peer_pk_hex, MEDIA_ENVELOPE_DOMAIN)
}

/// Opens media sealed with [`seal_dm`].
pub fn open_dm(
    ct_hex: &str,
    nonce_hex: &str,
    payload_b64: &str,
    sk_hex: &str,
) -> Result<Vec<u8>, String> {
    soshal_pqc_core::seal::hybrid_unseal(
        payload_b64,
        nonce_hex,
        ct_hex,
        sk_hex,
        MEDIA_ENVELOPE_DOMAIN,
    )
}

/// Seals media under a group's shared key (NIP-44). Returns a JSON envelope
/// `{"v":1,"payload":…}` suitable for storage on Blossom.
pub fn seal_group(data: &[u8], group_key_hex: &str) -> Result<String, String> {
    let key = group_key_bytes(group_key_hex)?;
    let payload = soshal_crypto_core::nip44::encrypt(data, &key)?;
    Ok(serde_json::json!({ "v": 1, "payload": payload }).to_string())
}

/// Opens media sealed with [`seal_group`].
pub fn open_group(envelope: &str, group_key_hex: &str) -> Result<Vec<u8>, String> {
    let v: serde_json::Value =
        serde_json::from_str(envelope).map_err(|e| format!("envelope: {e}"))?;
    let payload = v["payload"].as_str().ok_or("envelope has no payload")?;
    let key = group_key_bytes(group_key_hex)?;
    Ok(soshal_crypto_core::nip44::decrypt(payload, &key)?)
}

fn group_key_bytes(group_key_hex: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(group_key_hex).map_err(|_| "bad group key hex".to_string())?;
    if bytes.len() != 32 {
        return Err("group key must be 32 bytes".to_string());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}
