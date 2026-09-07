//! Turso database sync FFI module.

use super::db::with_db_result;
use flutter_rust_bridge::frb;

/// Configure Turso database URL and auth bearer token for replication.
#[frb(sync, serialize)]
pub fn db_turso_configure(url: String, auth_token: String) -> Result<String, String> {
    // SSRF/scheme guard: only Turso remote-schema endpoints are accepted so
    // a hostile or typo'd URL cannot sync the local DB out to an arbitrary
    // HTTP host (or a file:/other scheme interpretation).
    if url.is_empty() || url.len() > 512 {
        return Err("Turso URL too long or empty".to_string()).into();
    }
    let parsed = url::Url::parse(&url).map_err(|e| format!("invalid Turso URL: {e}"))?;
    match parsed.scheme() {
        "libsql" | "https" => {}
        s => {
            return Err(format!(
                "Turso URL scheme must be libsql: or https:, got {s:?}"
            ))
            .into()
        }
    }
    if parsed
        .host_str()
        .map(|h| h.trim().is_empty())
        .unwrap_or(true)
    {
        return Err("Turso URL has no host".to_string()).into();
    }
    if auth_token.is_empty() || auth_token.len() > 1024 {
        return Err("Turso auth token must be 1..=1024 chars".to_string()).into();
    }
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
