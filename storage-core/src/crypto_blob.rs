//! Zero-overhead encrypted storage at rest for cached media files using AES-256-GCM (ring).

use ring::aead::{BoundKey, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::{Zeroize, Zeroizing};

pub fn encrypt_blob_at_rest(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
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
    if ciphertext_with_nonce.len() < NONCE_LEN {
        return Err("Ciphertext payload too short");
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
}
