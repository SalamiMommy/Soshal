//! Spec-compliant NIP-44 (v2) authenticated encryption.
//!
//! Wire format matches the NIP-44 spec exactly — `version ‖ nonce ‖
//! ciphertext ‖ hmac` with HKDF-SHA256 derived message keys, ChaCha20 stream
//! cipher and an HMAC-SHA256 authenticator — so ciphertext produced here is
//! interoperable with any spec-compliant client (`nostr::nips::nip44`).
//!
//! A legacy decode path remains for ciphertexts written by pre-v2 versions
//! of this crate (salt-prefixed, ChaCha20-Poly1305 with AAD): stored group
//! messages, offline-sync blobs and media envelopes keep decrypting after
//! upgrade. All new encryption is spec v2.

use aead::{Aead, KeyInit, Payload};
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::{ChaCha20Poly1305, Key as P1305Key, Nonce as P1305Nonce};
use zeroize::Zeroize;

use crate::hash;

const NIP44_INFO: &[u8] = b"nip44-v2";
const DERIVED_LEN: usize = 76;
/// Nonce/salt length for NIP-44 v2 (32 bytes).
pub const SALT_LEN: usize = 32;
/// Symmetric key length for NIP-44 encryption (32 bytes).
pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
/// Format version byte: 1 = legacy unpadded, 2 = padded (NIP-44 spec v2).
const VERSION_LEN: usize = 1;
pub const VERSION_LEGACY: u8 = 1;
pub const VERSION_PADDED: u8 = 2;

/// NIP-44 padding: 2-byte big-endian plaintext length, then plaintext, then
/// zero padding to `calc_padding` total size (32-byte chunks, min 32, scaling
/// to power-of-two chunks for large plaintexts).
pub fn calc_padding(len: usize) -> usize {
    if len <= 32 {
        return 32;
    }
    let next_power: usize = 1 << (log2_round_down(len - 1) + 1);
    let chunk: usize = if next_power <= 256 {
        32
    } else {
        next_power / 8
    };
    chunk * (((len - 1) / chunk) + 1)
}

fn log2_round_down(n: usize) -> usize {
    usize::BITS as usize - 1 - n.leading_zeros() as usize
}

pub fn pad(plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    if plaintext.is_empty() {
        return Err("empty plaintext");
    }
    // The length prefix is a u16: anything larger would silently truncate,
    // corrupting the padding and breaking unpad() on the other side.
    if plaintext.len() > u16::MAX as usize {
        return Err("plaintext too large");
    }
    let take = calc_padding(plaintext.len()) - plaintext.len();
    let mut out = Vec::with_capacity(2 + plaintext.len() + take);
    out.extend_from_slice(&(plaintext.len() as u16).to_be_bytes());
    out.extend_from_slice(plaintext);
    out.resize(out.len() + take, 0);
    Ok(out)
}

/// Removes NIP-44 padding; rejects malformed lengths and wrong total size.
pub fn unpad(padded: &[u8]) -> Result<Vec<u8>, &'static str> {
    if padded.len() < 2 + 32 {
        return Err("padded payload too short");
    }
    let unpadded_len = u16::from_be_bytes([padded[0], padded[1]]) as usize;
    if unpadded_len == 0 {
        return Err("empty plaintext");
    }
    if padded.len() != 2 + calc_padding(unpadded_len) {
        return Err("invalid padding");
    }
    Ok(padded[2..2 + unpadded_len].to_vec())
}

/// Removes NIP-44 padding in-place on a mutable byte vector without extra heap allocations.
pub fn unpad_in_place(mut padded: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if padded.len() < 2 + 32 {
        return Err("padded payload too short");
    }
    let unpadded_len = u16::from_be_bytes([padded[0], padded[1]]) as usize;
    if unpadded_len == 0 {
        return Err("empty plaintext");
    }
    if padded.len() != 2 + calc_padding(unpadded_len) {
        return Err("invalid padding");
    }
    padded.drain(0..2);
    padded.truncate(unpadded_len);
    Ok(padded)
}

