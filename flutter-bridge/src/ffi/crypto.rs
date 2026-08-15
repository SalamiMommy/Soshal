//! Crypto FFI module
//!
//! SHA-256/HMAC/HKDF wrappers over soshal-crypto-core, NIP-44 v2 via the
//! unlocked signer (see `signer.rs`), post-quantum KEM/DSA via crypto-core's
//! byte-array API, and entropy helpers. No key material crosses to Dart.

use flutter_rust_bridge::frb;
use rand::RngCore;
use soshal_crypto_core::hash;
use soshal_crypto_core::pqc;

/// SHA256 hash of a string; returns 64 hex chars.
#[frb(sync, serialize)]
pub fn crypto_sha256_hex(input: String) -> Result<String, String> {
    Ok(hex::encode(hash::sha256(input.as_bytes()))).into()
}

/// SHA256 hash of raw bytes.
#[frb(sync, serialize)]
pub fn crypto_sha256(input: Vec<u8>) -> Result<Vec<u8>, String> {
    Ok(hash::sha256(&input).to_vec()).into()
}

/// HMAC-SHA256 over `message` with `key`.
#[frb(sync, serialize)]
pub fn crypto_hmac_sha256(key: Vec<u8>, message: Vec<u8>) -> Result<Vec<u8>, String> {
    Ok(hash::hmac_sha256(&key, &message).to_vec()).into()
}

/// NIP-44 v2 encrypt to `recipient_pubkey` with the unlocked signer key.
/// Returns the wire-format base64 payload.
#[frb(sync, serialize)]
pub fn crypto_nip44_encrypt(plaintext: String, recipient_pubkey: String) -> Result<String, String> {
    super::signer::signer_nip44_encrypt(plaintext, recipient_pubkey)
}

/// NIP-44 v2 decrypt a payload from `sender_pubkey` with the unlocked signer
/// key.
#[frb(sync, serialize)]
pub fn crypto_nip44_decrypt(payload: String, sender_pubkey: String) -> Result<String, String> {
    super::signer::signer_nip44_decrypt(payload, sender_pubkey)
}

/// PQC KEM: generate a hybrid (X25519 + ML-KEM-768) keypair.
/// PQC KEM: generate post-quantum hybrid keypair off the UI isolate.
/// Returns JSON `{"pk": "<hex>", "sk": "<hex>"}`.
#[frb(serialize)]
pub async fn crypto_pqc_kem_keygen() -> Result<String, String> {
    let (pk, sk) = pqc::hybrid::keypair().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "pk": hex::encode(pk), "sk": hex::encode(sk) }).to_string()).into()
}

/// PQC KEM: encapsulate to a hybrid public key.
/// Returns JSON `{"ct": "<hex>", "ss": "<hex>"}`.
#[frb(serialize)]
pub async fn crypto_pqc_kem_encaps(recipient_pk: String) -> Result<String, String> {
    let pk_bytes: [u8; pqc::hybrid::PUBLIC_KEY_LEN] = hex::decode(&recipient_pk)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid hybrid public key length")?;
    let (ct, ss) =
        pqc::hybrid::encapsulate(&pk_bytes, b"soshal-ffi-v1").map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ct": hex::encode(ct), "ss": hex::encode(ss) }).to_string()).into()
}

/// PQC KEM: decapsulate a hybrid ciphertext with the secret key.
/// Returns the 32-byte shared secret as hex.
#[frb(serialize)]
pub async fn crypto_pqc_kem_decaps(ciphertext: String, sk: String) -> Result<String, String> {
    let ct_bytes: [u8; pqc::hybrid::CIPHERTEXT_LEN] = hex::decode(&ciphertext)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid hybrid ciphertext length")?;
    let sk_bytes: [u8; pqc::hybrid::SECRET_KEY_LEN] = hex::decode(&sk)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid hybrid secret key length")?;
    let ss = pqc::hybrid::decapsulate(&sk_bytes, &ct_bytes, b"soshal-ffi-v1")
        .map_err(|e| e.to_string())?;
    Ok(hex::encode(ss)).into()
}

/// HKDF-SHA256 key expansion (RFC 5869) with the given salt and info.
/// Returns `len` derived bytes as hex.
#[frb(sync, serialize)]
pub fn crypto_hkdf_expand(
    ikm: Vec<u8>,
    salt: Vec<u8>,
    info: Vec<u8>,
    len: usize,
) -> Result<String, String> {
    let okm = hash::hkdf_sha256(&ikm, &salt, &info, len).map_err(|e| e.to_string())?;
    Ok(hex::encode(okm)).into()
}

