use aead::{Aead, KeyInit};
use base64::Engine;
use chacha20poly1305::ChaCha20Poly1305;
use zeroize::{Zeroize, Zeroizing};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const MAX_SEAL_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_UNSEAL_CIPHERTEXT_BYTES: usize = 24 * 1024 * 1024;
const MAX_DOMAIN_LEN: usize = 256;

pub fn kem_seal(
    payload: &[u8],
    peer_pk_hex: &str,
    domain: &[u8],
) -> Result<(String, String, String), String> {
    if payload.len() > MAX_SEAL_PAYLOAD_BYTES {
        return Err("payload exceeds max size".to_string());
    }
    if domain.len() > MAX_DOMAIN_LEN {
        return Err("domain exceeds max length".to_string());
    }
    let (ct_hex, ss_hex) = crate::kem::kem_encapsulate(peer_pk_hex.trim())?;
    let ss_hex = Zeroizing::new(ss_hex);
    let ss = Zeroizing::new(hex::decode(ss_hex.as_str()).map_err(|_| "bad ss hex".to_string())?);
    let derived = hkdf_derive(&ss, domain)?;
    let (encrypted, nonce) = aead_encrypt(payload, &derived.enc_key, &derived.nonce)?;
    Ok((ct_hex, nonce, encrypted))
}

pub fn kem_unseal(
    ciphertext_b64: &str,
    nonce_hex: &str,
    ct_hex: &str,
    sk_hex: &str,
    domain: &[u8],
) -> Result<Vec<u8>, String> {
    if ciphertext_b64.len() > MAX_UNSEAL_CIPHERTEXT_BYTES {
        return Err("ciphertext exceeds max size".to_string());
    }
    if domain.len() > MAX_DOMAIN_LEN {
        return Err("domain exceeds max length".to_string());
    }
    let ss_hex = crate::kem::kem_decapsulate(ct_hex.trim(), sk_hex.trim())?;
    let ss_hex = Zeroizing::new(ss_hex);
    let ss = Zeroizing::new(hex::decode(ss_hex.as_str()).map_err(|_| "bad ss hex".to_string())?);
    let derived = hkdf_derive(&ss, domain)?;
    let nonce_bytes = hex::decode(nonce_hex.trim()).map_err(|_| "bad nonce hex".to_string())?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err("bad nonce len".to_string());
    }
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(&nonce_bytes);
    if nonce_arr != derived.nonce {
        return Err("nonce mismatch with derived KEM session".to_string());
    }
    aead_decrypt(ciphertext_b64, &derived.enc_key, &derived.nonce)
}

pub fn hybrid_seal(
    payload: &[u8],
    peer_pk_hex: &str,
    domain: &[u8],
) -> Result<(String, String, String), String> {
    if payload.len() > MAX_SEAL_PAYLOAD_BYTES {
        return Err("payload exceeds max size".to_string());
    }
    if domain.len() > MAX_DOMAIN_LEN {
        return Err("domain exceeds max length".to_string());
    }
    let pk_bytes = hex::decode(peer_pk_hex.trim()).map_err(|_| "bad pk hex".to_string())?;
    if pk_bytes.len() != crate::hybrid::HYBRID_PK_LEN {
        return Err("bad pk len".to_string());
    }
    let mut pk_arr = [0u8; crate::hybrid::HYBRID_PK_LEN];
    pk_arr.copy_from_slice(&pk_bytes);
    let (ct, ss) = crate::hybrid::hybrid_encapsulate_bytes(&pk_arr, domain)?;
    let ss = Zeroizing::new(ss);
    let derived = hkdf_derive(ss.as_slice(), domain)?;
    let (encrypted, nonce) = aead_encrypt(payload, &derived.enc_key, &derived.nonce)?;
    Ok((hex::encode(ct), nonce, encrypted))
}

