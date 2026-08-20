use zeroize::Zeroize;

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let mut dk = vec![0u8; dk_len];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, iterations, &mut dk);
    dk
}

pub fn constant_time_equal(a: &str, b: &str) -> bool {
    soshal_common_core::util::constant_time_eq(a.as_bytes(), b.as_bytes())
}

/// Derives a PIN hash with a server-chosen salt and fixed parameters.
/// Returns `Err` on invalid parameters instead of silently producing an
/// unusable empty string.
pub fn derive_pin_hash(
    pin: &str,
    salt_hex: &str,
    iterations: u32,
    dk_len: usize,
) -> Result<String, String> {
    if pin.is_empty() || pin.len() > 64 {
        return Err("invalid pin".into());
    }
    if !(600_000..=1_000_000).contains(&iterations) || !(16..=64).contains(&dk_len) {
        return Err("invalid kdf parameters".into());
    }
    let salt = hex::decode(salt_hex).map_err(|_| "invalid salt".to_string())?;
    if salt.len() < 16 {
        return Err("salt too short".into());
    }
    let mut dk = pbkdf2_hmac_sha256(pin.as_bytes(), &salt, iterations, dk_len);
    let hex_str = hex::encode(&dk);
    dk.zeroize();
    Ok(hex_str)
}

// ─── PIN lockout state machine ────────────────────────────────────────────────

/// KDF parameters used for PIN hashing.
pub const PIN_ITERATIONS: u32 = 600_000;
pub const PIN_DK_LEN: usize = 32;
pub const PIN_SALT_BYTES: usize = 16;

/// Incorrect attempts allowed before a temporary lockout begins.
pub const PIN_MAX_ATTEMPTS: i64 = 3;
/// Incorrect attempts that permanently lock the account.
pub const PIN_HARD_LIMIT: i64 = 10;
/// Lockout durations in ms, indexed by attempt count past the threshold.
pub const PIN_LOCKOUT_DELAYS: [i64; 5] = [5000, 15000, 30000, 60000, 120000];

/// Persistent PIN-attempt counters. Serialized shape matches the legacy
/// `pin_lockout_state` settings key.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub struct PinLockoutState {
    #[serde(rename = "attemptCount", default)]
    pub attempt_count: i64,
    #[serde(rename = "lastAttemptAt", default)]
    pub last_attempt_at: i64,
    #[serde(rename = "lockoutUntil", default)]
    pub lockout_until: Option<i64>,
}

/// Outcome of applying one PIN verification attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinVerdict {
    /// Correct PIN; counters were reset.
    Ok,
    /// Wrong PIN; `locked_until` is set when a lockout now applies.
    Incorrect { locked_until: Option<i64> },
    /// Still inside an existing lockout window; attempt was recorded.
    LockedOutUntil(i64),
    /// Attempt counter hit the hard limit; account is permanently locked.
    PermanentlyLocked,
}

/// Applies one PIN attempt to the lockout state machine. Pure logic with no
/// I/O: the caller persists `state` and any permanent-lock flag per verdict.
pub fn apply_pin_attempt(
    state: &mut PinLockoutState,
    now: i64,
    correct: bool,
    permanent_locked: bool,
) -> PinVerdict {
    if permanent_locked {
        return PinVerdict::PermanentlyLocked;
    }

    if let Some(until) = state.lockout_until {
        if now < until {
            state.attempt_count += 1;
            state.last_attempt_at = now;
            if state.attempt_count >= PIN_HARD_LIMIT {
                return PinVerdict::PermanentlyLocked;
            }
            return PinVerdict::LockedOutUntil(until);
        }
    }

    if correct {
        state.attempt_count = 0;
        state.last_attempt_at = now;
        state.lockout_until = None;
        return PinVerdict::Ok;
    }

    state.attempt_count += 1;
    state.last_attempt_at = now;
    if state.attempt_count >= PIN_HARD_LIMIT {
        return PinVerdict::PermanentlyLocked;
    }
    if state.attempt_count >= PIN_MAX_ATTEMPTS {
        let idx = (state.attempt_count - PIN_MAX_ATTEMPTS)
            .clamp(0, PIN_LOCKOUT_DELAYS.len() as i64 - 1) as usize;
        let until = now + PIN_LOCKOUT_DELAYS[idx];
        state.lockout_until = Some(until);
        return PinVerdict::Incorrect {
            locked_until: Some(until),
        };
    }
    PinVerdict::Incorrect { locked_until: None }
}