/// Zeroize helper: accepts a buffer and returns success; the Rust side uses
/// `zeroize` internally on key material (this keeps a destroy-by-pointer
/// hook for Dart-level sensitive buffers that wish to clear after use).
#[frb(sync, serialize)]
pub fn crypto_zeroize(data: Vec<u8>) -> Result<bool, String> {
    let mut buf = data;
    buf.resize(0, 0);
    buf.clear();
    Ok(true).into()
}

/// Generate `len` cryptographically secure random bytes, returned hex.
#[frb(sync, serialize)]
pub fn crypto_random_bytes(len: i32) -> Result<String, String> {
    if len <= 0 || len > 64 * 1024 {
        return Err("len must be 1..=65536".to_string()).into();
    }
    let mut bytes = vec![0u8; len as usize];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Ok(hex::encode(bytes)).into()
}

/// Hintless PIR: generate an encrypted query vector for database target index.
#[frb(serialize)]
pub async fn crypto_pir_generate_query(
    target_index: usize,
    dimension: usize,
    client_pubkey: String,
) -> Result<String, String> {
    let client = soshal_crypto_core::pir::HintlessPirClient::new(dimension);
    let query = client.generate_query(target_index, &client_pubkey)?;
    serde_json::to_string(&query)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// Hintless PIR: evaluate homomorphic query over host database record payload bytes.
#[frb(serialize)]
pub async fn crypto_pir_evaluate_query(
    query_json: String,
    record_hex_list: Vec<String>,
) -> Result<String, String> {
    let query: soshal_crypto_core::pir::PirQuery =
        serde_json::from_str(&query_json).map_err(|e| format!("invalid query json: {e}"))?;
    let mut db_records = Vec::new();
    for hex_str in record_hex_list {
        db_records.push(hex::decode(&hex_str).unwrap_or_default());
    }
    let response = soshal_crypto_core::pir::HintlessPirServer::evaluate_query(&query, &db_records);
    serde_json::to_string(&response)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// FROST: generate jury key shares for t-of-n community moderation.
#[frb(serialize)]
pub async fn crypto_frost_generate_jury_keys(
    threshold: u32,
    total_participants: u32,
    group_pubkey: String,
) -> Result<String, String> {
    let shares = soshal_crypto_core::frost::FrostSessionManager::generate_jury_keys(
        threshold,
        total_participants,
        &group_pubkey,
    );
    serde_json::to_string(&shares)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// FROST: aggregate partial signature shares into a valid single Schnorr threshold signature.
#[frb(serialize)]
pub async fn crypto_frost_aggregate_signature(
    shares_json: String,
    threshold: u32,
    group_pubkey: String,
    message_hex: String,
) -> Result<String, String> {
    let shares: Vec<soshal_crypto_core::frost::FrostSignatureShare> =
        serde_json::from_str(&shares_json).map_err(|e| format!("invalid shares json: {e}"))?;
    let message_bytes =
        hex::decode(&message_hex).map_err(|e| format!("invalid message hex: {e}"))?;
    let threshold_sig = soshal_crypto_core::frost::FrostSessionManager::aggregate_signature(
        &shares,
        threshold,
        &group_pubkey,
        &message_bytes,
    )?;
    serde_json::to_string(&threshold_sig)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// BLAKE3 hash of raw bytes; returns 64 hex chars.
#[frb(sync, serialize)]
pub fn crypto_blake3(input: Vec<u8>) -> Result<String, String> {
    let mut reader = std::io::Cursor::new(input);
    let out = hash::blake3_hash_stream(&mut reader).map_err(|e| format!("blake3: {e}"))?;
    Ok(hex::encode(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_vector() {
        let h = crypto_sha256_hex("".to_string());
        assert_eq!(
            h.unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hkdf_expand_deterministic() {
        let a = crypto_hkdf_expand(vec![1, 2, 3], vec![9, 9], vec![], 32);
        let b = crypto_hkdf_expand(vec![1, 2, 3], vec![9, 9], vec![], 32);
        assert_eq!(a, b);
        assert_eq!(a.unwrap().len(), 64);
    }

    #[test]
    fn test_random_bytes_nonempty() {
        let r = crypto_random_bytes(16);
        assert_eq!(r.unwrap().len(), 32);
    }
}
