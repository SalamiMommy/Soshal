//! PIN lock FFI module
//!
//! Port of the legacy Tauri `session/pin.rs` commands onto db-core settings.
//! Full brute-force protection: attempt counters + temporary lockout +
//! permanent lock, all enforced Rust-side so no PIN-gated path can bypass it.
//! Hashing is PBKDF2-HMAC-SHA256 (600k iterations) via identity-core.

use flutter_rust_bridge::frb;
use soshal_identity_core::security::{
    apply_pin_attempt, constant_time_equal, derive_pin_hash, PinLockoutState, PinVerdict,
    PIN_DK_LEN, PIN_ITERATIONS, PIN_SALT_BYTES,
};

const PIN_HASH_KEY: &str = "pin_hash";

fn with_repo<T>(
    f: impl FnOnce(&soshal_db_core::repos::settings::SettingsRepo) -> Result<T, String>,
) -> Result<T, String> {
    super::db::with_db_string(|db| {
        let repo = soshal_db_core::repos::settings::SettingsRepo::new(db);
        f(&repo)
    })
}

/// Set the lock PIN for the first time. Must be 4-12 digits.
/// Only succeeds when no PIN is currently configured. To change an existing
/// PIN, use `pin_change` which requires the current PIN for authentication.
#[frb(serialize)]
pub async fn pin_set(pin: String) -> Result<bool, String> {
    if !pin.chars().all(|c| c.is_ascii_digit()) || !(4..=12).contains(&pin.len()) {
        return Err("PIN must be 4-12 digits".to_string());
    }
    // Guard against unauthenticated overwrite of an existing PIN.
    if pin_has()? {
        return Err(
            "a PIN is already set; use pin_change to update it with your current PIN".to_string(),
        );
    }
    let mut salt = [0u8; PIN_SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|e| format!("rng: {e}"))?;
    let salt_hex = hex::encode(salt);
    let hash = derive_pin_hash(&pin, &salt_hex, PIN_ITERATIONS, PIN_DK_LEN)?;
    with_repo(|r| {
        r.set(PIN_HASH_KEY, &format!("{salt_hex}:{hash}"))
            .map_err(super::util::to_err)?;
        r.set("pin_permanently_locked", "false")
            .map_err(super::util::to_err)?;
        r.set("pin_lockout_state", "{}")
            .map_err(super::util::to_err)
    })?;
    Ok(true)
}

/// Change the lock PIN. Requires the current PIN for authentication (full
/// lockout enforcement applies). `new_pin` must be 4-12 digits.
#[frb(serialize)]
pub async fn pin_change(old_pin: String, new_pin: String) -> Result<bool, String> {
    if !new_pin.chars().all(|c| c.is_ascii_digit()) || !(4..=12).contains(&new_pin.len()) {
        return Err("new PIN must be 4-12 digits".to_string());
    }
    // Require current PIN verification (includes lockout).
    check_pin_with_lockout(&old_pin)?;
    let mut salt = [0u8; PIN_SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|e| format!("rng: {e}"))?;
    let salt_hex = hex::encode(salt);
    let hash = derive_pin_hash(&new_pin, &salt_hex, PIN_ITERATIONS, PIN_DK_LEN)?;
    with_repo(|r| {
        r.set(PIN_HASH_KEY, &format!("{salt_hex}:{hash}"))
            .map_err(super::util::to_err)?;
        r.set("pin_permanently_locked", "false")
            .map_err(super::util::to_err)?;
        r.set("pin_lockout_state", "{}")
            .map_err(super::util::to_err)
    })?;
    Ok(true)
}

/// Whether a PIN is configured.
#[frb(sync, serialize)]
pub fn pin_has() -> Result<bool, String> {
    with_repo(|r| {
        Ok(r.get(PIN_HASH_KEY)
            .map_err(super::util::to_err)?
            .map(|v| !v.is_empty())
            .unwrap_or(false))
    })
}

