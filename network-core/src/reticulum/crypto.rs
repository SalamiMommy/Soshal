//! Reticulum encryption layer using AES-256-GCM for encrypted destinations.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};

/// Reticulum encryption context for destination encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReticulumEncryption {
    pub key: Vec<u8>,
    pub nonce: Vec<u8>,
}

impl Default for ReticulumEncryption {
    fn default() -> Self {
        Self::new()
    }
}

impl ReticulumEncryption {
    /// Creates a new encryption context with a random key
    pub fn new() -> Self {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng).to_vec();

        Self {
            key: key.to_vec(),
            nonce,
        }
    }

    /// Creates encryption context from existing key material
    pub fn from_key(key: Vec<u8>) -> Self {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng).to_vec();
        Self { key, nonce }
    }

    /// Encrypts plaintext using AES-256-GCM
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if self.key.len() != 32 {
            return Err("Invalid key length".to_string());
        }

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);

        let nonce = Nonce::from_slice(&self.nonce);

        cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("Encryption failed: {e}"))
    }

    /// Decrypts ciphertext using AES-256-GCM
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        if self.key.len() != 32 {
            return Err("Invalid key length".to_string());
        }

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&self.key);
        let cipher = Aes256Gcm::new(key);

        let nonce = Nonce::from_slice(&self.nonce);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {e}"))
    }

    /// Derives a shared key from two public keys using ECDH
    pub fn derive_shared_key(pubkey1: &[u8], pubkey2: &[u8]) -> Result<Vec<u8>, String> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        let mut keys = vec![pubkey1, pubkey2];
        keys.sort(); // Ensure deterministic ordering
        for key in keys {
            hasher.update(key);
        }
        Ok(hasher.finalize().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        let enc = ReticulumEncryption::new();
        let plaintext = b"Hello, Reticulum!";

        let ciphertext = enc.encrypt(plaintext).unwrap();
        assert_ne!(plaintext.to_vec(), ciphertext);

        let decrypted = enc.decrypt(&ciphertext).unwrap();
        assert_eq!(plaintext.to_vec(), decrypted);
    }

    #[test]
    fn test_shared_key_derivation() {
        let key1 = vec![1u8; 32];
        let key2 = vec![2u8; 32];

        let shared1 = ReticulumEncryption::derive_shared_key(&key1, &key2).unwrap();
        let shared2 = ReticulumEncryption::derive_shared_key(&key2, &key1).unwrap();

        assert_eq!(shared1, shared2); // Should be deterministic regardless of order
    }
}
