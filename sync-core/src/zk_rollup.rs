//! CRDT State Rollup with Merkle commitment verification.
//!
//! NOTE: NOT a zero-knowledge system. Verification is a plain SHA-256
//! commitment check (binding only — no zk-SNARK/STARK proof, no hiding).
//! `ZkProofType` variants are retained for serde wire-compat only; real
//! ZK provers (risc0/sp1) are roadmap items, not implemented here.
//! Name kept for API stability; semantics are honest commitments.

use libsql::{params, Connection};
use serde::{Deserialize, Serialize};
use soshal_db_core::block_on;
use std::sync::OnceLock;

/// Rollup commitment scheme tag (serde wire-compat only — no real ZK).
/// Every tag verifies identically as a SHA-256 commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZkProofType {
    RiscZeroStark,
    Sp1Stark,
    Groth16,
}

/// CRDT State Rollup Payload (SHA-256 commitment, not ZK)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkCrdtRollup {
    pub thread_id: String,
    pub genesis_root: String,
    pub final_state_root: String,
    pub operation_count: u64,
    pub proof_bytes_hex: String,
    pub proof_type: ZkProofType,
}

/// Verification Result details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkVerificationResult {
    pub verified: bool,
    pub thread_id: String,
    pub latency_ms: u64,
    pub verified_operations: u64,
    pub error_msg: Option<String>,
}

/// State Rollup Engine (SHA-256 commitment verification)
#[derive(Debug, Default)]
pub struct ZkRollupEngine;

impl ZkRollupEngine {
    pub fn new() -> Self {
        Self
    }

    /// Verify a state rollup commitment in milliseconds.
    /// Structural check: Hash(thread_id || genesis_root || final_state_root || op_count)
    /// equals the first 32 bytes of the proof payload. No cryptographic proof.
    pub fn verify_rollup(&self, rollup: &ZkCrdtRollup) -> ZkVerificationResult {
        let start_time = std::time::Instant::now();

        // Decode proof bytes from hex
        let proof_bytes = match hex::decode(&rollup.proof_bytes_hex) {
            Ok(bytes) => bytes,
            Err(e) => {
                return ZkVerificationResult {
                    verified: false,
                    thread_id: rollup.thread_id.clone(),
                    latency_ms: start_time.elapsed().as_millis() as u64,
                    verified_operations: 0,
                    error_msg: Some(format!("Invalid proof hex: {}", e)),
                };
            }
        };

        if proof_bytes.is_empty() {
            return ZkVerificationResult {
                verified: false,
                thread_id: rollup.thread_id.clone(),
                latency_ms: start_time.elapsed().as_millis() as u64,
                verified_operations: 0,
                error_msg: Some("Proof payload is empty".to_string()),
            };
        }

        // Verify commitment binding:
        // Hash(genesis_root || final_state_root || operation_count) must match proof tag
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(rollup.thread_id.as_bytes());
        hasher.update(rollup.genesis_root.as_bytes());
        hasher.update(rollup.final_state_root.as_bytes());
        hasher.update(rollup.operation_count.to_le_bytes());
        let expected_digest = hasher.finalize();

        // Commitment check: first 32 bytes must contain the expected digest
        let is_valid = proof_bytes.len() >= 32 && proof_bytes[0..32] == expected_digest[..];

        let latency_ms = start_time.elapsed().as_millis() as u64;

        ZkVerificationResult {
            verified: is_valid,
            thread_id: rollup.thread_id.clone(),
            latency_ms,
            verified_operations: if is_valid { rollup.operation_count } else { 0 },
            error_msg: if is_valid {
                None
            } else {
                Some("Rollup commitment mismatch".to_string())
            },
        }
    }