fn check_pin_with_lockout(pin: &str) -> Result<(), String> {
    let now = soshal_common_core::util::now_ms() as i64;
    let (permanent, mut state, stored) = super::db::with_db_string(|db| {
        let r = soshal_db_core::repos::settings::SettingsRepo::new(db);
        let permanent = r
            .get("pin_permanently_locked")
            .map_err(super::util::to_err)?
            .map(|v| v == "true")
            .unwrap_or(false);
        let state: PinLockoutState = r
            .get("pin_lockout_state")
            .map_err(super::util::to_err)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let stored = r.get(PIN_HASH_KEY).map_err(super::util::to_err)?;
        Ok((permanent, state, stored))
    })?;

    // Fast-path: if already permanently locked or within active lockout window,
    // apply attempt counter progression without computing expensive 600k PBKDF2 iterations.
    let is_locked = permanent
        || state
            .lockout_until
            .map(|until| now < until)
            .unwrap_or(false);
    if is_locked {
        let verdict = apply_pin_attempt(&mut state, now, false, permanent);
        // Lockout/permanent-lock state MUST be persisted durably. A failure
        // here is a hard error: returning Ok would let the caller bypass the
        // lockout by causing repeated DB write failures.
        with_repo(|r| {
            r.set(
                "pin_lockout_state",
                &serde_json::to_string(&state).unwrap_or_default(),
            )
            .map_err(|e| format!("pin lockout state persist failed: {e}"))?;
            if matches!(verdict, PinVerdict::PermanentlyLocked) {
                r.set("pin_permanently_locked", "true")
                    .map_err(|e| format!("pin permanent lock persist failed: {e}"))?;
            }
            Ok(())
        })?;
        return match verdict {
            PinVerdict::PermanentlyLocked => {
                Err("account permanently locked after too many failed PIN attempts".into())
            }
            _ => Err("too many incorrect PIN attempts; try again later".into()),
        };
    }

    let stored = stored
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "no PIN configured; set a PIN in Settings > Security first".to_string())?;
    let (salt_hex, expected) = stored
        .split_once(':')
        .ok_or_else(|| "corrupt pin storage".to_string())?;
    let candidate = derive_pin_hash(pin, salt_hex, PIN_ITERATIONS, PIN_DK_LEN)?;
    let ok = constant_time_equal(&candidate, expected);

    with_repo(|r| {
        let verdict = apply_pin_attempt(&mut state, now, ok, permanent);
        let state_json = serde_json::to_string(&state).unwrap_or_default();
        match verdict {
            PinVerdict::Ok => {
                // Success: log-and-continue on persist failure (authenticated user
                // should not be locked out because of a logging error).
                if let Err(e) = r.set("pin_lockout_state", &state_json) {
                    eprintln!("pin state persist failed (non-critical, user authenticated): {e}");
                }
                if let Err(e) = r.set("pin_permanently_locked", "false") {
                    eprintln!("pin state persist failed (non-critical, user authenticated): {e}");
                }
                Ok(())
            }
            PinVerdict::Incorrect { .. } => {
                // Wrong PIN: log-and-continue; next attempt will re-read fresh state.
                if let Err(e) = r.set("pin_lockout_state", &state_json) {
                    eprintln!("pin state persist failed (non-critical, wrong PIN): {e}");
                }
                Err("incorrect PIN".into())
            }
            PinVerdict::LockedOutUntil(_) => {
                // Lockout: MUST persist durably — a failure here is a hard error to
                // prevent brute-force via repeated DB write failures.
                r.set("pin_lockout_state", &state_json)
                    .map_err(|e| format!("pin lockout state persist failed: {e}"))?;
                Err("too many incorrect PIN attempts; try again later".into())
            }
            PinVerdict::PermanentlyLocked => {
                // Permanent lock: MUST persist both fields durably.
                r.set("pin_lockout_state", &state_json)
                    .map_err(|e| format!("pin lockout state persist failed: {e}"))?;
                r.set("pin_permanently_locked", "true")
                    .map_err(|e| format!("pin permanent lock persist failed: {e}"))?;
                Err("account permanently locked after too many failed PIN attempts".into())
            }
        }
    })?;
    Ok(())
}

/// Verify the PIN with full lockout enforcement.
///
/// Returns:
/// - `Ok(true)`  — PIN correct, account unlocked.
/// - `Ok(false)` — PIN incorrect (wrong digits).
/// - `Err(_)`    — Account is locked out or permanently locked; the error
///                 message describes the reason. Callers must distinguish
///                 this from a wrong PIN to show the correct UI state.
///
/// (M3 fix: lockout/permanent-lock errors are no longer masked as Ok(false).)
#[frb(serialize)]
pub async fn pin_verify(pin: String) -> Result<bool, String> {
    match check_pin_with_lockout(&pin) {
        Ok(()) => Ok(true),
        // Plain wrong PIN: return Ok(false) so the UI can increment an
        // on-screen attempt counter without treating it as a hard error.
        Err(e) if e == "incorrect PIN" => Ok(false),
        // Lockout or permanent-lock: propagate so the caller can show the
        // appropriate "try again later" / "account disabled" screen.
        Err(e) => Err(e),
    }
}

/// Clear the PIN after verifying the current one. Async: the lockout check
/// runs a 600k-iteration PBKDF2 (~0.3-1 s) and must not block the Dart isolate.
#[frb(serialize)]
pub async fn pin_clear(pin: String) -> Result<bool, String> {
    check_pin_with_lockout(&pin)?;
    with_repo(|r| {
        r.set(PIN_HASH_KEY, "").map_err(super::util::to_err)?;
        r.set("pin_lockout_state", "{}")
            .map_err(|e| format!("pin lockout reset failed: {e}"))?;
        r.set("pin_permanently_locked", "false")
            .map_err(|e| format!("pin lock reset failed: {e}"))?;
        Ok(())
    })?;
    Ok(true)
}

