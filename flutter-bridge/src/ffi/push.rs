//! Push FFI module
//! Push token management

use flutter_rust_bridge::frb;

/// Register (or clear, when empty) the push token for the active account.
/// Persisted to session.json; actual FCM delivery is backend-gated (needs
/// google-services.json in the Android build).
#[frb(sync, serialize)]
pub fn push_register_token(token: String) -> Result<bool, String> {
    super::session::session_register_push_token(token).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_token_requires_db_and_session() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(push_register_token("fcm-token".to_string()).is_err());
    }
}
