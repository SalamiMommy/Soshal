//! Turso database sync FFI module.

use super::db::with_db_result;
use flutter_rust_bridge::frb;

/// Configure Turso database URL and auth bearer token for replication.
#[frb(sync, serialize)]
pub fn db_turso_configure(url: String, auth_token: String) -> Result<String, String> {
    with_db_result(|db| {
        db.configure_turso(&url, &auth_token)?;
        Ok("Turso database credentials saved".to_string())
    })
}

/// Manually trigger database synchronization with the remote Turso Cloud database.
#[frb(sync, serialize)]
pub fn db_turso_sync() -> Result<String, String> {
    with_db_result(|db| db.sync_turso().map_err(Into::into))
}

/// Retrieve current Turso database replication sync status as a JSON string.
#[frb(sync, serialize)]
pub fn db_turso_status() -> Result<String, String> {
    let status = with_db_result(|db| Ok(db.turso_status()))?;
    serde_json::to_string(&status).map_err(|e| format!("serialize turso status: {e}"))
}