/// Lockout state snapshot for the lock screen UI.
#[frb(sync, serialize)]
pub fn pin_lockout_state() -> Result<String, String> {
    let (state, permanent) = with_repo(|r| {
        let state: serde_json::Value = r
            .get("pin_lockout_state")
            .map_err(super::util::to_err)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let permanent = r
            .get("pin_permanently_locked")
            .map_err(super::util::to_err)?
            .map(|v| v == "true")
            .unwrap_or(false);
        Ok((state, permanent))
    })?;
    super::util::json_ok(serde_json::json!({
        "attemptCount": state["attemptCount"].as_i64().unwrap_or(0),
        "lastAttemptAt": state["lastAttemptAt"].as_i64().unwrap_or(0),
        "lockoutUntil": state["lockoutUntil"].as_i64(),
        "permanentLocked": permanent,
    }))
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use soshal_identity_core::security::PIN_HARD_LIMIT;

    fn fresh_db() -> String {
        let path = soshal_test_util::tmp_path("pin", "pin.db");
        let path = path.to_string_lossy().to_string();
        super::super::db::db_init(path.clone()).unwrap();
        path
    }

    fn state_json() -> serde_json::Value {
        serde_json::from_str(&pin_lockout_state().unwrap()).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_set_rejects_invalid_pins() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        for bad in ["123", "1234567890123", "12a4", "", " 12 "] {
            let e = pin_set(bad.to_string()).await.unwrap_err();
            assert!(e.contains("4-12 digits"), "pin {bad:?} -> {e}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_set_has_verify_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(!pin_has().unwrap());
        assert!(pin_set("2468".to_string()).await.unwrap());
        assert!(pin_has().unwrap());
        assert!(pin_verify("2468".to_string()).await.unwrap());
        assert!(!pin_verify("0000".to_string()).await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_set_rejects_overwrite_without_pin_change() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(pin_set("1234".to_string()).await.unwrap());
        // Direct overwrite via pin_set must be rejected when a PIN already exists.
        let e = pin_set("5678".to_string()).await.unwrap_err();
        assert!(
            e.contains("pin_change"),
            "expected redirect to pin_change, got: {e}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_change_requires_correct_old_pin() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(pin_set("1234".to_string()).await.unwrap());
        // Wrong old PIN → rejected
        let e = pin_change("0000".to_string(), "5678".to_string())
            .await
            .unwrap_err();
        assert!(e.contains("incorrect PIN"), "got: {e}");
        // Correct old PIN → accepted
        assert!(pin_change("1234".to_string(), "5678".to_string())
            .await
            .unwrap());
        assert!(pin_verify("5678".to_string()).await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_lockout_after_three_failures() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(pin_set("1357".to_string()).await.unwrap());
        for _ in 0..3 {
            assert!(!pin_verify("0000".to_string()).await.unwrap());
        }
        let s = state_json();
        assert_eq!(s["attemptCount"], 3);
        let until = s["lockoutUntil"].as_i64().unwrap();
        let now = soshal_common_core::util::now_ms() as i64;
        assert!(until > now, "lockoutUntil {until} should be in the future");
        assert_eq!(s["permanentLocked"], false);
        // M3 fix: a PIN attempt during lockout now returns Err (not Ok(false)).
        let err = pin_verify("1357".to_string())
            .await
            .expect_err("correct PIN should be blocked during lockout");
        assert!(
            err.contains("too many incorrect") || err.contains("locked"),
            "error should describe lockout, got: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_permanent_lock_after_hard_limit() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(pin_set("1357".to_string()).await.unwrap());
        for _ in 0..PIN_HARD_LIMIT {
            // Each attempt returns either Ok(false) (wrong PIN pre-lockout)
            // or Err (once the account is locked). Both are fine here.
            let _ = pin_verify("0000".to_string()).await;
        }
        let s = state_json();
        assert_eq!(s["attemptCount"], PIN_HARD_LIMIT);
        assert_eq!(s["permanentLocked"], true);
        // M3 fix: permanently locked returns Err, not Ok(false).
        let err = pin_verify("1357".to_string())
            .await
            .expect_err("correct PIN should be rejected after permanent lock");
        assert!(
            err.contains("permanently locked") || err.contains("locked"),
            "error should describe permanent lock, got: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn pin_clear_requires_current_pin() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = fresh_db();
        assert!(pin_set("97531".to_string()).await.unwrap());
        assert!(
            pin_clear("0000".to_string()).await.is_err(),
            "wrong PIN rejected"
        );
        assert!(pin_has().unwrap());
        assert!(pin_clear("97531".to_string()).await.unwrap());
        assert!(!pin_has().unwrap());
        // M3 fix: "no PIN configured" now propagates as Err (not Ok(false)).
        let err = pin_verify("97531".to_string())
            .await
            .expect_err("should error when no PIN is configured");
        assert!(
            err.contains("no PIN configured"),
            "expected 'no PIN configured', got: {err}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[allow(clippy::await_holding_lock)]
    async fn pin_corrupt_storage_detected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let p = fresh_db();
        with_repo(|r| {
            r.set("pin_hash", "garbage")
                .map_err(crate::ffi::util::to_err)
        })
        .unwrap();
        let _ = p;
        // M3 fix: corrupt storage now propagates as Err (not Ok(false)).
        let err = pin_verify("1357".to_string())
            .await
            .expect_err("corrupt storage should return Err");
        assert!(
            err.contains("corrupt") || err.contains("invalid"),
            "expected corrupt-storage error, got: {err}"
        );
    }
}
