//! Zero-Knowledge State Rollup FFI Module

use flutter_rust_bridge::frb;
use soshal_sync_core::zk_rollup::{verify_zk_rollup_json, CommitmentRollup};

/// Verify a Zero-Knowledge state-rollup commitment for feed threads
/// (SHA-256 commitment, not a STARK proof).
#[frb(sync, serialize)]
pub fn zk_verify_rollup(rollup_json: String) -> Result<String, String> {
    Ok(verify_zk_rollup_json(&rollup_json))
}

/// Maximum allowed rollup JSON size (1 MB) to prevent memory exhaustion DoS.
const MAX_ROLLUP_JSON_BYTES: usize = 1024 * 1024;

/// Apply a verified ZK state-rollup commitment directly to the database
/// cache.
#[frb(sync, serialize)]
pub fn zk_apply_rollup(db_path: String, rollup_json: String) -> Result<bool, String> {
    if rollup_json.len() > MAX_ROLLUP_JSON_BYTES {
        return Err("rollup_json exceeds 1MB cap".to_string());
    }

    let rollup: CommitmentRollup = match serde_json::from_str(&rollup_json) {
        Ok(r) => r,
        Err(e) => return Err(format!("Invalid ZK rollup JSON: {}", e)),
    };

    // DB handle is global (opened by `db_init` with migrations + security
    // pragmas); `db_path` is kept for FFI signature compatibility.
    let _ = db_path;
    crate::ffi::db::with_db_string(|db| {
        let conn = db.conn().map_err(super::util::to_err)?;
        let engine = soshal_sync_core::zk_rollup::get_global_zk_engine();
        engine.apply_rollup_to_db(&conn, &rollup)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db_path(label: &str) -> String {
        db::tmp_db(label, "zk")
    }

    fn valid_rollup(thread_id: &str, ops: u64) -> CommitmentRollup {
        let genesis = "0000000000000000000000000000000000000000000000000000000000000000";
        let final_state = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(thread_id.as_bytes());
        ctx.update(genesis.as_bytes());
        ctx.update(final_state.as_bytes());
        ctx.update(&ops.to_le_bytes());
        CommitmentRollup {
            thread_id: thread_id.to_string(),
            genesis_root: genesis.to_string(),
            final_state_root: final_state.to_string(),
            operation_count: ops,
            commitment_hex: hex::encode(ctx.finish()),
        }
    }

    #[test]
    fn verify_rollup_valid_tampered_and_garbage() {
        let valid_json = serde_json::to_string(&valid_rollup("t1", 3)).unwrap();
        let out: serde_json::Value =
            serde_json::from_str(&zk_verify_rollup(valid_json).unwrap()).unwrap();
        assert_eq!(out["verified"], true);

        let mut tampered = valid_rollup("t2", 2);
        tampered.commitment_hex = hex::encode([0u8; 32]);
        let tampered_json = serde_json::to_string(&tampered).unwrap();
        let mismatch: serde_json::Value =
            serde_json::from_str(&zk_verify_rollup(tampered_json).unwrap()).unwrap();
        assert_eq!(mismatch["verified"], false);
        assert_eq!(
            mismatch["error_msg"].as_str().unwrap(),
            "Rollup commitment mismatch"
        );

        let garbage: serde_json::Value =
            serde_json::from_str(&zk_verify_rollup("not json".to_string()).unwrap()).unwrap();
        assert_eq!(garbage["verified"], false);
        assert!(garbage["error_msg"]
            .as_str()
            .unwrap()
            .contains("Invalid rollup JSON"));
    }

    #[test]
    fn apply_rollup_valid_tampered_and_garbage() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let path = tmp_db_path("apply");
        let valid_json = serde_json::to_string(&valid_rollup("a", 7)).unwrap();
        assert!(zk_apply_rollup(path.clone(), valid_json).unwrap());

        let mut tampered = valid_rollup("b", 1);
        tampered.commitment_hex = hex::encode([0u8; 32]);
        let tampered_json = serde_json::to_string(&tampered).unwrap();
        assert_eq!(
            zk_apply_rollup(path.clone(), tampered_json).unwrap_err(),
            "Rollup commitment mismatch"
        );

        assert!(zk_apply_rollup(path, "garbage".to_string())
            .unwrap_err()
            .contains("Invalid ZK rollup JSON"));
    }

    #[test]
    fn apply_rollup_oversized_rejected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let path = tmp_db_path("oversized");
        let huge = " ".repeat(MAX_ROLLUP_JSON_BYTES + 1);
        let err = zk_apply_rollup(path, huge).unwrap_err();
        assert!(err.contains("exceeds 1MB cap"));
    }
}
