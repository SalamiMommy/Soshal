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

use crate::hash;
use soshal_common_core::util::constant_time_eq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

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
/// Plaintext lengths below this use the 2-byte u16 prefix; lengths at or
/// above use the extended 6-byte prefix (2 zero bytes + u32) per NIP-44 spec
/// step 4.
const EXTENDED_PREFIX_THRESHOLD: usize = 65_536;
/// Length of the small-u16 plaintext prefix.
const PREFIX_LEN_SMALL: usize = 2;
/// Length of the extended-u32 plaintext prefix.
const PREFIX_LEN_EXTENDED: usize = 6;

/// NIP-44 padding: plaintext length prefix (2 bytes for <65536, 6 bytes —
/// `0x00 0x00` + u32 — otherwise), then plaintext, then zero padding to
/// `calc_padding` total size (32-byte chunks, min 32, scaling to power-of-two
/// chunks for large plaintexts).
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

/// The plaintext length prefix for NIP-44 padding: u16 for lengths below
/// `EXTENDED_PREFIX_THRESHOLD`, extended 6-byte (`0x00 0x00` + u32) otherwise.
/// Returns (prefix_bytes, prefix_len).
fn encode_prefix(len: usize) -> ([u8; PREFIX_LEN_EXTENDED], usize) {
    if len >= EXTENDED_PREFIX_THRESHOLD {
        let mut out = [0u8; PREFIX_LEN_EXTENDED];
        out[2..].copy_from_slice(&(len as u32).to_be_bytes());
        (out, PREFIX_LEN_EXTENDED)
    } else {
        let mut out = [0u8; PREFIX_LEN_EXTENDED];
        out[..PREFIX_LEN_SMALL].copy_from_slice(&(len as u16).to_be_bytes());
        (out, PREFIX_LEN_SMALL)
    }
}

/// Read the plaintext length from a padded blob, returning
/// `(unpadded_len, prefix_len)`. Rejects a zero first-two-bytes that is not a
/// valid extended prefix.
fn decode_prefix(padded: &[u8]) -> Result<(usize, usize), &'static str> {
    if padded.len() < 2 {
        return Err("padded payload too short");
    }
    let first_two = u16::from_be_bytes([padded[0], padded[1]]);
    if first_two == 0 {
        if padded.len() < PREFIX_LEN_EXTENDED {
            return Err("padded payload too short");
        }
        let len = u32::from_be_bytes([padded[2], padded[3], padded[4], padded[5]]) as usize;
        if len < EXTENDED_PREFIX_THRESHOLD {
            return Err("invalid extended padding length");
        }
        Ok((len, PREFIX_LEN_EXTENDED))
    } else {
        Ok((first_two as usize, PREFIX_LEN_SMALL))
    }
}

pub fn pad(plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    if plaintext.is_empty() {
        return Err("empty plaintext");
    }
    let (prefix, prefix_len) = encode_prefix(plaintext.len());
    let take = calc_padding(plaintext.len()) - plaintext.len();
    let mut out = Vec::with_capacity(prefix_len + plaintext.len() + take);
    out.extend_from_slice(&prefix[..prefix_len]);
    out.extend_from_slice(plaintext);
    out.resize(out.len() + take, 0);
    Ok(out)
}

/// Removes NIP-44 padding; rejects malformed lengths and wrong total size.
pub fn unpad(padded: &[u8]) -> Result<Vec<u8>, &'static str> {
    if padded.len() < PREFIX_LEN_SMALL + 32 {
        return Err("padded payload too short");
    }
    let (unpadded_len, prefix_len) = decode_prefix(padded)?;
    if unpadded_len == 0 {
        return Err("empty plaintext");
    }
    if padded.len() != prefix_len + calc_padding(unpadded_len) {
        return Err("invalid padding");
    }
    Ok(padded[prefix_len..prefix_len + unpadded_len].to_vec())
}

/// Removes NIP-44 padding in-place on a mutable byte vector without extra heap allocations.
pub fn unpad_in_place(mut padded: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if padded.len() < PREFIX_LEN_SMALL + 32 {
        padded.zeroize();
        return Err("padded payload too short");
    }
    let (unpadded_len, prefix_len) = match decode_prefix(&padded) {
        Ok(v) => v,
        Err(e) => {
            padded.zeroize();
            return Err(e);
        }
    };
    if unpadded_len == 0 {
        padded.zeroize();
        return Err("empty plaintext");
    }
    if padded.len() != prefix_len + calc_padding(unpadded_len) {
        padded.zeroize();
        return Err("invalid padding");
    }
    padded.drain(0..prefix_len);
    padded.truncate(unpadded_len);
    Ok(padded)
}

