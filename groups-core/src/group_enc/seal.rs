//! NIP-44 group-message sealing under a 32-byte shared group key.

use soshal_crypto_core::nip44;

/// NIP-44 encrypt for group messages.
pub fn nip44_seal_group(plaintext: &str, key_hex: &str) -> Result<String, String> {
    let mut key = [0u8; 32];
    hex::decode_to_slice(key_hex, &mut key).map_err(|_| "bad group key hex".to_string())?;
    nip44::encrypt(plaintext.as_bytes(), &key).map_err(str::to_string)
}

/// NIP-44 decrypt for group messages.
pub fn nip44_open_group(payload: &str, key_hex: &str) -> Result<String, String> {
    let mut key = [0u8; 32];
    hex::decode_to_slice(key_hex, &mut key).map_err(|_| "bad group key hex".to_string())?;
    let plaintext = nip44::decrypt(payload, &key).map_err(str::to_string)?;
    String::from_utf8(plaintext).map_err(|e| format!("plaintext utf8: {e}"))
}

/// Group message envelope: `{"v":1,"payload":<nip44 ciphertext>}` when the
/// group has a shared key; plaintext otherwise (open groups).
pub fn group_message_envelope(content: &str, key_hex: Option<&str>) -> String {
    match key_hex {
        Some(k) => {
            let payload = nip44_seal_group(content, k).unwrap_or_default();
            format!(r#"{{"v":1,"payload":"{}"}}"#, payload)
        }
        None => content.to_string(),
    }
}
