//! At-rest encryption for key material that must persist in the local DB.
//!
//! v2 (current): AES-256-GCM under a key derived from the keychain-held nsec
//! (layer 1), plus a second wrap layer — a random hybrid (X25519 + ML-KEM-768)
//! keypair held in the platform keychain. Opening an envelope requires both
//! the master secret AND the keychain hybrid secret key: a leaked DB plus the
//! nsec alone is not sufficient. The hybrid public key travels in the
//! envelope, so only the secret key stays out of the DB.
//!
//! v1 blobs (plain AES-GCM, hex-encoded) are still readable for migration;
//! every write re-seals to v2.

use crate::hash::{hkdf_sha256, sha256};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use zeroize::Zeroize;

/// HKDF info parameter for v1 at-rest key derivation.
/// Also used as the inner-layer info for v2 envelope sealing (the v2 outer
/// wrap adds an additional hybrid KEM layer on top of the v1 AES-GCM blob).
const AT_REST_V1_INFO: &[u8] = b"soshal-at-rest-v1";

/// Derives the 32-byte at-rest key from the keychain master secret (nsec).
pub fn at_rest_key(master: &[u8]) -> Result<[u8; 32], String> {
    if master.is_empty() {
        return Err("at-rest master secret is empty".into());
    }
    // Hash the master first so a multi-entry keychain never leaks via HKDF info.
    let mut ikm = sha256(master);
    let okm = hkdf_sha256(&ikm, b"soshal-at-rest-salt", AT_REST_V1_INFO, 32);
    ikm.zeroize();
    let okm = okm?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm);
    Ok(key)
}

/// Seals `plaintext` and returns hex(nonce ‖ ciphertext ‖ tag).
pub fn seal_at_rest(key: &[u8; 32], plaintext: &[u8]) -> Result<String, String> {
    Ok(hex::encode(seal_at_rest_bin(key, plaintext)?))
}

/// Seals `plaintext` and returns binary `nonce ‖ ciphertext ‖ tag` (no hex
/// expansion — used for large blobs like encrypted DB backups).
pub fn seal_at_rest_bin(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|e| format!("rng: {e}"))?;
    let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|e| format!("key: {e}"))?;
    let sealing = LessSafeKey::new(unbound);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    sealing
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|e| format!("seal: {e}"))?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&in_out);
    Ok(out)
}

/// Opens a blob produced by `seal_at_rest`. Returns the plaintext or `Err`
/// on any tampering, wrong key or malformed input.
pub fn open_at_rest(key: &[u8; 32], sealed_hex: &str) -> Result<Vec<u8>, String> {
    let blob = hex::decode(sealed_hex).map_err(|e| format!("sealed blob: {e}"))?;
    open_at_rest_bin(key, &blob)
}

/// Opens a binary blob produced by [`seal_at_rest_bin`].
pub fn open_at_rest_bin(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() <= NONCE_LEN {
        return Err("sealed blob too short".into());
    }
    let (nonce_bytes, body) = blob.split_at(NONCE_LEN);
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(nonce_bytes);
    let unbound = UnboundKey::new(&AES_256_GCM, key).map_err(|e| format!("key: {e}"))?;
    let opening = LessSafeKey::new(unbound);
    let nonce = Nonce::assume_unique_for_key(nonce_arr);
    let mut buf = body.to_vec();
    let plaintext = opening
        .open_in_place(nonce, Aad::empty(), &mut buf)
        .map_err(|_| "decryption failed (tampered or wrong key)".to_string())?;
    Ok(plaintext.to_vec())
}

// ─── v2: AES-GCM + hybrid keychain wrap ───────────────────────────────

const AT_REST_V2_DOMAIN: &[u8] = b"soshal-at-rest-v2";

/// Opt-in: untagged legacy blobs (v1 hex, pre-tag v2) still open when true.
pub const ALLOW_UNTAGGED_V1: bool = true;

const AT_REST_V2_INNER_TAG: &[u8] = b"soshal-at-rest-v2\x00";

