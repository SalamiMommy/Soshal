//! NIP-44 group-message sealing under a 32-byte shared group key.

use soshal_crypto_core::nip44;
use zeroize::Zeroize;

/// NIP-44 encrypt for group messages.
pub fn nip44_seal_group(plaintext: &str, key_hex: &str) -> Result<String, String> {
    let mut key = [0u8; 32];
    hex::decode_to_slice(key_hex, &mut key).map_err(|_| "bad group key hex".to_string())?;
    let res = nip44::encrypt(plaintext.as_bytes(), &key).map_err(str::to_string);
    key.zeroize();
    res
}

/// NIP-44 decrypt for group messages.
pub fn nip44_open_group(payload: &str, key_hex: &str) -> Result<String, String> {
    let mut key = [0u8; 32];
    hex::decode_to_slice(key_hex, &mut key).map_err(|_| "bad group key hex".to_string())?;
    let res = nip44::decrypt(payload, &key)
        .map_err(str::to_string)
        .and_then(|plaintext| {
            String::from_utf8(plaintext).map_err(|e| format!("plaintext utf8: {e}"))
        });
    key.zeroize();
    res
}

/// Group message envelope: `{"v":1,"payload":<nip44 ciphertext>}` when the
/// group has a shared key; plaintext otherwise (open groups).
pub fn group_message_envelope(content: &str, key_hex: Option<&str>) -> Result<String, String> {
    match key_hex {
        Some(k) => {
            let payload = nip44_seal_group(content, k)?;
            Ok(format!(r#"{{"v":1,"payload":"{}"}}"#, payload))
        }
        None => Ok(content.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_unseal_roundtrip() {
        let key_hex = hex::encode([0x2au8; 32]);
        for msg in ["hello group", "ünïcode 🎉", "x"] {
            let ct = nip44_seal_group(msg, &key_hex).unwrap();
            assert_eq!(nip44_open_group(&ct, &key_hex).unwrap(), msg);
        }
    }

    #[test]
    fn wrong_key_fails() {
        let key_hex = hex::encode([0x11u8; 32]);
        let wrong = hex::encode([0x22u8; 32]);
        let ct = nip44_seal_group("secret msg", &key_hex).unwrap();
        assert!(nip44_open_group(&ct, &wrong).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key_hex = hex::encode([0x33u8; 32]);
        let ct = nip44_seal_group("tamper me", &key_hex).unwrap();
        let mut bytes = ct.into_bytes();
        bytes[0] = if bytes[0] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(nip44_open_group(&tampered, &key_hex).is_err());
    }

    #[test]
    fn sealed_envelope_roundtrip() {
        let key_hex = hex::encode([0x44u8; 32]);
        let env = group_message_envelope("secret", Some(&key_hex)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&env).unwrap();
        assert_eq!(v["v"], 1);
        let payload = v["payload"].as_str().unwrap();
        assert_eq!(nip44_open_group(payload, &key_hex).unwrap(), "secret");
    }
}
