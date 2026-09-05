//! LAN sync transport security: ephemeral X25519 key exchange plus per-session
//! ChaCha20-Poly1305 frame encryption.
//!
//! The bearer token (checked in the command layer before this module is used)
//! only gates who may connect; this module guarantees the *contents* of the
//! session — a LAN sniffer capturing the token or the plaintext handshake
//! cannot read post data on the wire. Keys are ephemeral per session (no
//! forward secrecy concern for a one-shot pull, but rotation is automatic).

use ring::agreement::{EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::rand::SystemRandom;

use crate::nip44::KEY_LEN;

pub const NONCE_LEN: usize = 12;
pub const PUBLIC_KEY_LEN: usize = 32;

/// An ephemeral X25519 keypair for one LAN session.
pub struct EphemeralSessionKey {
    private: EphemeralPrivateKey,
    pub public: [u8; PUBLIC_KEY_LEN],
}

impl EphemeralSessionKey {
    /// Generates a fresh ephemeral X25519 keypair.
    pub fn generate() -> Result<Self, &'static str> {
        let rng = SystemRandom::new();
        let private =
            EphemeralPrivateKey::generate(&X25519, &rng).map_err(|_| "x25519 keygen failed")?;
        let public_bytes = private
            .compute_public_key()
            .map_err(|_| "x25519 public key failed")?;
        let mut public = [0u8; PUBLIC_KEY_LEN];
        public.copy_from_slice(public_bytes.as_ref());
        Ok(Self { private, public })
    }

    /// Derives the 32-byte session key from this key and the peer's ephemeral
    /// public key (X25519 ECDH + HKDF-SHA256 with a fixed info string).
    /// Consumes the key: it is single-use (one session, one agreement).
    pub fn agree(self, peer_public: &[u8; PUBLIC_KEY_LEN]) -> Result<SessionKey, &'static str> {
        let peer = UnparsedPublicKey::new(&X25519, peer_public);
        let shared = ring::agreement::agree_ephemeral(self.private, &peer, |k| k.to_vec())
            .map_err(|_| "x25519 agreement failed")?;
        let okm = crate::hash::hkdf_sha256(&shared, b"soshal-lan-session", b"lan-session", KEY_LEN)
            .map_err(|_| "session key derivation failed")?;
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&okm);
        Ok(SessionKey {
            key: zeroize::Zeroizing::new(key),
        })
    }
}

/// A per-session symmetric key (ChaCha20-Poly1305). The key bytes are held in
/// a `Zeroizing` wrapper so they are wiped from memory on drop.
pub struct SessionKey {
    key: zeroize::Zeroizing<[u8; KEY_LEN]>,
}

/// Encrypts a frame: random 12-byte nonce prepended to ciphertext+tag.
pub fn encrypt_frame(key: &SessionKey, plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|_| "rng failed")?;
    let cipher = ChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(&key.key[..]));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| "frame encrypt failed")?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypts a frame produced by [`encrypt_frame`]. Returns `Err` on any
/// tampering, truncation, or wrong key.
pub fn decrypt_frame(key: &SessionKey, frame: &[u8]) -> Result<Vec<u8>, &'static str> {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};

    if frame.len() < NONCE_LEN + 16 {
        return Err("frame too short");
    }
    let (nonce_bytes, ct) = frame.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(&key.key[..]));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| "frame decrypt failed (tampered or wrong key)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_distinct_public_keys() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        assert_eq!(a.public.len(), PUBLIC_KEY_LEN);
        assert_eq!(b.public.len(), PUBLIC_KEY_LEN);
        assert_ne!(a.public, b.public);
    }

    #[test]
    fn test_mutual_agreement_roundtrip() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let a_pub = a.public;
        let b_pub = b.public;
        let key_a = a.agree(&b_pub).unwrap();
        let key_b = b.agree(&a_pub).unwrap();
        let frame = encrypt_frame(&key_a, b"hello lan").unwrap();
        let plain = decrypt_frame(&key_b, &frame).unwrap();
        assert_eq!(plain, b"hello lan");
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_full_and_empty() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let key = a.agree(&b.public).unwrap();
        let full: Vec<u8> = (0..=255u8).collect();
        let out = decrypt_frame(&key, &encrypt_frame(&key, &full).unwrap()).unwrap();
        assert_eq!(out, full);
        let out = decrypt_frame(&key, &encrypt_frame(&key, b"").unwrap()).unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn test_nonce_random_per_frame() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let key = a.agree(&b.public).unwrap();
        let f1 = encrypt_frame(&key, b"same plaintext").unwrap();
        let f2 = encrypt_frame(&key, b"same plaintext").unwrap();
        assert_ne!(&f1[..NONCE_LEN], &f2[..NONCE_LEN]);
    }

    #[test]
    fn test_tampered_last_byte_fails() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let key = a.agree(&b.public).unwrap();
        let mut frame = encrypt_frame(&key, b"payload").unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert!(decrypt_frame(&key, &frame).is_err());
    }

    #[test]
    fn test_truncated_and_empty_frames_fail() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let key = a.agree(&b.public).unwrap();
        let frame = encrypt_frame(&key, b"payload").unwrap();
        assert!(decrypt_frame(&key, &frame[..NONCE_LEN + 16 - 1]).is_err());
        assert!(decrypt_frame(&key, b"").is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let a = EphemeralSessionKey::generate().unwrap();
        let b = EphemeralSessionKey::generate().unwrap();
        let c = EphemeralSessionKey::generate().unwrap();
        let key_a = a.agree(&b.public).unwrap();
        let key_c = c.agree(&b.public).unwrap();
        let frame = encrypt_frame(&key_a, b"secret").unwrap();
        assert!(decrypt_frame(&key_c, &frame).is_err());
    }
}
