//! Private community access control & password verification.
//!
//! Provides salted PBKDF2-HMAC-SHA256 password hashing, constant-time verification,
//! and privacy checks for password-protected communities.

use zeroize::Zeroize;

pub const COMMUNITY_PASSWORD_MIN_LEN: usize = 8;
pub const COMMUNITY_PASSWORD_MAX_LEN: usize = 128;
pub const COMMUNITY_PASSWORD_ITERATIONS: u32 = 600_000;
pub const COMMUNITY_PASSWORD_SALT_BYTES: usize = 16;
pub const COMMUNITY_PASSWORD_DK_LEN: usize = 32;

/// Hashes a community password with a freshly generated random salt.
/// Format returned: `<salt_hex>:<hash_hex>`.
pub fn hash_community_password(password: &str) -> Result<String, String> {
    if password.len() < COMMUNITY_PASSWORD_MIN_LEN {
        return Err(format!(
            "Password must be at least {} characters",
            COMMUNITY_PASSWORD_MIN_LEN
        ));
    }
    if password.len() > COMMUNITY_PASSWORD_MAX_LEN {
        return Err(format!(
            "Password must not exceed {} characters",
            COMMUNITY_PASSWORD_MAX_LEN
        ));
    }

    let mut salt = [0u8; COMMUNITY_PASSWORD_SALT_BYTES];
    getrandom::fill(&mut salt)
        .map_err(|_| "failed to draw password salt from OS RNG".to_string())?;

    let mut dk = vec![0u8; COMMUNITY_PASSWORD_DK_LEN];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
        password.as_bytes(),
        &salt,
        COMMUNITY_PASSWORD_ITERATIONS,
        &mut dk,
    );

    let salt_hex = hex::encode(salt);
    let hash_hex = hex::encode(&dk);
    dk.zeroize();

    Ok(format!("{salt_hex}:{hash_hex}"))
}

/// Verifies a candidate password against the stored `<salt_hex>:<hash_hex>`.
/// Comparison is performed in constant time to prevent timing side-channels.
pub fn verify_community_password(password: &str, stored_hash: &str) -> bool {
    if password.is_empty()
        || password.len() > COMMUNITY_PASSWORD_MAX_LEN
        || stored_hash.is_empty()
        || stored_hash.len() > 256
    {
        return false;
    }

    let Some((salt_hex, expected_hash_hex)) = stored_hash.split_once(':') else {
        return false;
    };

    let Ok(salt) = hex::decode(salt_hex) else {
        return false;
    };
    if salt.len() < COMMUNITY_PASSWORD_SALT_BYTES {
        return false;
    }

    let mut dk = vec![0u8; COMMUNITY_PASSWORD_DK_LEN];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
        password.as_bytes(),
        &salt,
        COMMUNITY_PASSWORD_ITERATIONS,
        &mut dk,
    );

    let mut candidate_hash_hex = hex::encode(&dk);
    dk.zeroize();

    let matches = soshal_common_core::util::constant_time_eq(
        candidate_hash_hex.as_bytes(),
        expected_hash_hex.as_bytes(),
    );
    candidate_hash_hex.zeroize();
    matches
}

/// Determines whether a community is private based on its access_type and password presence.
pub fn is_community_private(access_type: &str, has_password_hash: bool) -> bool {
    access_type.trim().eq_ignore_ascii_case("private") || has_password_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_roundtrip() {
        let password = "SuperSecretCommunityPassword123!";
        let stored = hash_community_password(password).expect("hash success");
        assert!(stored.contains(':'));
        assert!(verify_community_password(password, &stored));
        assert!(!verify_community_password("WrongPassword!", &stored));
    }

    #[test]
    fn test_short_and_empty_passwords_rejected() {
        assert!(hash_community_password("").is_err());
        assert!(hash_community_password("123").is_err());
        assert!(hash_community_password("1234").is_err());
        assert!(hash_community_password("12345678").is_ok());
    }

    #[test]
    fn test_oversized_password_rejected() {
        let long = "a".repeat(COMMUNITY_PASSWORD_MAX_LEN + 1);
        assert!(hash_community_password(&long).is_err());
    }

    #[test]
    fn test_corrupted_storage_fails_closed() {
        assert!(!verify_community_password("testpass", ""));
        assert!(!verify_community_password("testpass", "no_colon"));
        assert!(!verify_community_password("testpass", "badhex:1234"));
        assert!(!verify_community_password("testpass", "001122:1234"));
    }

    #[test]
    fn test_unique_salts_for_identical_passwords() {
        let p = "identicalPassword";
        let h1 = hash_community_password(p).unwrap();
        let h2 = hash_community_password(p).unwrap();
        assert_ne!(h1, h2);
        assert!(verify_community_password(p, &h1));
        assert!(verify_community_password(p, &h2));
    }

    #[test]
    fn test_is_community_private() {
        assert!(is_community_private("private", false));
        assert!(is_community_private("Private", false));
        assert!(is_community_private("  PRIVATE  ", false));
        assert!(is_community_private("open", true));
        assert!(is_community_private("private", true));
        assert!(!is_community_private("open", false));
        assert!(!is_community_private("public", false));
    }
}
