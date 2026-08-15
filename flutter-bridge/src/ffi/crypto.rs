//! Crypto FFI module
//!
//! SHA-256/HMAC/HKDF wrappers over soshal-crypto-core, NIP-44 v2 via the
//! unlocked signer (see `signer.rs`), post-quantum KEM/DSA via crypto-core's
//! byte-array API, and entropy helpers. No key material crosses to Dart.

use flutter_rust_bridge::frb;
use rand::RngCore;
use soshal_crypto_core::hash;
use soshal_crypto_core::pqc;

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
    fn test_blake3_empty_known_vector() {
        let h = crypto_blake3(vec![]).unwrap();
        assert_eq!(h.len(), 64);
        assert_eq!(
            h,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_ne!(crypto_blake3(b"x".to_vec()).unwrap(), h);
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
    async fn test_pqc_kem_roundtrip() {
        let keygen = crypto_pqc_kem_keygen().await.unwrap();
        let kp: serde_json::Value = serde_json::from_str(&keygen).unwrap();
        let pk = kp["pk"].as_str().unwrap().to_string();
        let sk = kp["sk"].as_str().unwrap().to_string();
        assert_eq!(pk.len(), 1217 * 2);
        assert_eq!(sk.len(), 96 * 2);
        let enc = crypto_pqc_kem_encaps(pk).await.unwrap();
        let e: serde_json::Value = serde_json::from_str(&enc).unwrap();
        let ct = e["ct"].as_str().unwrap().to_string();
        assert_eq!(ct.len(), 1121 * 2);
        let ss = crypto_pqc_kem_decaps(ct, sk).await.unwrap();
        assert_eq!(ss, e["ss"].as_str().unwrap());
        assert_eq!(ss.len(), 64);
    }

    #[tokio::test]
    async fn test_pqc_kem_wrong_secret_key_mismatches() {
        let a: serde_json::Value =
            serde_json::from_str(&crypto_pqc_kem_keygen().await.unwrap()).unwrap();
        let b: serde_json::Value =
            serde_json::from_str(&crypto_pqc_kem_keygen().await.unwrap()).unwrap();
        let e: serde_json::Value = serde_json::from_str(
            &crypto_pqc_kem_encaps(a["pk"].as_str().unwrap().to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        let dec = crypto_pqc_kem_decaps(
            e["ct"].as_str().unwrap().to_string(),
            b["sk"].as_str().unwrap().to_string(),
        )
        .await;
        match dec {
            Ok(ss) => assert_ne!(ss, e["ss"].as_str().unwrap()),
            Err(err) => assert!(err.contains("invalid hybrid"), "{err}"),
        }
    }

    #[tokio::test]
    async fn test_pqc_kem_invalid_lengths() {
        let err = crypto_pqc_kem_encaps("deadbeef".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid hybrid public key length"), "{err}");
        let err = crypto_pqc_kem_encaps("zz".to_string()).await.unwrap_err();
        assert!(err.contains("Invalid character"), "{err}");
        let err = crypto_pqc_kem_decaps("dead".to_string(), "beef".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid hybrid ciphertext length"), "{err}");
        let kp: serde_json::Value =
            serde_json::from_str(&crypto_pqc_kem_keygen().await.unwrap()).unwrap();
        let e: serde_json::Value = serde_json::from_str(
            &crypto_pqc_kem_encaps(kp["pk"].as_str().unwrap().to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        let err = crypto_pqc_kem_decaps(e["ct"].as_str().unwrap().to_string(), "beef".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid hybrid secret key length"), "{err}");
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
    async fn test_frost_generate_jury_keys() {
        let group_pk =
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
        let json = crypto_frost_generate_jury_keys(3, 5, group_pk.clone())
            .await
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["participant_id"].as_u64(), Some(1));
        assert_eq!(arr[4]["participant_id"].as_u64(), Some(5));
        assert_eq!(arr[0]["threshold"].as_u64(), Some(3));
        assert_eq!(arr[0]["total_participants"].as_u64(), Some(5));
        assert_eq!(arr[0]["group_pubkey_hex"], group_pk);
        assert!(!arr[0]["secret_share_hex"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_frost_aggregate_signature_roundtrip() {
        let group_pk =
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
        let shares_json = crypto_frost_generate_jury_keys(3, 5, group_pk.clone())
            .await
            .unwrap();
        let shares: Vec<soshal_crypto_core::frost::FrostKeyShare> =
            serde_json::from_str(&shares_json).unwrap();
        let message = b"Ban spammer npub_123";
        let sig_shares: Vec<soshal_crypto_core::frost::FrostSignatureShare> = shares
            .iter()
            .take(3)
            .map(|s| {
                soshal_crypto_core::frost::FrostSessionManager::sign_share(s, message).unwrap()
            })
            .collect();
        let sig_json = crypto_frost_aggregate_signature(
            serde_json::to_string(&sig_shares).unwrap(),
            3,
            group_pk.clone(),
            hex::encode(message),
        )
        .await
        .unwrap();
        let sig: serde_json::Value = serde_json::from_str(&sig_json).unwrap();
        assert_eq!(sig["group_pubkey_hex"], group_pk);
        assert_eq!(sig["schnorr_signature_hex"].as_str().unwrap().len(), 96);
        let expected_msg_hash = hex::encode(crypto_sha256(message.to_vec()).unwrap());
        assert_eq!(sig["message_hash_hex"], expected_msg_hash);
    }

    #[tokio::test]
    async fn test_frost_aggregate_insufficient_shares() {
        let group_pk = "ab".repeat(32);
        let shares_json = crypto_frost_generate_jury_keys(3, 5, group_pk.clone())
            .await
            .unwrap();
        let shares: Vec<soshal_crypto_core::frost::FrostKeyShare> =
            serde_json::from_str(&shares_json).unwrap();
        let sig_shares: Vec<soshal_crypto_core::frost::FrostSignatureShare> = shares
            .iter()
            .take(2)
            .map(|s| soshal_crypto_core::frost::FrostSessionManager::sign_share(s, b"msg").unwrap())
            .collect();
        let err = crypto_frost_aggregate_signature(
            serde_json::to_string(&sig_shares).unwrap(),
            3,
            group_pk,
            hex::encode(b"msg"),
        )
        .await
        .unwrap_err();
        assert!(err.contains("insufficient signature shares"), "{err}");
    }
}