/// NIP-44 v2 spec derivation. The conversation key is the HKDF extract step
/// `ck = HMAC-SHA256("nip44-v2", key)`; message keys are the expand step
/// `T(i) = HMAC-SHA256(ck, T(i-1) ‖ nonce ‖ i)`, truncated to 76 bytes
/// (T1 ‖ T2 ‖ T3, mapped to enc-key 32 ‖ chacha nonce 12 ‖ auth-key 32).
fn spec_derive_keys(
    ck: &[u8; KEY_LEN],
    nonce: &[u8; SALT_LEN],
) -> ([u8; KEY_LEN], [u8; NONCE_LEN], [u8; KEY_LEN]) {
    let mut t1_input = [0u8; SALT_LEN + 1];
    t1_input[..SALT_LEN].copy_from_slice(nonce);
    t1_input[SALT_LEN] = 1;
    let mut t1 = hash::hmac_sha256(ck, &t1_input);
    t1_input.zeroize();

    let mut t2_input = [0u8; KEY_LEN + SALT_LEN + 1];
    t2_input[..KEY_LEN].copy_from_slice(&t1);
    t2_input[KEY_LEN..KEY_LEN + SALT_LEN].copy_from_slice(nonce);
    t2_input[KEY_LEN + SALT_LEN] = 2;
    let mut t2 = hash::hmac_sha256(ck, &t2_input);
    t2_input.zeroize();

    let mut t3_input = [0u8; KEY_LEN + SALT_LEN + 1];
    t3_input[..KEY_LEN].copy_from_slice(&t2);
    t3_input[KEY_LEN..KEY_LEN + SALT_LEN].copy_from_slice(nonce);
    t3_input[KEY_LEN + SALT_LEN] = 3;
    let t3 = hash::hmac_sha256(ck, &t3_input);
    t3_input.zeroize();

    let mut enc_key = [0u8; KEY_LEN];
    enc_key.copy_from_slice(&t1);
    let mut chacha_nonce = [0u8; NONCE_LEN];
    chacha_nonce.copy_from_slice(&t2[..NONCE_LEN]);
    let mut auth_key = [0u8; KEY_LEN];
    auth_key[..KEY_LEN - NONCE_LEN].copy_from_slice(&t2[NONCE_LEN..]);
    auth_key[KEY_LEN - NONCE_LEN..].copy_from_slice(&t3[..NONCE_LEN]);
    t1.zeroize();
    t2.zeroize();
    (enc_key, chacha_nonce, auth_key)
}

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

struct CkCache {
    map: HashMap<[u8; KEY_LEN], [u8; KEY_LEN]>,
    queue: VecDeque<[u8; KEY_LEN]>,
}

static CK_CACHE: Mutex<Option<CkCache>> = Mutex::new(None);

/// Derives and caches the NIP-44 v2 conversation key `ck = HMAC-SHA256("nip44-v2", key)`.
pub fn derive_conversation_key(key: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    let mut guard = CK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| CkCache {
        map: HashMap::with_capacity(64),
        queue: VecDeque::with_capacity(64),
    });
    if let Some(&ck) = cache.map.get(key) {
        return ck;
    }
    let ck = hash::hmac_sha256(NIP44_INFO, key);
    if cache.map.len() >= 128 {
        if let Some(oldest) = cache.queue.pop_front() {
            cache.map.remove(&oldest);
        }
    }
    cache.map.insert(*key, ck);
    cache.queue.push_back(*key);
    ck
}

/// Clear and zeroize all cached NIP-44 v2 conversation keys.
pub fn clear_conversation_key_cache() {
    let mut guard = CK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut cache) = guard.take() {
        for (mut k, mut v) in cache.map.drain() {
            k.zeroize();
            v.zeroize();
        }
    }
}