type DerivedKeys = (
    Zeroizing<[u8; KEY_LEN]>,
    Zeroizing<[u8; NONCE_LEN]>,
    Zeroizing<[u8; KEY_LEN]>,
);

/// NIP-44 v2 spec derivation. The conversation key is the HKDF extract step
/// `ck = HMAC-SHA256("nip44-v2", key)`; message keys are the expand step
/// `T(i) = HMAC-SHA256(ck, T(i-1) ‖ nonce ‖ i)`, truncated to 76 bytes
/// (T1 ‖ T2 ‖ T3, mapped to enc-key 32 ‖ chacha nonce 12 ‖ auth-key 32).
fn spec_derive_keys(ck: &[u8; KEY_LEN], nonce: &[u8; SALT_LEN]) -> DerivedKeys {
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
    let mut t3 = hash::hmac_sha256(ck, &t3_input);
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
    t3.zeroize();
    (
        Zeroizing::new(enc_key),
        Zeroizing::new(chacha_nonce),
        Zeroizing::new(auth_key),
    )
}

use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

/// A 32-byte key that is always scrubbed on drop.  Used as both the HashMap
/// lookup key and the stored conversation key so that eviction automatically
/// zeroes the bytes — no manual zeroize call is required at eviction sites.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq, Hash)]
struct ZeroizeKey([u8; KEY_LEN]);

struct CkCache {
    map: HashMap<ZeroizeKey, ZeroizeKey>,
    queue: VecDeque<ZeroizeKey>,
}

static CK_CACHE: RwLock<Option<CkCache>> = RwLock::new(None);

/// Derives and caches the NIP-44 v2 conversation key `ck = HMAC-SHA256("nip44-v2", key)`.
///
/// Returned key is wrapped in `Zeroizing` so callers that fail to zeroize it
/// explicitly still get the bytes scrubbed on drop.
pub fn derive_conversation_key(key: &[u8; KEY_LEN]) -> Zeroizing<[u8; KEY_LEN]> {
    let lookup = ZeroizeKey(*key);
    {
        let guard = CK_CACHE.read().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = guard.as_ref() {
            if let Some(ck) = cache.map.get(&lookup) {
                return Zeroizing::new(ck.0);
            }
        }
    }
    let ck = hash::hmac_sha256(NIP44_INFO, key);
    let mut guard = CK_CACHE.write().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| CkCache {
        map: HashMap::with_capacity(64),
        queue: VecDeque::with_capacity(64),
    });
    if cache.map.contains_key(&lookup) {
        return Zeroizing::new(ck);
    }
    if cache.map.len() >= 128 {
        if let Some(oldest) = cache.queue.pop_front() {
            // ZeroizeOnDrop scrubs both oldest (key) and the removed value
            // automatically when they go out of scope — no explicit zeroize
            // call is needed here.
            cache.map.remove(&oldest);
        }
    }
    cache.map.insert(ZeroizeKey(*key), ZeroizeKey(ck));
    cache.queue.push_back(ZeroizeKey(*key));
    Zeroizing::new(ck)
}

/// Clear and zeroize all cached NIP-44 v2 conversation keys.
pub fn clear_conversation_key_cache() {
    let mut guard = CK_CACHE.write().unwrap_or_else(|e| e.into_inner());
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
    let (prefix, prefix_len) = encode_prefix(plaintext.len());
    let mut nonce = [0u8; SALT_LEN];
    getrandom::fill(&mut nonce).map_err(|_| "rng failed")?;
    let ck = derive_conversation_key(key);
    let (enc_key_bytes, nonce12, auth_key_bytes) = spec_derive_keys(&ck, &nonce);

    let padded_len = calc_padding(plaintext.len());
    let mut payload = Vec::with_capacity(VERSION_LEN + SALT_LEN + prefix_len + padded_len + 32);
    payload.push(VERSION_PADDED);
    payload.extend_from_slice(&nonce);
    let cipher_start = payload.len();
    payload.extend_from_slice(&prefix[..prefix_len]);
    payload.extend_from_slice(plaintext);
    let take = padded_len - plaintext.len();
    payload.resize(payload.len() + take, 0);

    // ChaCha20 stream cipher in-place over the padded plaintext slice.
    use chacha20::cipher::{KeyIvInit, StreamCipher as _};
    let mut cipher = chacha20::ChaCha20::new(
        chacha20::Key::from_slice(&enc_key_bytes[..]),
        chacha20::Nonce::from_slice(&nonce12[..]),
    );
    cipher.apply_keystream(&mut payload[cipher_start..]);
    drop(enc_key_bytes);
    drop(nonce12);

    // HMAC-SHA256 over nonce ‖ ciphertext (constant-time on verify).
    let mac = hash::hmac_sha256_slices(&auth_key_bytes[..], &[&nonce, &payload[cipher_start..]]);
    drop(auth_key_bytes);

    payload.extend_from_slice(&mac);
    Ok(general_purpose::STANDARD.encode(&payload))
}

