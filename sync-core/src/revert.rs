//! Per-kind revert strategies for the transaction graph.
//!
//! Each revert is idempotent and local-only: the UI re-renders whatever SQLite
//! holds afterward. Unknown kinds are no-ops — a revert must never fail the
//! whole rollback chain.

use libsql::{params, params_from_iter, Connection};
use soshal_db_core::block_on;

pub const KIND_POST: &str = "post";
pub const KIND_LIKE: &str = "like";
pub const KIND_PROFILE: &str = "profile";

/// Undoes the local side-effects of one action. Idempotent.
pub fn revert(conn: &Connection, kind: &str, payload_json: &str) -> Result<(), String> {
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|e| format!("revert payload: {e}"))?;
    match kind {
        KIND_POST => {
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                // Tombstone, don't hard-delete: the row must stay to propagate
                // the deletion to peers and keep reply linkage intact.
                block_on(
                    conn.execute("UPDATE posts SET is_deleted = 1 WHERE id = ?1", params![id]),
                )
                .map_err(|e| format!("revert post: {e}"))?;
            }
            Ok(())
        }
        KIND_LIKE => {
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                block_on(conn.execute("DELETE FROM reactions WHERE id = ?1", params![id]))
                    .map_err(|e| format!("revert like: {e}"))?;
            }
            Ok(())
        }
        KIND_PROFILE => {
            let Some(pubkey) = payload.get("pubkey").and_then(|v| v.as_str()) else {
                return Ok(());
            };
            let Some(prior) = payload.get("prior") else {
                return Ok(());
            };
            let mut cols = Vec::new();
            let mut vals: Vec<String> = Vec::new();
            for (i, k) in ["name", "display_name", "about", "picture"]
                .iter()
                .enumerate()
            {
                if let Some(v) = prior.get(*k).and_then(|x| x.as_str()) {
                    cols.push(format!("{k} = ?{}", i + 1));
                    vals.push(v.to_string());
                }
            }
            if cols.is_empty() {
                return Ok(());
            }
            let sql = format!(
                "UPDATE users SET {} WHERE pubkey = ?{}",
                cols.join(", "),
                vals.len() + 1
            );
            vals.push(pubkey.to_string());
            block_on(conn.execute(&sql, params_from_iter(vals.iter().map(String::as_str))))
                .map_err(|e| format!("revert profile: {e}"))?;
            Ok(())
        }
        _ => Ok(()),
    }
}
