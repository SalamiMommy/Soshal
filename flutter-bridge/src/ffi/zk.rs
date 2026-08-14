//! Zero-Knowledge State Rollup FFI Module

use flutter_rust_bridge::frb;
use soshal_sync_core::zk_rollup::{verify_zk_rollup_json, ZkCrdtRollup};

/// Verify a Zero-Knowledge STARK state rollup proof for feed threads
#[frb(sync, serialize)]
pub fn zk_verify_rollup(rollup_json: String) -> Result<String, String> {
    Ok(verify_zk_rollup_json(&rollup_json))
}

/// Apply a verified ZK state rollup directly to the database cache
#[frb(sync, serialize)]
pub fn zk_apply_rollup(db_path: String, rollup_json: String) -> Result<bool, String> {
    let rollup: ZkCrdtRollup = match serde_json::from_str(&rollup_json) {
        Ok(r) => r,
        Err(e) => return Err(format!("Invalid ZK rollup JSON: {}", e)),
    };

    let db = soshal_db_core::block_on(libsql::Builder::new_local(&db_path).build())
        .map_err(|e| format!("Database open failed: {}", e))?;
    let conn = db
        .connect()
        .map_err(|e| format!("Database connect failed: {}", e))?;

    let engine = soshal_sync_core::zk_rollup::get_global_zk_engine();
    engine.apply_rollup_to_db(&conn, &rollup)
}