/// NIP-44 v2 spec decryption.
///
/// The v2 path is attempted only when the payload is structurally a v2 blob:
/// `2 ‖ nonce(32) ‖ padded length ‖ padded plaintext ‖ hmac(32)` (padded
/// length prefix is 2 bytes for plaintext <65536, extended 6 bytes otherwise)
/// with the length field consistent with the total size. A v2-shaped payload
/// that fails AEAD verification returns `Err` immediately — it is **never**
/// re-interpreted through the legacy path. Mixing the two would create an
/// authentication oracle (chosen-ciphertext bypass).
///
/// The legacy path is reserved for everything else, including pre-v2
/// ciphertexts whose random salt byte happens to be `0x02` (1/256 of legacy
/// blobs) but whose length does not match the exact v2 layout.
pub fn decrypt(payload: &str, key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    let decoded = general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| "invalid base64")?;
    if decoded.is_empty() {
        return Err("empty payload");
    }
    if decoded[0] == VERSION_PADDED && is_spec_shape(&decoded) {
        // v2 payload: succeed or fail — never fall through to the legacy path.
        return decrypt_spec(decoded, key);
    }
    decrypt_legacy(&decoded, key)
}

/// True when the blob matches the exact NIP-44 v2 length layout. Two forms:
/// small plaintext (<65536) is `2 ‖ nonce(32) ‖ padded(2-byte prefix, ≥32,
/// 32-byte multiples) ‖ hmac(32)` — total `67 + 32k`; large plaintext
/// (≥65536) is `2 ‖ nonce(32) ‖ padded(6-byte prefix, ≥65536, 32-byte
/// multiples) ‖ hmac(32)` — total `71 + 32k`. Legacy blobs are
/// `49 + plaintext_len`, so non-32-aligned shapes fall through to the legacy
/// path. Blobs matching both a v2 layout and the legacy layout are genuinely
/// ambiguous and stay on the v2 path — the AEAD tag decides.
fn is_spec_shape(decoded: &[u8]) -> bool {
    // Small layout: fixed part = version(1) + salt(32) + u16 prefix(2) + hmac(32) = 67.
    const V2_FIXED_SMALL: usize = VERSION_LEN + SALT_LEN + PREFIX_LEN_SMALL + 32; // 67
                                                                                  // Large layout: fixed part = version(1) + salt(32) + extended prefix(6) + hmac(32) = 71.
    const V2_FIXED_LARGE: usize = VERSION_LEN + SALT_LEN + PREFIX_LEN_EXTENDED + 32; // 71
                                                                                     // The large layout's padded region is always ≥ 65536 bytes.
    const V2_MIN_LARGE: usize = V2_FIXED_LARGE + EXTENDED_PREFIX_THRESHOLD; // 65607

    (decoded.len() >= V2_FIXED_SMALL + 32 && (decoded.len() - V2_FIXED_SMALL).is_multiple_of(32))
        || (decoded.len() >= V2_MIN_LARGE && (decoded.len() - V2_FIXED_LARGE).is_multiple_of(32))
}