/// v2 identity-derived key (HKDF path under the v2 domain). Deterministic per
/// nsec, so two devices of the same identity derive the same value — used for
/// cross-device secrets (LAN sync tokens, beacon MAC keys) that must NOT
/// depend on the per-device random keychain wrap keypair.
pub fn at_rest_key_v2(master: &[u8]) -> Result<[u8; 32], String> {
    if master.is_empty() {
        return Err("at-rest master secret is empty".into());
    }
    let ikm = sha256(master);
    let okm = hkdf_sha256(&ikm, b"soshal-at-rest-salt", AT_REST_V2_DOMAIN, 32)?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm);
    Ok(key)
}

/// Seals `plaintext` with the v2 envelope: layer-1 AES-GCM under the
/// master-derived key, then a hybrid one-shot seal of the layer-1 blob to
/// `wrap_pk_hex` (the keychain-held hybrid keypair's public key). Returns a
/// JSON envelope `{"v":2,"ct":…,"nonce":…,"payload":…}`.
pub fn seal_at_rest_v2(
    master_key: &[u8; 32],
    wrap_pk_hex: &str,
    plaintext: &[u8],
) -> Result<String, String> {
    let inner = seal_at_rest(master_key, plaintext)?;
    let mut tagged = Vec::with_capacity(AT_REST_V2_INNER_TAG.len() + inner.len());
    tagged.extend_from_slice(AT_REST_V2_INNER_TAG);
    tagged.extend_from_slice(inner.as_bytes());
    let (ct_hex, nonce, payload) =
        soshal_pqc_core::seal::hybrid_seal(&tagged, wrap_pk_hex, AT_REST_V2_DOMAIN)?;
    Ok(serde_json::json!({
        "v": 2,
        "ct": ct_hex,
        "nonce": nonce,
        "payload": payload,
    })
    .to_string())
}

/// Opens a v2 envelope (or falls back to a legacy v1 blob). Requires the
/// keychain-held hybrid secret key: without it, opening fails even when the
/// master secret is known.
pub fn open_at_rest_v2(
    master_key: &[u8; 32],
    wrap_sk_hex: &str,
    blob: &str,
) -> Result<Vec<u8>, String> {
    let v2 = blob
        .strip_prefix('{')
        .and_then(|_| serde_json::from_str::<serde_json::Value>(blob).ok())
        .filter(|v| v["v"].as_u64() == Some(2))
        .and_then(|v| {
            let ct = v["ct"].as_str()?.to_string();
            let nonce = v["nonce"].as_str()?.to_string();
            let payload = v["payload"].as_str()?.to_string();
            Some((ct, nonce, payload))
        });
    let Some((ct, nonce, payload)) = v2 else {
        // M-2 fix: only treat the blob as a legacy v1 hex blob when it is
        // actually valid hex. A non-JSON, non-hex blob is corrupted or
        // crafted to bypass the KEM wrap — reject it rather than attempting
        // decryption under the deterministic identity key alone.
        let is_hex = !blob.is_empty()
            && blob.len().is_multiple_of(2)
            && blob.bytes().all(|b| b.is_ascii_hexdigit());
        if !is_hex {
            return Err(
                "at-rest blob is neither a v2 JSON envelope nor a valid v1 hex blob".into(),
            );
        }
        if !ALLOW_UNTAGGED_V1 {
            return Err("legacy v1 blob rejected: untagged blobs disabled".into());
        }
        // Legacy v1 blob (plain hex-encoded AES-GCM): readable for migration.
        return open_at_rest(master_key, blob);
    };
    let mut inner = soshal_pqc_core::seal::hybrid_unseal(
        &payload,
        &nonce,
        &ct,
        wrap_sk_hex,
        AT_REST_V2_DOMAIN,
    )?;
    let inner_hex = match inner.strip_prefix(AT_REST_V2_INNER_TAG) {
        Some(tagged) => tagged,
        None if ALLOW_UNTAGGED_V1 => &inner[..],
        None => {
            inner.zeroize();
            return Err("v2 envelope missing version marker; untagged disabled".into());
        }
    };
    let inner_str = match std::str::from_utf8(inner_hex) {
        Ok(s) => s.to_string(),
        Err(_) => {
            inner.zeroize();
            return Err("v2 inner blob is not UTF-8 (wrong key?)".to_string());
        }
    };
    inner.zeroize();
    open_at_rest(master_key, &inner_str)
}
