//! Pure post-quantum FFI module (ML-KEM-768 / ML-DSA-65).
//!
//! Thin wrappers over `soshal-pqc-core` (kem/dsa/hybrid/seal/hkdf). Unlike the
//! hybrid X25519+ML-KEM surfaces in `crypto.rs`, these are pure ML-KEM/ML-DSA —
//! `pqc_` prefix keeps the two distinct. All inputs/outputs are hex Strings or
//! serde_json JSON objects; no key material crosses to Dart raw.

use flutter_rust_bridge::frb;
use soshal_pqc_core::{dsa, hkdf, seal};

/// KEM domain separation string shared with the hybrid API.
const PQC_DOMAIN: &[u8] = b"soshal-ffi-v1";

/// ML-DSA-65 keypair generation.
/// Returns JSON `{"sk": "<hex>", "vk": "<hex>"}`.
#[frb(sync, serialize)]
pub fn pqc_dsa_keygen() -> Result<String, String> {
    let (sk, vk) = dsa::dsa_keygen(None)?;
    Ok(serde_json::json!({ "sk": sk, "vk": vk }).to_string()).into()
}

/// ML-DSA-65 signing of `message_hex` with the secret key.
/// Returns the signature as hex.
#[frb(sync, serialize)]
pub fn pqc_dsa_sign(message_hex: String, sk_hex: String) -> Result<String, String> {
    let msg = hex::decode(&message_hex).map_err(|e| format!("invalid message hex: {e}"))?;
    dsa::dsa_sign(&msg, &sk_hex)
        .ok_or_else(|| "dsa sign failed".to_string())
        .into()
}

/// ML-DSA-65 verification of `sig_hex` over `message_hex` with the public key.
/// Returns JSON `{"valid": bool}`.
#[frb(sync, serialize)]
pub fn pqc_dsa_verify(
    sig_hex: String,
    message_hex: String,
    vk_hex: String,
) -> Result<String, String> {
    let msg = hex::decode(&message_hex).map_err(|e| format!("invalid message hex: {e}"))?;
    let valid = dsa::dsa_verify_hex(&sig_hex, &msg, &vk_hex);
    Ok(serde_json::json!({ "valid": valid }).to_string()).into()
}

/// Hybrid X25519 + ML-KEM-768 sealed blob: KEM encapsulate + ChaCha20-Poly1305.
/// Returns JSON `{"ct": "<hex>", "nonce": "<hex>", "b64": "<base64>"}`.
#[frb(sync, serialize)]
pub fn pqc_hybrid_seal(payload_hex: String, recipient_pk_hex: String) -> Result<String, String> {
    let payload = hex::decode(&payload_hex).map_err(|e| format!("invalid payload hex: {e}"))?;
    let (ct, nonce, b64) = seal::hybrid_seal(&payload, &recipient_pk_hex, PQC_DOMAIN)?;
    Ok(serde_json::json!({ "ct": ct, "nonce": nonce, "b64": b64 }).to_string()).into()
}

/// Opens a blob produced by `pqc_hybrid_seal`. Returns plaintext as hex.
#[frb(sync, serialize)]
pub fn pqc_hybrid_unseal(
    ct_hex: String,
    nonce_hex: String,
    b64: String,
    sk_hex: String,
) -> Result<String, String> {
    let plaintext = seal::hybrid_unseal(&b64, &nonce_hex, &ct_hex, &sk_hex, PQC_DOMAIN)?;
    Ok(hex::encode(plaintext)).into()
}

/// HKDF-SHA256 (RFC 5869) over hex inputs. Returns `len` derived bytes as hex.
#[frb(sync, serialize)]
pub fn pqc_hkdf_sha256(
    ikm_hex: String,
    salt_hex: String,
    info_hex: String,
    len: u32,
) -> Result<String, String> {
    let ikm = hex::decode(&ikm_hex).map_err(|e| format!("invalid ikm hex: {e}"))?;
    let salt = hex::decode(&salt_hex).map_err(|e| format!("invalid salt hex: {e}"))?;
    let info = hex::decode(&info_hex).map_err(|e| format!("invalid info hex: {e}"))?;
    let okm = hkdf::hkdf_sha256(&ikm, &salt, &info, len as usize)?;
    Ok(hex::encode(okm)).into()
}