fn decrypt_spec(mut decoded: Vec<u8>, key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    if decoded[0] != VERSION_PADDED {
        decoded.zeroize();
        return Err("unsupported payload version");
    }
    if decoded.len() < VERSION_LEN + SALT_LEN + 2 + 32 + 32 {
        decoded.zeroize();
        return Err("payload too short");
    }
    let (expected, nonce_arr, enc_key_bytes, nonce12) = {
        let nonce = &decoded[VERSION_LEN..VERSION_LEN + SALT_LEN];
        let buffer = &decoded[VERSION_LEN + SALT_LEN..decoded.len() - 32];
        let mac = &decoded[decoded.len() - 32..];

        let ck = derive_conversation_key(key);
        let nonce_arr: [u8; SALT_LEN] = match nonce.try_into() {
            Ok(arr) => arr,
            Err(_) => {
                decoded.zeroize();
                return Err("bad nonce");
            }
        };
        let (enc_key_bytes, nonce12, auth_key_bytes) = spec_derive_keys(&ck, &nonce_arr);

        // Constant-time authenticator check before any keystream work.
        let expected = hash::hmac_sha256_slices(&auth_key_bytes[..], &[nonce, buffer]);
        drop(auth_key_bytes);
        if expected.len() != mac.len() || !constant_time_eq(&expected, mac) {
            decoded.zeroize();
            return Err("decrypt failed");
        }
        (expected, nonce_arr, enc_key_bytes, nonce12)
    };
    let _ = (expected, nonce_arr);

    let payload_len = decoded.len();
    let buffer_range = VERSION_LEN + SALT_LEN..payload_len - 32;

    use chacha20::cipher::{KeyIvInit, StreamCipher as _};
    let mut cipher = chacha20::ChaCha20::new(
        chacha20::Key::from_slice(&enc_key_bytes[..]),
        chacha20::Nonce::from_slice(&nonce12[..]),
    );
    cipher.apply_keystream(&mut decoded[buffer_range]);
    drop(enc_key_bytes);
    drop(nonce12);

    // Truncate the 32-byte HMAC from the end
    decoded.truncate(payload_len - 32);
    // Drain the version byte and 32-byte nonce from the beginning
    decoded.drain(0..VERSION_LEN + SALT_LEN);

    unpad_in_place(decoded)
}

