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
    let host = match parsed.host_str() {
        Some(h) if !h.trim().is_empty() => h.trim(),
        _ => return Err("Turso URL has no host".to_string()).into(),
    };
    if soshal_common_core::url::is_loopback_host(host)
        || soshal_common_core::url::is_private_ip_str(host)
        || soshal_common_core::url::is_private_ipv6_str(host)
    {
        return Err(
            "Turso URL cannot point to loopback, private, or link-local addresses".to_string(),
        )
        .into();
    }
    if auth_token.is_empty() || auth_token.len() > 1024 {
        return Err("Turso auth token must be 1..=1024 chars".to_string()).into();
    }
    super::signer::signer_pubkey()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_turso_configure_ssrf_protection() {
        let bad_urls = [
            "https://localhost:8080",
            "https://127.0.0.1:8080",
            "libsql://127.0.0.1",
            "https://169.254.169.254/latest/meta-data",
            "https://10.0.0.1:8080",
            "https://192.168.1.1",
            "https://172.16.0.1",
            "libsql://[::1]",
            "https://[fe80::1]",
        ];
        for u in bad_urls {
            let err = db_turso_configure(u.to_string(), "tok".to_string());
            assert!(err.is_err(), "URL {u} should have been rejected");
            assert!(
                err.unwrap_err()
                    .contains("cannot point to loopback, private, or link-local"),
                "URL {u} had unexpected error"
            );
        }
    }
}
