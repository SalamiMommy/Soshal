use crate::hash;
use zeroize::Zeroizing;

const DB_KEY_INFO: &[u8] = b"soshal-db-key-v1";
const DB_KEY_SALT: &[u8] = b"soshal-db-key-derivation-salt-v1";

pub fn derive_db_key(kem_secret_key: &[u8], device_salt: Option<&str>) -> Result<[u8; 32], String> {
    let salt = if let Some(ds) = device_salt {
        let mut combined = ds.as_bytes().to_vec();
        combined.extend_from_slice(DB_KEY_SALT);
        hash::sha256(&combined).to_vec()
    } else {
        DB_KEY_SALT.to_vec()
    };

    let mut okm = Zeroizing::new([0u8; 32]);
    // Fail closed: a failed derivation must not produce a weak (all-zero)
    // database key.
    let derived = hash::hkdf_sha256(kem_secret_key, &salt, DB_KEY_INFO, 32)
        .map_err(|e| format!("db key derivation failed: {}", e))?;
    okm.copy_from_slice(&derived);
    Ok(*okm)
}