/// NIP-44 v2 spec encryption: `2 ‖ nonce ‖ ciphertext ‖ hmac`, base64-encoded.
///
/// Message keys derive per the spec (`extract("nip44-v2", key)` then
/// `expand(ck, nonce, 76)`); the authenticator is HMAC-SHA256(auth-key,
/// nonce ‖ ciphertext), verified constant-time on decrypt.
pub fn encrypt(plaintext: &[u8], key: &[u8; KEY_LEN]) -> Result<String, &'static str> {
    if plaintext.is_empty() {
        return Err("empty plaintext");
    }
    if plaintext.len() > u16::MAX as usize {
        return Err("plaintext too large");
    }
    let mut nonce = [0u8; SALT_LEN];
    getrandom::fill(&mut nonce).map_err(|_| "rng failed")?;
    let ck = derive_conversation_key(key);
    let (mut enc_key_bytes, mut nonce12, mut auth_key_bytes) = spec_derive_keys(&ck, &nonce);

    let padded_len = calc_padding(plaintext.len());
    let mut payload = Vec::with_capacity(VERSION_LEN + SALT_LEN + 2 + padded_len + 32);
    payload.push(VERSION_PADDED);
    payload.extend_from_slice(&nonce);
    let cipher_start = payload.len();
    payload.extend_from_slice(&(plaintext.len() as u16).to_be_bytes());
    payload.extend_from_slice(plaintext);
    let take = padded_len - plaintext.len();
    payload.resize(payload.len() + take, 0);

    // ChaCha20 stream cipher in-place over the padded plaintext slice.
    use chacha20::cipher::{KeyIvInit, StreamCipher as _};
    let mut cipher = chacha20::ChaCha20::new(
        chacha20::Key::from_slice(&enc_key_bytes),
        chacha20::Nonce::from_slice(&nonce12),
    );
    cipher.apply_keystream(&mut payload[cipher_start..]);
    enc_key_bytes.zeroize();
    nonce12.zeroize();

    // HMAC-SHA256 over nonce ‖ ciphertext (constant-time on verify).
    let mac = hash::hmac_sha256_slices(&auth_key_bytes, &[&nonce, &payload[cipher_start..]]);
    auth_key_bytes.zeroize();

    payload.extend_from_slice(&mac);
    Ok(general_purpose::STANDARD.encode(&payload))
}

/// NIP-44 v2 spec decryption. Falls back to the legacy (pre-v2) format for
/// ciphertexts written by older versions of this crate.
pub fn decrypt(payload: &str, key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    let decoded = general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| "invalid base64")?;
    if decoded.is_empty() {
        return Err("empty payload");
    }
    if decoded[0] == VERSION_PADDED {
        if let Ok(pt) = decrypt_spec(&decoded, key) {
            return Ok(pt);
        }
    }
    if let Ok(pt) = decrypt_legacy(&decoded, key) {
        return Ok(pt);
    }
    Err("decrypt failed")
}

