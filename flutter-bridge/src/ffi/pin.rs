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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn with_repo<T>(
    f: impl FnOnce(&soshal_db_core::repos::settings::SettingsRepo) -> Result<T, String>,
) -> Result<T, String> {
    super::db::with_db_string(|db| {
        let repo = soshal_db_core::repos::settings::SettingsRepo::new(db);
        f(&repo)
    })
}

/// Set (or reset) the lock PIN. Must be 4-12 digits.
#[frb(sync, serialize)]
pub fn pin_set(pin: String) -> Result<bool, String> {
    if !pin.chars().all(|c| c.is_ascii_digit()) || !(4..=12).contains(&pin.len()) {
        return Err("PIN must be 4-12 digits".to_string());
    }
    let mut salt = [0u8; PIN_SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|e| format!("rng: {e}"))?;
    let salt_hex = hex::encode(salt);
    let hash = derive_pin_hash(&pin, &salt_hex, PIN_ITERATIONS, PIN_DK_LEN)?;
    with_repo(|r| {
        r.set(PIN_HASH_KEY, &format!("{salt_hex}:{hash}"))
            .map_err(|e| e.to_string())?;
        r.set("pin_permanently_locked", "false")
            .map_err(|e| e.to_string())
    })?;
    Ok(true)
}

/// Whether a PIN is configured.
#[frb(sync, serialize)]
pub fn pin_has() -> Result<bool, String> {
    with_repo(|r| {
        Ok(r.get(PIN_HASH_KEY)
            .map_err(|e| e.to_string())?
            .map(|v| !v.is_empty())
            .unwrap_or(false))
    })
}

fn check_pin_with_lockout(pin: &str) -> Result<(), String> {
    let now = now_ms();
    let permanent = with_repo(|r| {
        Ok(r.get("pin_permanently_locked")
            .map_err(|e| e.to_string())?
            .map(|v| v == "true")
            .unwrap_or(false))
    })?;
    let stored = with_repo(|r| r.get(PIN_HASH_KEY).map_err(|e| e.to_string()))?;
    let stored = stored
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "no PIN configured; set a PIN in Settings > Security first".to_string())?;
    let (salt_hex, expected) = stored
        .split_once(':')
        .ok_or_else(|| "corrupt pin storage".to_string())?;
    let candidate = derive_pin_hash(pin, salt_hex, PIN_ITERATIONS, PIN_DK_LEN)?;
    let ok = constant_time_equal(&candidate, expected);

    with_repo(|r| {
        let mut state: PinLockoutState = r
            .get("pin_lockout_state")
            .map_err(|e| e.to_string())?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let verdict = apply_pin_attempt(&mut state, now, ok, permanent);
        let persist = || {
            let _ = r.set(
                "pin_lockout_state",
                &serde_json::to_string(&state).unwrap_or_default(),
            );
        };
        match verdict {
            PinVerdict::Ok => {
                persist();
                let _ = r.set("pin_permanently_locked", "false");
                Ok(())
            }
            PinVerdict::Incorrect { .. } => {
                persist();
                Err("incorrect PIN".into())
            }
            PinVerdict::LockedOutUntil(_) => {
                persist();
                Err("too many incorrect PIN attempts; try again later".into())
            }
            PinVerdict::PermanentlyLocked => {
                persist();
                let _ = r.set("pin_permanently_locked", "true");
                Err("account permanently locked after too many failed PIN attempts".into())
            }
        }
    })?;
    Ok(())
}

/// Verify the PIN with full lockout enforcement. Returns Ok(true) on a
/// correct PIN, Ok(false) on wrong PIN / lockout / error.
#[frb(sync, serialize)]
pub fn pin_verify(pin: String) -> Result<bool, String> {
    Ok(check_pin_with_lockout(&pin).is_ok())
}

/// Clear the PIN after verifying the current one.
#[frb(sync, serialize)]
pub fn pin_clear(pin: String) -> Result<bool, String> {
    check_pin_with_lockout(&pin)?;
    with_repo(|r| {
        r.set(PIN_HASH_KEY, "").map_err(|e| e.to_string())?;
        let _ = r.set("pin_lockout_state", "{}");
        let _ = r.set("pin_permanently_locked", "false");
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
            .map_err(|e| e.to_string())?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let permanent = r
            .get("pin_permanently_locked")
            .map_err(|e| e.to_string())?
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