/// Opens a blob produced by [`hybrid_seal`].
pub fn hybrid_unseal(
    ciphertext_b64: &str,
    nonce_hex: &str,
    ct_hex: &str,
    sk_hex: &str,
    domain: &[u8],
) -> Result<Vec<u8>, String> {
    if ciphertext_b64.len() > MAX_UNSEAL_CIPHERTEXT_BYTES {
        return Err("ciphertext exceeds max size".to_string());
    }
    if domain.len() > MAX_DOMAIN_LEN {
        return Err("domain exceeds max length".to_string());
    }
    let ct_bytes = hex::decode(ct_hex.trim()).map_err(|_| "bad ct hex".to_string())?;
    let sk_bytes =
        zeroize::Zeroizing::new(hex::decode(sk_hex.trim()).map_err(|_| "bad sk hex".to_string())?);
    if ct_bytes.len() != crate::hybrid::HYBRID_CT_LEN
        || sk_bytes.len() != crate::hybrid::HYBRID_SK_LEN
    {
        return Err("bad input length".to_string());
    }
    let mut ct_arr = [0u8; crate::hybrid::HYBRID_CT_LEN];
    let mut sk_arr = [0u8; crate::hybrid::HYBRID_SK_LEN];
    ct_arr.copy_from_slice(&ct_bytes);
    sk_arr.copy_from_slice(&sk_bytes);
    let res = crate::hybrid::hybrid_decapsulate_bytes(&ct_arr, &sk_arr, domain);
    sk_arr.zeroize();
    let ss = res?;
    let ss = Zeroizing::new(ss);
    let derived = hkdf_derive(ss.as_slice(), domain)?;
    let nonce_bytes = hex::decode(nonce_hex.trim()).map_err(|_| "bad nonce hex".to_string())?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err("bad nonce len".to_string());
    }
    let mut nonce_arr = [0u8; NONCE_LEN];
    nonce_arr.copy_from_slice(&nonce_bytes);
    if nonce_arr != derived.nonce {
        return Err("nonce mismatch with derived KEM session".to_string());
    }
    aead_decrypt(ciphertext_b64, &derived.enc_key, &derived.nonce)
}

struct DerivedKey {
    enc_key: [u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
}

impl Zeroize for DerivedKey {
    fn zeroize(&mut self) {
        self.enc_key.zeroize();
        self.nonce.zeroize();
    }
}

impl Drop for DerivedKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

fn hkdf_derive(shared_secret: &[u8], domain: &[u8]) -> Result<DerivedKey, String> {
    let mut okm = vec![0u8; KEY_LEN + NONCE_LEN];
    let salt = b"soshal-kem-seal-v1";
    let derived = crate::hkdf::hkdf_sha256(shared_secret, salt, domain, KEY_LEN + NONCE_LEN)?;
    okm.copy_from_slice(&derived);
    let mut enc_key = [0u8; KEY_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    enc_key.copy_from_slice(&okm[..KEY_LEN]);
    nonce.copy_from_slice(&okm[KEY_LEN..]);
    okm.zeroize();
    Ok(DerivedKey { enc_key, nonce })
}

fn aead_encrypt(
    plaintext: &[u8],
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
) -> Result<(String, String), String> {
    use aead::generic_array::GenericArray;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(key));
    let nonce_arr = chacha20poly1305::Nonce::from_slice(nonce);
    let ct = cipher
        .encrypt(nonce_arr, plaintext)
        .map_err(|_| "encrypt failed".to_string())?;
    let ct_b64 = base64::engine::general_purpose::STANDARD.encode(&ct);
    let nonce_hex = hex::encode(nonce);
    Ok((ct_b64, nonce_hex))
}

const MAX_CIPHERTEXT_B64_LEN: usize = 64 * 1024 * 1024;

fn aead_decrypt(
    ciphertext_b64: &str,
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, String> {
    if ciphertext_b64.len() > MAX_CIPHERTEXT_B64_LEN {
        return Err("ciphertext oversized".to_string());
    }
    use aead::generic_array::GenericArray;
    let mut ct = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64)
        .map_err(|_| "bad base64".to_string())?;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(key));
    let nonce_arr = chacha20poly1305::Nonce::from_slice(nonce);
    let res = cipher
        .decrypt(nonce_arr, ct.as_ref())
        .map_err(|_| "decrypt failed".to_string());
    ct.zeroize();
    res
}
