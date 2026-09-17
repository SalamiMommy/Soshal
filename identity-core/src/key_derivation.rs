//! HKDF database encryption key derivation module.
//!
//! Derives local database AES keys from ML-KEM secret keys via HKDF-SHA256.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use soshal_common_core::json_util::json_out;

const SERIALIZATION_FAILED: &str =
    r#"{"success":false,"derived_key_hex":"","error":"serialization failed"}"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeriveDbKeyInput {
    #[serde(alias = "kem_secret_key_hex")]
    pub kem_secret_key_hex: String,
    #[serde(alias = "device_salt_hex")]
    pub device_salt_hex: Option<String>,
}

impl Drop for DeriveDbKeyInput {
    fn drop(&mut self) {
        self.kem_secret_key_hex.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeriveDbKeyOutput {
    pub success: bool,
    pub derived_key_hex: String,
    pub error: Option<String>,
}

/// Derives AES-256 local database encryption key via HKDF-SHA256.
/// Accepts JSON input, returns JSON output.
pub fn derive_db_key_hkdf(input_json: &str) -> String {
    let input: DeriveDbKeyInput = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(e) => {
            return json_out(
                &DeriveDbKeyOutput {
                    success: false,
                    derived_key_hex: String::new(),
                    error: Some(format!("JSON parse error: {}", e)),
                },
                SERIALIZATION_FAILED,
            )
        }
    };

    if input.kem_secret_key_hex.len() > 16_384 {
        return json_out(
            &DeriveDbKeyOutput {
                success: false,
                derived_key_hex: String::new(),
                error: Some("KEM secret key exceeds maximum allowed length".into()),
            },
            SERIALIZATION_FAILED,
        );
    }
    if let Some(ref salt) = input.device_salt_hex {
        if salt.len() > 1024 {
            return json_out(
                &DeriveDbKeyOutput {
                    success: false,
                    derived_key_hex: String::new(),
                    error: Some("Device salt exceeds maximum allowed length".into()),
                },
                SERIALIZATION_FAILED,
            );
        }
    }

    let kem_bytes = match hex::decode(&input.kem_secret_key_hex) {
        Ok(b) if !b.is_empty() => Zeroizing::new(b),
        _ => {
            return json_out(
                &DeriveDbKeyOutput {
                    success: false,
                    derived_key_hex: String::new(),
                    error: Some("Invalid hex in KEM secret key".into()),
                },
                SERIALIZATION_FAILED,
            )
        }
    };

    let derived = match soshal_crypto_core::key_derivation::derive_db_key(
        &kem_bytes,
        input.device_salt_hex.as_deref(),
    ) {
        Ok(d) => Zeroizing::new(d),
        Err(e) => {
            // Fail closed: never hand out a weak (all-zero) key on failure.
            return json_out(
                &DeriveDbKeyOutput {
                    success: false,
                    derived_key_hex: String::new(),
                    error: Some(e),
                },
                SERIALIZATION_FAILED,
            );
        }
    };

    let out = DeriveDbKeyOutput {
        success: true,
        derived_key_hex: hex::encode(derived.as_slice()),
        error: None,
    };

    json_out(&out, SERIALIZATION_FAILED)
}
