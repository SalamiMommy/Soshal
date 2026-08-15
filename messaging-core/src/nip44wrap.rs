//! NIP-44 encryption wrapper — delegates to crypto-core nip44.
//!
//! Thin adapter layer for message payloads: callers supply a 32-byte
//! conversation key (derived per-conversation by the caller) and this module
//! handles NIP-44 encrypt/decrypt. No ratchet state is kept here; the
//! post-quantum ratchet lives in crypto-core's `pqc_ratchet` module.

use soshal_crypto_core::nip44;

/// A NIP-44 encrypted message payload.
#[derive(Debug, serde::Serialize)]
pub struct EncryptedMessage {
    pub ciphertext: String,
    pub conversation_pubkey: Option<String>,
}

/// Encrypts `plaintext` under a 32-byte conversation key using NIP-44.
///
/// # Errors
/// Returns an error string if the plaintext is empty or encryption fails.
pub fn wrap_message(
    plaintext: &[u8],
    key: &[u8; nip44::KEY_LEN],
) -> Result<EncryptedMessage, String> {
    let ciphertext = nip44::encrypt(plaintext, key).map_err(|e| format!("nip44 encrypt: {e}"))?;
    Ok(EncryptedMessage {
        ciphertext,
        conversation_pubkey: None,
    })
}

/// Decrypts a NIP-44 payload under a 32-byte conversation key.
///
/// # Errors
/// Returns an error string if the payload is malformed or decryption fails.
pub fn unwrap_message(payload: &str, key: &[u8; nip44::KEY_LEN]) -> Result<Vec<u8>, String> {
    nip44::decrypt(payload, key).map_err(|e| format!("nip44 decrypt: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; nip44::KEY_LEN] {
        [byte; nip44::KEY_LEN]
    }

    #[test]
    fn wrap_unwrap_roundtrip() {
        let plaintext = b"dm wrap roundtrip";
        let wrapped = wrap_message(plaintext, &key(0x42)).unwrap();
        assert!(!wrapped.ciphertext.is_empty());
        assert!(wrapped.conversation_pubkey.is_none());
        assert_eq!(
            unwrap_message(&wrapped.ciphertext, &key(0x42)).unwrap(),
            plaintext
        );
    }

    #[test]
    fn wrong_key_fails() {
        let wrapped = wrap_message(b"secret", &key(0x42)).unwrap();
        assert!(unwrap_message(&wrapped.ciphertext, &key(0x01)).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let wrapped = wrap_message(b"secret", &key(0x42)).unwrap();
        let mut bytes = wrapped.ciphertext.into_bytes();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(unwrap_message(&tampered, &key(0x42)).is_err());
    }
}
