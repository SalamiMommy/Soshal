//! RFC 6238 Time-based One-Time Password (TOTP) two-factor authentication.
//!
//! Provides pure-Rust cryptographic 2FA primitives for account security,
//! relay administration, and backup key protection using `totp-rs`.

use totp_rs::{Algorithm, Builder, Secret, Totp};

const DEFAULT_DIGITS: u8 = 6;
const DEFAULT_STEP_SECS: u64 = 30;
const DEFAULT_SKEW_STEPS: u16 = 1;
const DEFAULT_ISSUER: &str = "Soshal";

/// A configured TOTP authenticator instance for account protection.
#[derive(Debug, Clone)]
pub struct TotpAuthenticator {
    totp: Totp,
    secret_base32: String,
    account_name: String,
    issuer: Option<String>,
}

impl TotpAuthenticator {
    /// Create a new authenticator with an existing base32 secret.
    pub fn new(
        secret_base32: &str,
        account_name: &str,
        issuer: Option<&str>,
    ) -> Result<Self, String> {
        let secret = Secret::try_from_base32(secret_base32.trim())
            .map_err(|e| format!("invalid base32 TOTP secret: {e:?}"))?;

        let actual_issuer = issuer.unwrap_or(DEFAULT_ISSUER);
        let totp = Builder::new()
            .with_algorithm(Algorithm::SHA1)
            .with_digits(DEFAULT_DIGITS)
            .with_step_duration(DEFAULT_STEP_SECS)
            .with_skew(DEFAULT_SKEW_STEPS)
            .with_secret(secret)
            .with_account_name(account_name)
            .with_issuer(Some(actual_issuer))
            .build()
            .map_err(|e| format!("failed to build TOTP instance: {e:?}"))?;

        Ok(Self {
            totp,
            secret_base32: secret_base32.trim().to_string(),
            account_name: account_name.to_string(),
            issuer: Some(actual_issuer.to_string()),
        })
    }

    /// Generate a fresh random secret and initialize an authenticator.
    pub fn generate_new(account_name: &str, issuer: Option<&str>) -> Result<Self, String> {
        let mut rng_bytes = [0u8; 20];
        getrandom::fill(&mut rng_bytes)
            .map_err(|e| format!("entropy failure generating TOTP secret: {e}"))?;
        let secret = Secret::new_stack(rng_bytes);
        let base32 = secret.to_base32();
        Self::new(&base32, account_name, issuer)
    }

    /// Return the base32-encoded secret string.
    pub fn secret_base32(&self) -> &str {
        &self.secret_base32
    }

    /// Return the associated account name.
    pub fn account_name(&self) -> &str {
        &self.account_name
    }

    /// Return the issuer organization name.
    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }

    /// Generate a 6-digit TOTP token for the current system time.
    pub fn generate_current(&self) -> String {
        self.totp.generate_current().to_string()
    }

    /// Generate a 6-digit TOTP token for a specific UNIX timestamp in seconds.
    pub fn generate_at(&self, timestamp_secs: u64) -> String {
        self.totp.generate(timestamp_secs).to_string()
    }

    /// Verify a token against current system time (allowing ±1 time step skew).
    pub fn verify_current(&self, token: &str) -> bool {
        self.totp.check_current(token.trim()).is_some()
    }

    /// Verify a token against a specific UNIX timestamp in seconds.
    pub fn verify_at(&self, token: &str, timestamp_secs: u64) -> bool {
        self.totp.check(token.trim(), timestamp_secs).is_some()
    }

    /// Return the remaining time-to-live in seconds for the current 30s token step.
    pub fn ttl_seconds(&self) -> u64 {
        self.totp.ttl()
    }

    /// Generate the standard `otpauth://totp/...` URI suitable for QR codes and authenticator apps.
    pub fn otpauth_url(&self) -> Result<String, String> {
        self.totp
            .to_url()
            .map_err(|e| format!("failed to format otpauth URL: {e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_verify_at_timestamp() {
        let auth =
            TotpAuthenticator::generate_new("alice@soshal.net", Some("Soshal Network")).unwrap();
        assert!(!auth.secret_base32().is_empty());
        assert_eq!(auth.account_name(), "alice@soshal.net");
        assert_eq!(auth.issuer(), Some("Soshal Network"));

        let t0 = 1_700_000_000_u64;
        let code = auth.generate_at(t0);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));

        // Verification at exact same timestamp
        assert!(auth.verify_at(&code, t0));

        // Verification within skew window (t0 + 20s still within step or step+1)
        assert!(auth.verify_at(&code, t0 + 20));

        // Verification outside skew window (t0 + 90s is 3 steps away)
        assert!(!auth.verify_at(&code, t0 + 90));

        // Wrong code fails
        assert!(!auth.verify_at("999999", t0) || code == "999999");
    }

    #[test]
    fn rfc_6238_standard_test_vector() {
        // RFC 6238 Appendix B test secret: "12345678901234567890" ASCII
        // Base32 for ASCII "12345678901234567890" is "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        let secret_b32 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let auth = TotpAuthenticator::new(secret_b32, "test@example.com", Some("RFC6238")).unwrap();

        // Timestamp 59 seconds: counter = 1
        let code59 = auth.generate_at(59);
        assert_eq!(code59, "287082");

        // Timestamp 1111111109 seconds: counter = 37037036
        let code_long = auth.generate_at(1111111109);
        assert_eq!(code_long, "081804");
    }

    #[test]
    fn otpauth_url_formatting() {
        let auth = TotpAuthenticator::new(
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
            "bob@soshal.app",
            Some("SoshalApp"),
        )
        .unwrap();

        let url = auth.otpauth_url().expect("valid otpauth url");
        assert!(url.starts_with("otpauth://totp/"));
        assert!(url.contains("secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
        assert!(url.contains("issuer=SoshalApp"));
        assert!(url.contains("bob%40soshal.app") || url.contains("bob@soshal.app"));
    }

    #[test]
    fn verify_current_roundtrip() {
        let auth = TotpAuthenticator::generate_new("user@domain.com", None).unwrap();
        let code = auth.generate_current();
        assert_eq!(code.len(), 6);
        assert!(auth.verify_current(&code));
        assert!(auth.ttl_seconds() <= 30);
    }
}