    /// Apply a verified rollup directly into SQLite database, bypassing historical delta replay
    pub fn apply_rollup_to_db(
        &self,
        conn: &Connection,
        rollup: &ZkCrdtRollup,
    ) -> Result<bool, String> {
        let verification = self.verify_rollup(rollup);
        if !verification.verified {
            return Err(verification
                .error_msg
                .unwrap_or_else(|| "Verification failed".to_string()));
        }

        block_on(async {
            let now = soshal_common_core::format::now_secs();

            if rollup.operation_count > i64::MAX as u64 {
                return Err("operation count exceeds i64 range".to_string());
            }

            // Anchor check: a rollup for a thread must agree with the
            // stored genesis and never regress the operation count —
            // otherwise a forged self-consistent commitment (no anchor to
            // prior state) could overwrite or downgrade thread state.
            let existing: Option<(String, i64)> = {
                let mut stmt = conn
                    .prepare(
                        "SELECT genesis_root, operation_count FROM zk_state_rollups WHERE thread_id = ?1",
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt
                    .query(params![rollup.thread_id.as_str()])
                    .await
                    .map_err(|e| e.to_string())?;
                match rows.next().await.map_err(|e| e.to_string())? {
                    Some(row) => Some((
                        row.get(0).map_err(|e| e.to_string())?,
                        row.get(1).map_err(|e| e.to_string())?,
                    )),
                    None => None,
                }
            };
            if let Some((stored_genesis, stored_ops)) = existing {
                if stored_genesis != rollup.genesis_root {
                    return Err(format!(
                        "rollup genesis mismatch for {}: stored {} vs proposed {}",
                        rollup.thread_id, stored_genesis, rollup.genesis_root
                    ));
                }
                if rollup.operation_count < stored_ops as u64 {
                    return Err(format!(
                        "rollup operation count regression for {}: stored {stored_ops} vs proposed {}",
                        rollup.thread_id, rollup.operation_count
                    ));
                }
            }

            conn.execute(
                "INSERT INTO zk_state_rollups (thread_id, genesis_root, final_state_root, operation_count, verified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(thread_id) DO UPDATE SET
                    final_state_root = excluded.final_state_root,
                    operation_count = excluded.operation_count,
                    verified_at = excluded.verified_at",
                params![
                    rollup.thread_id.as_str(),
                    rollup.genesis_root.as_str(),
                    rollup.final_state_root.as_str(),
                    rollup.operation_count as i64,
                    now
                ],
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(true)
        })
    }
}

static GLOBAL_ZK_ENGINE: OnceLock<ZkRollupEngine> = OnceLock::new();

pub fn get_global_zk_engine() -> &'static ZkRollupEngine {
    GLOBAL_ZK_ENGINE.get_or_init(ZkRollupEngine::new)
}

/// FFI helper function to verify a rollup commitment JSON string
pub fn verify_zk_rollup_json(rollup_json: &str) -> String {
    let rollup: ZkCrdtRollup = match serde_json::from_str(rollup_json) {
        Ok(r) => r,
        Err(e) => {
            let err_res = ZkVerificationResult {
                verified: false,
                thread_id: String::new(),
                latency_ms: 0,
                verified_operations: 0,
                error_msg: Some(format!("Invalid rollup JSON: {}", e)),
            };
            return serde_json::to_string(&err_res).unwrap_or_default();
        }
    };

    let result = get_global_zk_engine().verify_rollup(&rollup);
    serde_json::to_string(&result).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_db_core::query::query_first;

    fn valid_rollup(thread_id: &str, ops: u64) -> ZkCrdtRollup {
        let genesis = "0000000000000000000000000000000000000000000000000000000000000000";
        let final_state = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let mut input = Vec::new();
        input.extend_from_slice(thread_id.as_bytes());
        input.extend_from_slice(genesis.as_bytes());
        input.extend_from_slice(final_state.as_bytes());
        input.extend_from_slice(&ops.to_le_bytes());
        ZkCrdtRollup {
            thread_id: thread_id.to_string(),
            genesis_root: genesis.to_string(),
            final_state_root: final_state.to_string(),
            operation_count: ops,
            proof_bytes_hex: soshal_crypto_core::hash::sha256_hex(&input),
            proof_type: ZkProofType::RiscZeroStark,
        }
    }

    #[test]
    fn apply_rollup_to_db_writes_row_and_upserts() {
        let db = soshal_test_util::test_db();
        let conn = db.conn().unwrap();

        let engine = ZkRollupEngine::new();
        assert!(engine
            .apply_rollup_to_db(&conn, &valid_rollup("t1", 7))
            .unwrap());

        let (root, ops): (String, i64) = query_first(
            &conn,
            "SELECT final_state_root, operation_count FROM zk_state_rollups WHERE thread_id = ?1",
            params!["t1"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            root,
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        );
        assert_eq!(ops, 7);

        assert!(engine
            .apply_rollup_to_db(&conn, &valid_rollup("t1", 9))
            .unwrap());
        let ops2: i64 = query_first(
            &conn,
            "SELECT operation_count FROM zk_state_rollups WHERE thread_id = ?1",
            params!["t1"],
            |r| r.get::<i64>(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ops2, 9);
    }

    #[test]
    fn apply_rollup_to_db_rejects_invalid_proof() {
        let db = soshal_test_util::test_db();
        let conn = db.conn().unwrap();
        let mut rollup = valid_rollup("t2", 1);
        rollup.proof_bytes_hex = "zz".to_string();
        assert!(ZkRollupEngine::new()
            .apply_rollup_to_db(&conn, &rollup)
            .is_err());
    }

    #[test]
    fn apply_rollup_to_db_rejects_genesis_and_regression() {
        let db = soshal_test_util::test_db();
        let conn = db.conn().unwrap();
        let engine = ZkRollupEngine::new();
        assert!(engine
            .apply_rollup_to_db(&conn, &valid_rollup("t6", 5))
            .unwrap());

        let mut forged_genesis = valid_rollup("t6", 6);
        forged_genesis.genesis_root =
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
        // Recompute the commitment so the rollup is self-consistent; only the
        // anchor check (genesis vs stored row) may reject it.
        {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(forged_genesis.thread_id.as_bytes());
            h.update(forged_genesis.genesis_root.as_bytes());
            h.update(forged_genesis.final_state_root.as_bytes());
            h.update(&forged_genesis.operation_count.to_le_bytes());
            forged_genesis.proof_bytes_hex = hex::encode(h.finalize());
        }
        let res = engine.apply_rollup_to_db(&conn, &forged_genesis);
        assert!(res.is_err(), "genesis mismatch must reject");
        assert!(res.unwrap_err().contains("genesis"));

        let regressed = valid_rollup("t6", 3);
        let res = engine.apply_rollup_to_db(&conn, &regressed);
        assert!(res.is_err(), "operation count regression must reject");
        assert!(res.unwrap_err().contains("regression"));
    }

    #[test]
    fn get_global_zk_engine_is_stable_singleton() {
        let a = get_global_zk_engine();
        let b = get_global_zk_engine();
        assert!(std::ptr::eq(a, b));
        assert!(a.verify_rollup(&valid_rollup("t3", 1)).verified);
    }

    #[test]
    fn verify_zk_rollup_json_valid_malformed_and_bad_proof() {
        let valid_json = serde_json::to_string(&valid_rollup("t4", 3)).unwrap();
        let out: serde_json::Value =
            serde_json::from_str(&verify_zk_rollup_json(&valid_json)).unwrap();
        assert_eq!(out["verified"], true);

        let bad: serde_json::Value =
            serde_json::from_str(&verify_zk_rollup_json("not json")).unwrap();
        assert_eq!(bad["verified"], false);
        assert!(bad["error_msg"]
            .as_str()
            .unwrap()
            .contains("Invalid rollup JSON"));

        let mut tampered = valid_rollup("t5", 2);
        tampered.proof_bytes_hex = hex::encode([0u8; 32]);
        let tampered_json = serde_json::to_string(&tampered).unwrap();
        let mismatch: serde_json::Value =
            serde_json::from_str(&verify_zk_rollup_json(&tampered_json)).unwrap();
        assert_eq!(mismatch["verified"], false);
        assert_eq!(
            mismatch["error_msg"].as_str().unwrap(),
            "Rollup commitment mismatch"
        );
    }

    #[test]
    fn test_zk_rollup_verification() {
        let thread_id = "thread_12345";
        let genesis = "0000000000000000000000000000000000000000000000000000000000000000";
        let final_state = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let ops = 500u64;

        let mut input = Vec::new();
        input.extend_from_slice(thread_id.as_bytes());
        input.extend_from_slice(genesis.as_bytes());
        input.extend_from_slice(final_state.as_bytes());
        input.extend_from_slice(&ops.to_le_bytes());
        let proof_hex = soshal_crypto_core::hash::sha256_hex(&input);

        let rollup = ZkCrdtRollup {
            thread_id: thread_id.to_string(),
            genesis_root: genesis.to_string(),
            final_state_root: final_state.to_string(),
            operation_count: ops,
            proof_bytes_hex: proof_hex,
            proof_type: ZkProofType::RiscZeroStark,
        };

        let engine = ZkRollupEngine::new();
        let res = engine.verify_rollup(&rollup);
        assert!(res.verified);
        assert_eq!(res.verified_operations, 500);
    }

    #[test]
    fn test_zk_rollup_rejects_truncated_proof() {
        let mut rollup = valid_rollup("t_short", 10);
        rollup.proof_bytes_hex = "01".to_string(); // 1 byte only
        let engine = ZkRollupEngine::new();
        let res = engine.verify_rollup(&rollup);
        assert!(!res.verified);
        assert_eq!(res.verified_operations, 0);
    }
}
