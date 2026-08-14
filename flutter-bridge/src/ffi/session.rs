//! Session FFI module
//! Multi-account, keychain, pin/biometrics

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// Session account entry
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionAccount {
    pub pubkey: String,
    pub npub: String,
    pub last_used: u64,
    pub relay_list: Vec<String>,
    #[serde(default)]
    pub push_token: Option<String>,
}

/// Session file structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionData {
    pub active_pubkey: Option<String>,
    pub accounts: Vec<SessionAccount>,
}

lazy_static::lazy_static! {
    static ref SESSION: Mutex<Option<SessionData>> = Mutex::new(None);
}

fn lock_session() -> Result<std::sync::MutexGuard<'static, Option<SessionData>>, String> {
    SESSION
        .lock()
        .map_err(|e| format!("session lock poisoned: {e}"))
}

/// Load session from file
#[frb(sync, serialize)]
pub fn session_load(db_path: String) -> Result<String, String> {
    let session_path = Path::new(&db_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("session.json");

    if session_path.exists() {
        match std::fs::read_to_string(&session_path) {
            Ok(content) => match serde_json::from_str::<SessionData>(&content) {
                Ok(session) => {
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session.clone());
                    super::util::json_ok(session)
                }
                Err(e) => Err(format!("Failed to parse session: {}", e)).into(),
            },
            Err(e) => Err(format!("Failed to read session file: {}", e)).into(),
        }
    } else {
        // No session file yet: still register the empty session so the
        // in-memory SESSION (and thus session_add_account/switch/get_active)
        // sees a loaded state instead of erroring with "Session not loaded".
        let empty = SessionData {
            active_pubkey: None,
            accounts: Vec::new(),
        };
        let mut session_lock = lock_session()?;
        *session_lock = Some(empty.clone());
        super::util::json_ok(empty)
    }
}

/// Save session to file
#[frb(sync, serialize)]
pub fn session_save(db_path: String, session_data: String) -> Result<bool, String> {
    let session_path = Path::new(&db_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("session.json");

    match serde_json::from_str::<SessionData>(&session_data) {
        Ok(session) => {
            match std::fs::write(
                &session_path,
                serde_json::to_string_pretty(&session).unwrap(),
            ) {
                Ok(_) => {
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session);
                    Ok(true).into()
                }
                Err(e) => Err(format!("Failed to write session: {}", e)).into(),
            }
        }
        Err(e) => Err(format!("Invalid session JSON: {}", e)).into(),
    }
}

/// Add account to session
#[frb(sync, serialize)]
pub fn session_add_account(
    pubkey: String,
    npub: String,
    relays_json: String,
) -> Result<bool, String> {
    match serde_json::from_str::<Vec<String>>(&relays_json) {
        Ok(relays) => {
            let account = SessionAccount {
                pubkey,
                npub,
                last_used: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                relay_list: relays,
                push_token: None,
            };

            let mut session_lock = lock_session()?;
            let session = session_lock.get_or_insert_with(|| SessionData {
                active_pubkey: None,
                accounts: Vec::new(),
            });
            session.accounts.push(account.clone());
            if session.active_pubkey.is_none() {
                session.active_pubkey = Some(account.pubkey);
            }
            Ok(true).into()
        }
        Err(e) => Err(format!("Invalid relays JSON: {}", e)).into(),
    }
}

/// Switch to an account
#[frb(sync, serialize)]
pub fn session_switch_account(pubkey: String) -> Result<bool, String> {
    let mut session_lock = lock_session()?;
    if let Some(session) = session_lock.as_mut() {
        if session.accounts.iter().any(|a| a.pubkey == pubkey) {
            session.active_pubkey = Some(pubkey);
            Ok(true).into()
        } else {
            Err("Account not found".to_string()).into()
        }
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Get active account
#[frb(sync, serialize)]
pub fn session_get_active() -> Result<String, String> {
    let session_lock = lock_session()?;
    if let Some(session) = session_lock.as_ref() {
        if let Some(active_pubkey) = &session.active_pubkey {
            let account = session
                .accounts
                .iter()
                .find(|a| &a.pubkey == active_pubkey)
                .cloned();
            super::util::json_ok(account)
        } else {
            super::util::json_ok(None::<SessionAccount>)
        }
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// List all accounts
#[frb(sync, serialize)]
pub fn session_list_accounts() -> Result<String, String> {
    let session_lock = lock_session()?;
    if let Some(session) = session_lock.as_ref() {
        super::util::json_ok(session.accounts.clone())
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Register (or clear, when empty) the push token for the active account.
/// Persists to session.json via the current DB path.
#[frb(sync, serialize)]
pub fn session_register_push_token(token: String) -> Result<bool, String> {
    let mut session_lock = lock_session()?;
    let session = session_lock
        .as_mut()
        .ok_or_else(|| "Session not loaded".to_string())?;
    let active = session
        .active_pubkey
        .clone()
        .ok_or_else(|| "No active account".to_string())?;
    let account = session
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == active)
        .ok_or_else(|| "Active account not found".to_string())?;
    if account.push_token.as_deref() == Some(token.as_str()) {
        return Ok(true).into();
    }
    account.push_token = if token.is_empty() { None } else { Some(token) };
    match super::db::db_path() {
        Ok(db_path) => {
            let data = session.clone();
            drop(session_lock);
            session_save(db_path, serde_json::to_string(&data).unwrap_or_default())
        }
        Err(e) => Err(format!("DB not initialized: {e}")).into(),
    }
}