/// Legacy encryption format (pre-spec): random salt, then
/// `salt ‖ version ‖ ciphertext` where the AEAD is ChaCha20-Poly1305 with
/// the auth key passed as AAD. Kept only for decrypting existing data.
#[deprecated(note = "legacy emitter, do not use for new data")]
pub fn encrypt_padded(plaintext: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| "rng failed")?;

    let mut derived =
        hash::hkdf_sha256(key, &salt, NIP44_INFO, DERIVED_LEN).map_err(|_| "hkdf failed")?;

    let (enc_key_slice, rest) = derived.split_at(KEY_LEN);
    let (auth_key_slice, nonce_slice) = rest.split_at(KEY_LEN);

    let mut enc_key_bytes = Zeroizing::new([0u8; KEY_LEN]);
    let mut auth_key_bytes = Zeroizing::new([0u8; KEY_LEN]);
    let mut nonce_bytes = Zeroizing::new([0u8; NONCE_LEN]);
    enc_key_bytes.copy_from_slice(enc_key_slice);
    auth_key_bytes.copy_from_slice(auth_key_slice);
    nonce_bytes.copy_from_slice(nonce_slice);

    derived.zeroize();

    let enc_key = P1305Key::from_slice(&enc_key_bytes[..]);
    let nonce = P1305Nonce::from_slice(&nonce_bytes[..]);
    let mut aad = [0u8; 40];
    aad[..8].copy_from_slice(NIP44_INFO);
    aad[8..].copy_from_slice(&auth_key_bytes[..]);

    let cipher = ChaCha20Poly1305::new(enc_key);
    let payload = Payload {
        msg: plaintext,
        aad: &aad,
    };
    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| "encrypt failed")?;

    drop(enc_key_bytes);
    drop(auth_key_bytes);
    drop(nonce_bytes);

    let mut output = salt.to_vec();
    output.push(VERSION_LEGACY);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_legacy(decoded: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>, &'static str> {
    if decoded.len() <= SALT_LEN {
        return Err("payload too short");
    }
    let salt = &decoded[..SALT_LEN];
    let tagged = matches!(decoded[SALT_LEN], VERSION_LEGACY | VERSION_PADDED);
    let (version, encrypted) = if tagged {
        (decoded[SALT_LEN], &decoded[SALT_LEN + VERSION_LEN..])
    } else {
        (VERSION_LEGACY, &decoded[SALT_LEN..])
    };

    let mut derived =
        hash::hkdf_sha256(key, salt, NIP44_INFO, DERIVED_LEN).map_err(|_| "hkdf failed")?;

    let (enc_key_slice, rest) = derived.split_at(KEY_LEN);
    let (auth_key_slice, nonce_slice) = rest.split_at(KEY_LEN);

    let mut enc_key_bytes = Zeroizing::new([0u8; KEY_LEN]);
    let mut auth_key_bytes = Zeroizing::new([0u8; KEY_LEN]);
    let mut nonce_bytes = Zeroizing::new([0u8; NONCE_LEN]);
    enc_key_bytes.copy_from_slice(enc_key_slice);
    auth_key_bytes.copy_from_slice(auth_key_slice);
    nonce_bytes.copy_from_slice(nonce_slice);

    derived.zeroize();

    let enc_key = P1305Key::from_slice(&enc_key_bytes[..]);
    let nonce = P1305Nonce::from_slice(&nonce_bytes[..]);
    let mut aad = [0u8; 40];
    aad[..8].copy_from_slice(NIP44_INFO);
    aad[8..].copy_from_slice(&auth_key_bytes[..]);

    let cipher = ChaCha20Poly1305::new(enc_key);
    let payload = Payload {
        msg: encrypted,
        aad: &aad,
    };
    let (version, plaintext) = match cipher.decrypt(nonce, payload) {
        Ok(pt) => (version, pt),
        Err(_) if tagged => {
            let payload_untagged = Payload {
                msg: &decoded[SALT_LEN..],
                aad: &aad,
            };
            let pt = cipher
                .decrypt(nonce, payload_untagged)
                .map_err(|_| "decrypt failed")?;
            (VERSION_LEGACY, pt)
        }
        Err(_) => return Err("decrypt failed"),
    };

    drop(enc_key_bytes);
    drop(auth_key_bytes);
    drop(nonce_bytes);

    if version == VERSION_LEGACY {
        return Ok(plaintext);
    }
    if version != VERSION_PADDED {
        let mut p = plaintext;
        p.zeroize();
        return Err("unsupported payload version");
    }
    unpad_in_place(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_multiple_lengths() {
        let key = [0x42u8; 32];
        for len in [1usize, 100, 5000] {
            let pt: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let ct = encrypt(&pt, &key).unwrap();
            assert_eq!(decrypt(&ct, &key).unwrap(), pt);
        }
    }

    #[test]
    fn test_extended_prefix_boundary_roundtrip() {
        // Exercises the 2-byte u16 prefix / 6-byte extended-u32 prefix switch
        // at the 65536 boundary (spec step 4).
        let key = [0x42u8; 32];
        for len in [65_534usize, 65_535, 65_536, 65_537] {
            let pt: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let padded = pad(&pt).unwrap();
            assert_eq!(unpad(&padded).unwrap(), pt);
            let ct = encrypt(&pt, &key).unwrap();
            assert_eq!(decrypt(&ct, &key).unwrap(), pt);
            // The v2 length layout must detect the extended form.
            let decoded = general_purpose::STANDARD.decode(&ct).unwrap();
            assert!(is_spec_shape(&decoded));
        }
    }

    #[test]
    fn test_encryption_nonce_random_per_call() {
        let key = [0x42u8; 32];
        let a = encrypt(b"same plaintext", &key).unwrap();
        let b = encrypt(b"same plaintext", &key).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn test_v2_envelope_format() {
        let key = [0x42u8; 32];
        let pt = b"envelope";
        let ct = encrypt(pt, &key).unwrap();
        let bytes = general_purpose::STANDARD.decode(&ct).unwrap();
        assert_eq!(bytes[0], VERSION_PADDED);
        assert_eq!(
            bytes.len(),
            VERSION_LEN + SALT_LEN + 2 + calc_padding(pt.len()) + 32
        );
    }

    #[test]
    fn test_tampered_nonce_fails() {
        let key = [0x42u8; 32];
        let ct = encrypt(b"tamper nonce", &key).unwrap();
        let mut bytes = general_purpose::STANDARD.decode(&ct).unwrap();
        bytes[VERSION_LEN + 1] ^= 0x01;
        assert!(decrypt(&general_purpose::STANDARD.encode(&bytes), &key).is_err());
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let ct = encrypt(b"tamper ciphertext", &key).unwrap();
        let mut bytes = general_purpose::STANDARD.decode(&ct).unwrap();
        bytes[VERSION_LEN + SALT_LEN + 4] ^= 0x01;
        assert!(decrypt(&general_purpose::STANDARD.encode(&bytes), &key).is_err());
    }

    #[test]
    fn test_tampered_hmac_fails() {
        let key = [0x42u8; 32];
        let ct = encrypt(b"tamper hmac", &key).unwrap();
        let mut bytes = general_purpose::STANDARD.decode(&ct).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        bytes[last - 1] ^= 0x01;
        assert!(decrypt(&general_purpose::STANDARD.encode(&bytes), &key).is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let key = [0x42u8; 32];
        let wrong = [0x00u8; 32];
        let ct = encrypt(b"secret", &key).unwrap();
        assert!(decrypt(&ct, &wrong).is_err());
        assert_eq!(decrypt(&ct, &key).unwrap(), b"secret");
    }

    #[test]
    #[allow(deprecated)]
    fn test_legacy_salt_byte_02_routes_to_legacy() {
        let key = [0x42u8; 32];
        let mut tested = false;
        for _ in 0..4096 {
            let legacy_ct = encrypt_padded(b"legacy stored data", &key).unwrap();
            // Salt byte naturally colliding with VERSION_PADDED (1/256).
            if legacy_ct[0] == VERSION_PADDED {
                // 32 salt + 1 version + 18 plaintext + 16 tag = 67 bytes,
                // NOT a v2-shaped length — must route to the legacy decoder
                // even though decoded[0] == VERSION_PADDED.
                assert_eq!(legacy_ct.len(), 67);
                let encoded = general_purpose::STANDARD.encode(&legacy_ct);
                assert_eq!(decrypt(&encoded, &key).unwrap(), b"legacy stored data");
                tested = true;
                break;
            }
        }
        assert!(tested, "no salt collision in 4096 tries (1/256 expected)");
    }

    #[test]
    #[allow(deprecated)]
    fn test_legacy_decode_path() {
        let key = [0x42u8; 32];
        let legacy_ct = encrypt_padded(b"legacy stored data", &key).unwrap();
        let encoded = general_purpose::STANDARD.encode(&legacy_ct);
        assert_eq!(decrypt(&encoded, &key).unwrap(), b"legacy stored data");
        let mut untagged = legacy_ct[..SALT_LEN].to_vec();
        untagged.extend_from_slice(&legacy_ct[SALT_LEN + VERSION_LEN..]);
        let encoded = general_purpose::STANDARD.encode(&untagged);
        assert_eq!(decrypt(&encoded, &key).unwrap(), b"legacy stored data");
    }

    #[test]
    fn test_error_paths() {
        let key = [0x42u8; 32];
        assert_eq!(encrypt(b"", &key), Err("empty plaintext"));
        assert!(decrypt("", &key).is_err());
        assert!(decrypt("not base64!!!", &key).is_err());
        assert!(decrypt(&general_purpose::STANDARD.encode([]), &key).is_err());
        // Boundary: 65536 plaintext bytes is the first length that uses the
        // extended 6-byte prefix; must encrypt+decrypt round-trip, not reject.
        let big = vec![0xABu8; 65_536];
        assert_eq!(decrypt(&encrypt(&big, &key).unwrap(), &key).unwrap(), big);
    }

    #[test]
    fn test_pad_unpad_error_paths() {
        assert_eq!(pad(b""), Err("empty plaintext"));
        assert!(unpad(b"").is_err());
        let mut too_short = vec![0u8; 34];
        too_short[0] = 0x01;
        assert!(unpad(&too_short).is_err());
        let zero_len = vec![0u8; 34];
        // First-two-bytes 0 signals the extended-u32 prefix; a 0 length there
        // is below the extended threshold and must be rejected.
        assert!(unpad(&zero_len).is_err());
        assert_eq!(
            unpad_in_place(pad(b"in place").unwrap()).unwrap(),
            b"in place"
        );
    }

    #[test]
    fn test_decrypt_spec_rejects_malformed() {
        let key = [0x42u8; 32];
        assert_eq!(
            decrypt_spec(vec![VERSION_PADDED; 1], &key),
            Err("payload too short")
        );
        assert_eq!(
            decrypt_spec(vec![VERSION_LEGACY; 100], &key),
            Err("unsupported payload version")
        );
        assert_eq!(decrypt_legacy(&[], &key), Err("payload too short"));
    }
}
