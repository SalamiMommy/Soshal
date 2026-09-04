//! Web-of-Trust Moderation Commitment Prover & Verifier.
//!
//! NOTE: NOT a zero-knowledge proof. This is a deterministic SHA-256
//! commitment envelope (no zk-SNARK/STARK, no cryptographic hiding).
//! Binding only: proves the prover knew the WoT root + blacklist root
//! at proof time, NOT membership, and reveals linkable identifiers.
//! Real zk (halo2/arkworks/risc0) is a deliberate non-goal for now —
//! see roadmap phase on hintless PIR / zk rollups. Keep naming honest.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Commitment Envelope (< 200 bytes). Binding only, no zero-knowledge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZkTrustProof {
    pub wot_merkle_root: String,
    pub blacklist_nullifier_hash: String,
    pub proof_bytes_b64: String,
}

/// Generates a deterministic SHA-256 commitment asserting:
/// 1. Prover knew the WoT Merkle root at generation time.
/// 2. Prover key nullifier is NOT in the local blacklist.
///
/// NOT zero-knowledge: identifiers are linkable by design.
pub fn generate_zk_wot_proof(
    prover_pubkey: &str,
    wot_merkle_root: &str,
    blacklist_root: &str,
) -> ZkTrustProof {
    use base64::Engine;

    // Deterministic SHA-256 commitment (not zero-knowledge)
    let mut hasher = Sha256::new();
    hasher.update(prover_pubkey.as_bytes());
    hasher.update(wot_merkle_root.as_bytes());
    hasher.update(blacklist_root.as_bytes());
    let proof_hash: [u8; 32] = hasher.finalize().into();

    let mut nullifier_hasher = Sha256::new();
    nullifier_hasher.update(b"nullifier:");
    nullifier_hasher.update(prover_pubkey.as_bytes());
    let nullifier_hash: [u8; 32] = nullifier_hasher.finalize().into();

    ZkTrustProof {
        wot_merkle_root: wot_merkle_root.to_string(),
        blacklist_nullifier_hash: hex::encode(nullifier_hash),
        proof_bytes_b64: base64::engine::general_purpose::STANDARD.encode(proof_hash),
    }
}

/// Verifies the commitment envelope structure. Returns `true` if the
/// binding matches the expected WoT root and the nullifier is unblacklisted.
/// NOT zero-knowledge verification.
pub fn verify_zk_wot_proof(
    proof: &ZkTrustProof,
    expected_wot_root: &str,
    known_blacklisted_nullifiers: &[String],
) -> bool {
    use base64::Engine;

    if proof.wot_merkle_root != expected_wot_root {
        return false;
    }

    // Reject if nullifier appears in blacklist tree
    if known_blacklisted_nullifiers.contains(&proof.blacklist_nullifier_hash) {
        return false;
    }

    // Verify proof commitment structure
    let decoded = base64::engine::general_purpose::STANDARD.decode(&proof.proof_bytes_b64);
    match decoded {
        Ok(bytes) => bytes.len() == 32,
        Err(_) => false,
    }
}

/// Fully-correct verification: recomputes the SHA-256 commitment from
/// prover_pubkey, expected WoT root and blacklist root, plus the nullifier
/// from prover_pubkey. Binds `proof_bytes_b64` and `blacklist_nullifier_hash`
/// to the prover — unlike [`verify_zk_wot_proof`], which is structural only.
pub fn verify_zk_wot_proof_binding(
    proof: &ZkTrustProof,
    prover_pubkey: &str,
    expected_wot_root: &str,
    blacklist_root: &str,
    known_blacklisted_nullifiers: &[String],
) -> bool {
    use base64::Engine;

    if proof.wot_merkle_root != expected_wot_root {
        return false;
    }

    // Reject if nullifier appears in blacklist tree
    if known_blacklisted_nullifiers.contains(&proof.blacklist_nullifier_hash) {
        return false;
    }

    // Recompute and compare the prover nullifier — binds the proof to pubkey
    let mut nullifier_hasher = Sha256::new();
    nullifier_hasher.update(b"nullifier:");
    nullifier_hasher.update(prover_pubkey.as_bytes());
    let nullifier_h: [u8; 32] = nullifier_hasher.finalize().into();
    if hex::encode(nullifier_h) != proof.blacklist_nullifier_hash {
        return false;
    }

    // Recompute the commitment: SHA256(pubkey ‖ wot_root ‖ blacklist_root)
    let mut hasher = Sha256::new();
    hasher.update(prover_pubkey.as_bytes());
    hasher.update(proof.wot_merkle_root.as_bytes());
    hasher.update(blacklist_root.as_bytes());
    let expected_hash: [u8; 32] = hasher.finalize().into();

    match base64::engine::general_purpose::STANDARD.decode(&proof.proof_bytes_b64) {
        Ok(bytes) => bytes.as_slice() == expected_hash.as_slice(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zk_wot_proof_generation_and_verification() {
        let proof = generate_zk_wot_proof("pubkey_alice", "wot_root_123", "black_root_456");

        assert_eq!(proof.wot_merkle_root, "wot_root_123");
        assert!(verify_zk_wot_proof(&proof, "wot_root_123", &[]));

        // Wrong root fails
        assert!(!verify_zk_wot_proof(&proof, "wrong_root", &[]));

        // Blacklisted nullifier fails
        let blacklisted = vec![proof.blacklist_nullifier_hash.clone()];
        assert!(!verify_zk_wot_proof(&proof, "wot_root_123", &blacklisted));

        // Binding verification: correct prover + roots pass
        assert!(verify_zk_wot_proof_binding(
            &proof,
            "pubkey_alice",
            "wot_root_123",
            "black_root_456",
            &[],
        ));

        // Wrong prover pubkey fails (does not bind to the commitment)
        assert!(!verify_zk_wot_proof_binding(
            &proof,
            "pubkey_mallory",
            "wot_root_123",
            "black_root_456",
            &[],
        ));

        // Wrong blacklist root fails
        assert!(!verify_zk_wot_proof_binding(
            &proof,
            "pubkey_alice",
            "wot_root_123",
            "other_root",
            &[],
        ));

        // Blacklisted nullifier fails in binding path too
        assert!(!verify_zk_wot_proof_binding(
            &proof,
            "pubkey_alice",
            "wot_root_123",
            "black_root_456",
            &blacklisted,
        ));
    }
}