fn decrypt_spec(decoded: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    if decoded[0] != VERSION_PADDED {
        return Err("unsupported payload version");
    }
    if decoded.len() < VERSION_LEN + SALT_LEN + 2 + 32 + 32 {
        return Err("payload too short");
    }
    let nonce = &decoded[VERSION_LEN..VERSION_LEN + SALT_LEN];
    let buffer = &decoded[VERSION_LEN + SALT_LEN..decoded.len() - 32];
    let mac = &decoded[decoded.len() - 32..];

    let ck = derive_conversation_key(key);
    let nonce_arr: [u8; SALT_LEN] = nonce.try_into().map_err(|_| "bad nonce")?;
    let (mut enc_key_bytes, mut nonce12, mut auth_key_bytes) = spec_derive_keys(&ck, &nonce_arr);

    // Constant-time authenticator check before any keystream work.
    let expected = hash::hmac_sha256_slices(&auth_key_bytes, &[nonce, buffer]);
    auth_key_bytes.zeroize();
    if expected.len() != mac.len() || !constant_time_eq(&expected, mac) {
        enc_key_bytes.zeroize();
        nonce12.zeroize();
        return Err("decrypt failed");
    }

    let mut plaintext = buffer.to_vec();
    use chacha20::cipher::{KeyIvInit, StreamCipher as _};
    let mut cipher = chacha20::ChaCha20::new(
        chacha20::Key::from_slice(&enc_key_bytes),
        chacha20::Nonce::from_slice(&nonce12),
    );
    cipher.apply_keystream(&mut plaintext);
    enc_key_bytes.zeroize();
    nonce12.zeroize();

    unpad_in_place(plaintext)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Legacy encryption format (pre-spec): random salt, then
/// `salt ‖ version ‖ ciphertext` where the AEAD is ChaCha20-Poly1305 with
/// the auth key passed as AAD. Kept only for decrypting existing data.
pub fn encrypt_padded(plaintext: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| "rng failed")?;

    let mut derived =
        hash::hkdf_sha256(key, &salt, NIP44_INFO, DERIVED_LEN).map_err(|_| "hkdf failed")?;

    let (enc_key_slice, rest) = derived.split_at(KEY_LEN);
    let (auth_key_slice, nonce_slice) = rest.split_at(KEY_LEN);

    let mut enc_key_bytes = [0u8; KEY_LEN];
    let mut auth_key_bytes = [0u8; KEY_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    enc_key_bytes.copy_from_slice(enc_key_slice);
    auth_key_bytes.copy_from_slice(auth_key_slice);
    nonce_bytes.copy_from_slice(nonce_slice);

    derived.zeroize();

    let enc_key = P1305Key::from_slice(&enc_key_bytes);
    let nonce = P1305Nonce::from_slice(&nonce_bytes);
    let mut aad = [0u8; 40];
    aad[..8].copy_from_slice(NIP44_INFO);
    aad[8..].copy_from_slice(&auth_key_bytes);

    let cipher = ChaCha20Poly1305::new(enc_key);
    let payload = Payload {
        msg: plaintext,
        aad: &aad,
    };
    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| "encrypt failed")?;

    enc_key_bytes.zeroize();
    auth_key_bytes.zeroize();
    nonce_bytes.zeroize();

    let mut output = salt.to_vec();
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_legacy(decoded: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    let salt = &decoded[..SALT_LEN];
    let version = decoded[SALT_LEN];
    let encrypted = &decoded[SALT_LEN + VERSION_LEN..];

    let mut derived =
        hash::hkdf_sha256(key, salt, NIP44_INFO, DERIVED_LEN).map_err(|_| "hkdf failed")?;

    let (enc_key_slice, rest) = derived.split_at(KEY_LEN);
    let (auth_key_slice, nonce_slice) = rest.split_at(KEY_LEN);

    let mut enc_key_bytes = [0u8; KEY_LEN];
    let mut auth_key_bytes = [0u8; KEY_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    enc_key_bytes.copy_from_slice(enc_key_slice);
    auth_key_bytes.copy_from_slice(auth_key_slice);
    nonce_bytes.copy_from_slice(nonce_slice);

    derived.zeroize();

    let enc_key = P1305Key::from_slice(&enc_key_bytes);
    let nonce = P1305Nonce::from_slice(&nonce_bytes);
    let mut aad = [0u8; 40];
    aad[..8].copy_from_slice(NIP44_INFO);
    aad[8..].copy_from_slice(&auth_key_bytes);

    let cipher = ChaCha20Poly1305::new(enc_key);
    let payload = Payload {
        msg: encrypted,
        aad: &aad,
    };
    let plaintext = cipher
        .decrypt(nonce, payload)
        .map_err(|_| "decrypt failed")?;

    enc_key_bytes.zeroize();
    auth_key_bytes.zeroize();
    nonce_bytes.zeroize();

    if version == VERSION_LEGACY {
        return Ok(plaintext);
    }
    if version != VERSION_PADDED {
        return Err("unsupported payload version");
    }
    unpad(&plaintext)
}
