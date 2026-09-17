//! Zero-overhead encrypted storage at rest for cached media files using AES-256-GCM (ring).

use ring::aead::{BoundKey, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_BLOB_LEN: usize = 64 * 1024 * 1024; // 64 MB cap

pub fn encrypt_blob_at_rest(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    if plaintext.len() > MAX_BLOB_LEN {
        return Err("Plaintext exceeds maximum blob length (64MB)");
    }
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| "Failed to generate random nonce")?;

    let unbound_key = UnboundKey::new(&AES_256_GCM, key).map_err(|_| "Invalid key length")?;

    struct OneNonceSequence(Option<ring::aead::Nonce>);
    impl ring::aead::NonceSequence for OneNonceSequence {
        fn advance(&mut self) -> Result<ring::aead::Nonce, ring::error::Unspecified> {
            self.0.take().ok_err(ring::error::Unspecified)
        }
    }

    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce_bytes)
        .map_err(|_| "Failed to build nonce")?;
    let mut key_handle = ring::aead::SealingKey::new(unbound_key, OneNonceSequence(Some(nonce)));

    let tag_len = AES_256_GCM.tag_len();
    let mut result = Vec::with_capacity(NONCE_LEN + plaintext.len() + tag_len);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(plaintext);

    let tag = key_handle
        .seal_in_place_separate_tag(ring::aead::Aad::empty(), &mut result[NONCE_LEN..])
        .map_err(|_| {
            result.zeroize();
            "Encryption failed"
        })?;

    result.extend_from_slice(tag.as_ref());
    Ok(result)
}

pub fn decrypt_blob_at_rest(
    key: &[u8; 32],
    ciphertext_with_nonce: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if ciphertext_with_nonce.len() < NONCE_LEN + AES_256_GCM.tag_len() {
        return Err("Ciphertext payload too short");
    }
    if ciphertext_with_nonce.len() > MAX_BLOB_LEN + NONCE_LEN + AES_256_GCM.tag_len() {
        return Err("Ciphertext exceeds maximum blob length (64MB)");
    }

    let (nonce_bytes, data) = ciphertext_with_nonce.split_at(NONCE_LEN);
    let unbound_key = UnboundKey::new(&AES_256_GCM, key).map_err(|_| "Invalid key length")?;

    struct OneNonceSequence(Option<ring::aead::Nonce>);
    impl ring::aead::NonceSequence for OneNonceSequence {
        fn advance(&mut self) -> Result<ring::aead::Nonce, ring::error::Unspecified> {
            self.0.take().ok_err(ring::error::Unspecified)
        }
    }

    let nonce =
        ring::aead::Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| "Invalid nonce")?;
    let mut key_handle = ring::aead::OpeningKey::new(unbound_key, OneNonceSequence(Some(nonce)));

    let mut buf = Zeroizing::new(data.to_vec());
    let decrypted = key_handle
        .open_in_place(ring::aead::Aad::empty(), &mut buf)
        .map_err(|_| "Decryption failed / tag mismatch")?;

    let decrypted_len = decrypted.len();
    buf.truncate(decrypted_len);
    Ok(buf)
}

trait OptionExt<T> {
    fn ok_err<E>(self, err: E) -> Result<T, E>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_err<E>(self, err: E) -> Result<T, E> {
        match self {
            Some(v) => Ok(v),
            None => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_blob_at_rest() {
        let key = [42u8; 32];
        let message = b"Secret DM / sensitive cached post content";

        let encrypted = encrypt_blob_at_rest(&key, message).unwrap();
        assert_ne!(encrypted, message);

        let decrypted = decrypt_blob_at_rest(&key, &encrypted).unwrap();
        assert_eq!(&decrypted[..], message);
    }

    #[test]
    fn test_decrypt_payload_too_short() {
        let key = [42u8; 32];
        assert!(decrypt_blob_at_rest(&key, &[0u8; NONCE_LEN]).is_err());
        assert!(decrypt_blob_at_rest(&key, &[0u8; NONCE_LEN + 15]).is_err());
    }

    #[test]
    fn test_blob_payload_max_len_bounds() {
        let key = [42u8; 32];
        let oversized = vec![0u8; MAX_BLOB_LEN + 1];
        assert!(encrypt_blob_at_rest(&key, &oversized).is_err());

        let oversized_ct = vec![0u8; MAX_BLOB_LEN + NONCE_LEN + AES_256_GCM.tag_len() + 1];
        assert!(decrypt_blob_at_rest(&key, &oversized_ct).is_err());
    }
}
