//! Freenet-native hybrid identity: ECDSA P-256 (via ring) + ML-DSA-65 +
//! ML-KEM-768. Mirrors the legacy client's `FreenetIdentityService`: the
//! P-256 keypair signs and the address is `free:<publicKey>`. The public key
//! is ring's fixed encoding (x||y, 64 bytes) hex-encoded.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{
    EcdsaKeyPair, KeyPair, UnparsedPublicKey, ECDSA_P256_SHA256_ASN1,
    ECDSA_P256_SHA256_ASN1_SIGNING,
};

/// Generated Freenet identity payload (hex keys, base64 private key, PQ
/// hybrid keys). Serialized to JSON by callers.
pub struct FreenetIdentity {
    /// Public key as 128 hex chars (ring fixed encoding: 64-byte x||y).
    pub public_key: String,
    /// PKCS#8 private key, base64-encoded.
    pub private_key: String,
    /// Freenet address: `free:<publicKey>`.
    pub address: String,
    /// Post-quantum hybrid keys (ML-DSA-65 signing, ML-KEM-768 encryption).
    pub pqc_dsa_public_key: String,
    pub pqc_dsa_secret_key: String,
    pub pqc_kem_public_key: String,
    pub pqc_kem_secret_key: String,
}

impl FreenetIdentity {
    /// JSON representation used by the Tauri command layer.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "publicKey": self.public_key,
            "privateKey": self.private_key,
            "address": self.address,
            "pqc": {
                "dsaPublicKey": self.pqc_dsa_public_key,
                "dsaSecretKey": self.pqc_dsa_secret_key,
                "kemPublicKey": self.pqc_kem_public_key,
                "kemSecretKey": self.pqc_kem_secret_key,
            }
        })
    }
}

/// Generates a Freenet hybrid keypair. `seed` (first 32 bytes) is used to
/// derive the ML-DSA seed; the P-256 key uses the OS RNG like the legacy
/// Web Crypto flow.
pub fn freenet_keygen(seed: Option<&[u8]>) -> Result<FreenetIdentity, String> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
        .map_err(|e| format!("p256 keygen: {e}"))?;
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
        .map_err(|e| format!("p256 import: {e}"))?;
    let raw = pair.public_key().as_ref();
    // ring returns 65-byte uncompressed (0x04 || x || y): drop the leading
    // octet for the 64-byte fixed encoding.
    let fixed = if raw.len() == 65 && raw[0] == 0x04 {
        &raw[1..]
    } else {
        raw
    };
    let public_key = hex::encode(fixed);
    let private_key = B64.encode(pkcs8.as_ref());
    let address = format!("free:{public_key}");
    let (dsa_pk, dsa_sk) =
        crate::dsa::dsa_keygen(seed).map_err(|e| format!("ml-dsa keygen: {e}"))?;
    let (kem_pk, kem_sk) = crate::kem::kem_keygen().map_err(|e| format!("ml-kem keygen: {e}"))?;
    Ok(FreenetIdentity {
        public_key,
        private_key,
        address,
        pqc_dsa_public_key: dsa_pk,
        pqc_dsa_secret_key: dsa_sk,
        pqc_kem_public_key: kem_pk,
        pqc_kem_secret_key: kem_sk,
    })
}

/// Signs a message with the base64 PKCS#8 P-256 private key. Returns the
/// signature as base64 (DER-encoded, matching Web Crypto ECDSA output).
pub fn freenet_sign(message: &[u8], private_key_b64: &str) -> Result<String, String> {
    let der = B64
        .decode(private_key_b64)
        .map_err(|e| format!("sk decode: {e}"))?;
    let rng = SystemRandom::new();
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &der, &rng)
        .map_err(|e| format!("sk import: {e}"))?;
    let sig = pair.sign(&rng, message).map_err(|e| format!("sign: {e}"))?;
    Ok(B64.encode(sig.as_ref()))
}

/// Verifies a base64 DER ECDSA signature against the hex-encoded public key
/// (ring fixed encoding, 128 hex chars). Returns true when valid.
pub fn freenet_verify(message: &[u8], signature_b64: &str, public_key_hex: &str) -> bool {
    let Ok(sig) = B64.decode(signature_b64) else {
        return false;
    };
    let Ok(pk_bytes) = hex::decode(public_key_hex) else {
        return false;
    };
    let mut full = [0u8; 65];
    let key_bytes = if pk_bytes.len() == 64 {
        full[0] = 0x04;
        full[1..65].copy_from_slice(&pk_bytes);
        &full[..]
    } else {
        pk_bytes.as_slice()
    };
    let key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, key_bytes);
    key.verify(message, &sig).is_ok()
}
