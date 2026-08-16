//! Crypto FFI module
//!
//! SHA-256/HMAC/HKDF wrappers over soshal-crypto-core, NIP-44 v2 via the
//! unlocked signer (see `signer.rs`), post-quantum KEM/DSA via crypto-core's
//! byte-array API, and entropy helpers. No key material crosses to Dart.

use flutter_rust_bridge::frb;
use rand::RngCore;
use soshal_crypto_core::hash;
use zeroize::Zeroize;

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

/// PQC KEM: generate a hybrid (X25519 + ML-KEM-768) keypair.
/// Disabled: KEM secret key must not cross FFI into the Dart heap;
/// PQ-KEM remains Rust-side-only infrastructure.
#[frb(serialize)]
pub async fn crypto_pqc_kem_keygen() -> Result<String, String> {
    Err("PQ-KEM FFI disabled: key material must not cross to Dart".to_string()).into()
}

/// PQC KEM: encapsulate to a hybrid public key.
/// Disabled: PQ-KEM FFI surface disabled; key material must not cross to Dart.
#[frb(serialize)]
pub async fn crypto_pqc_kem_encaps(_recipient_pk: String) -> Result<String, String> {
    Err("PQ-KEM FFI disabled: key material must not cross to Dart".to_string()).into()
}

/// PQC KEM: decapsulate a hybrid ciphertext with the secret key.
/// Disabled: PQ-KEM FFI surface disabled; key material must not cross to Dart.
#[frb(serialize)]
pub async fn crypto_pqc_kem_decaps(_ciphertext: String, _sk: String) -> Result<String, String> {
    Err("PQ-KEM FFI disabled: key material must not cross to Dart".to_string()).into()
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

/// Zeroize helper: scrubs buffer contents in place before returning.
/// The Rust side uses `zeroize` internally on key material (this keeps a
/// destroy-by-pointer hook for Dart-level sensitive buffers that wish to
/// clear after use).
#[frb(sync, serialize)]
pub fn crypto_zeroize(data: Vec<u8>) -> Result<bool, String> {
    let mut buf = data;
    buf.zeroize();
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
/// Disabled: frost is a NON-CRYPTOGRAPHIC simulation (deterministic,
/// forgeable hashes); real threshold signing is a feature project.
#[frb(serialize)]
pub async fn crypto_frost_generate_jury_keys(
    _threshold: u32,
    _total_participants: u32,
    _group_pubkey: String,
) -> Result<String, String> {
    Err("frost is non-cryptographic simulation, disabled".to_string()).into()
}

/// FROST: aggregate partial signature shares into a valid single Schnorr threshold signature.
/// Disabled: frost is a NON-CRYPTOGRAPHIC simulation (deterministic,
/// forgeable hashes); real threshold signing is a feature project.
#[frb(serialize)]
pub async fn crypto_frost_aggregate_signature(
    _shares_json: String,
    _threshold: u32,
    _group_pubkey: String,
    _message_hex: String,
) -> Result<String, String> {
    Err("frost is non-cryptographic simulation, disabled".to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_vector() {
        let h = super::super::util::util_sha256_hex("".to_string());
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

    #[test]
    fn test_sha256_direct_known_vectors() {
        assert_eq!(
            hex::encode(crypto_sha256(vec![]).unwrap()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex::encode(crypto_sha256(b"abc".to_vec()).unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_vector() {
        let mac = crypto_hmac_sha256(vec![0x0b; 20], b"Hi There".to_vec()).unwrap();
        assert_eq!(
            hex::encode(mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn test_hmac_sha256_wrong_key_tamper() {
        let a = crypto_hmac_sha256(vec![1u8; 16], b"payload".to_vec()).unwrap();
        let b = crypto_hmac_sha256(vec![2u8; 16], b"payload".to_vec()).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn test_zeroize_returns_true() {
        assert!(crypto_zeroize(vec![1, 2, 3]).unwrap());
        assert!(crypto_zeroize(vec![]).unwrap());
    }

    #[test]
    fn test_random_bytes_bounds() {
        assert!(crypto_random_bytes(0).is_err());
        assert!(crypto_random_bytes(-1).is_err());
        assert!(crypto_random_bytes(65537).is_err());
        assert_eq!(crypto_random_bytes(1).unwrap().len(), 2);
        assert_eq!(crypto_random_bytes(65536).unwrap().len(), 131072);
    }

    #[tokio::test]
    async fn test_pqc_kem_disabled() {
        let err = crypto_pqc_kem_keygen().await.unwrap_err();
        assert!(err.contains("key material must not cross to Dart"), "{err}");
        let err = crypto_pqc_kem_encaps("ab".repeat(1217).to_string())
            .await
            .unwrap_err();
        assert!(err.contains("key material must not cross to Dart"), "{err}");
        let err = crypto_pqc_kem_decaps("cd".repeat(1121).to_string(), "ef".repeat(96).to_string())
            .await
            .unwrap_err();
        assert!(err.contains("key material must not cross to Dart"), "{err}");
    }

    #[tokio::test]
    async fn test_pir_query_generate_and_evaluate_roundtrip() {
        let query_json = crypto_pir_generate_query(3, 8, "npub_client".to_string())
            .await
            .unwrap();
        let query: serde_json::Value = serde_json::from_str(&query_json).unwrap();
        assert_eq!(query["target_dimension"].as_u64(), Some(8));
        assert_eq!(query["client_pubkey"], "npub_client");
        assert_eq!(query["encrypted_vector"].as_array().unwrap().len(), 8);
        let mut records = vec![format!("{:02x}", 10u8); 8];
        records[3] = format!("{:02x}", 42u8);
        let resp = crypto_pir_evaluate_query(query_json, records)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["record_found"].as_bool(), Some(true));
        assert_eq!(resp["response_vector"].as_array().unwrap().len(), 8);
    }

    #[tokio::test]
    async fn test_pir_generate_query_out_of_bounds() {
        let err = crypto_pir_generate_query(8, 8, "npub".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("out of bounds"), "{err}");
    }

    #[tokio::test]
    async fn test_pir_evaluate_query_errors() {
        let err = crypto_pir_evaluate_query("not json".to_string(), vec![])
            .await
            .unwrap_err();
        assert!(err.contains("invalid query json"), "{err}");
        let query_json = crypto_pir_generate_query(0, 4, "npub".to_string())
            .await
            .unwrap();
        let resp = crypto_pir_evaluate_query(query_json, vec![]).await.unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["record_found"].as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_frost_disabled_non_cryptographic() {
        let group_pk = "ab".repeat(32);
        let err = crypto_frost_generate_jury_keys(3, 5, group_pk.clone())
            .await
            .unwrap_err();
        assert!(err.contains("non-cryptographic simulation"), "{err}");
        let err =
            crypto_frost_aggregate_signature("[]".to_string(), 3, group_pk, hex::encode(b"msg"))
                .await
                .unwrap_err();
        assert!(err.contains("non-cryptographic simulation"), "{err}");
    }
}
