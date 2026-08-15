//! Envelope encryption for Blossom media.
//!
//! Media ciphertext is stored on Blossom servers; the envelope holds the
//! decryption material so plaintext never leaves the client:
//! - DM media: hybrid (X25519 + ML-KEM-768) one-shot seal to the recipient's
//!   published hybrid public key.

const MEDIA_ENVELOPE_DOMAIN: &[u8] = b"soshal-media-envelope-v1";

/// Opens DM media sealed under the hybrid public key.
pub fn open_dm(
    ct_hex: &str,
    nonce_hex: &str,
    payload_b64: &str,
    sk_hex: &str,
) -> Result<Vec<u8>, String> {
    soshal_pqc_core::seal::hybrid_unseal(
        payload_b64,
        nonce_hex,
        ct_hex,
        sk_hex,
        MEDIA_ENVELOPE_DOMAIN,
    )
}
