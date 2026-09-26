use frost::keys::{
    generate_with_dealer, IdentifierList, KeyPackage, PublicKeyPackage, SecretShare,
};
use frost::round1::SigningCommitments;
use frost::round2::SignatureShare;
use frost::{Identifier, SigningPackage};
use frost_secp256k1 as frost;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// FROST Key Package for a jury participant holding a key share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostKeyShare {
    pub participant_id: u32,
    pub threshold: u32,
    pub total_participants: u32,
    pub secret_share_hex: String,
    pub group_pubkey_hex: String,
}

/// Round 2 Partial Signature Share issued by a juror.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSignatureShare {
    pub participant_id: u32,
    pub sig_share_hex: String,
    #[serde(default)]
    pub commitments_hex: String,
}

/// Final aggregated Schnorr Signature valid under group public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostThresholdSignature {
    pub group_pubkey_hex: String,
    pub schnorr_signature_hex: String,
    pub message_hash_hex: String,
}

/// FROST Session Manager for coordinating multi-party Schnorr signatures.
pub struct FrostSessionManager;

impl FrostSessionManager {
    /// Generate real FROST key shares using trusted dealer setup for a t-of-n jury.
    pub fn generate_jury_keys(
        threshold: u32,
        total_participants: u32,
        _group_pubkey_hint: &str,
    ) -> Result<(Vec<FrostKeyShare>, String), String> {
        if threshold == 0 || threshold > total_participants {
            return Err("invalid threshold".to_string());
        }

        let rng = OsRng;
        let (shares, pubkey_package) = generate_with_dealer(
            total_participants as u16,
            threshold as u16,
            IdentifierList::Default,
            rng,
        )
        .map_err(|e| format!("dealer keygen failed: {e:?}"))?;

        let group_pk_bytes = pubkey_package
            .verifying_key()
            .serialize()
            .map_err(|e| format!("verifying key serialize failed: {e:?}"))?;
        let group_pubkey_hex = hex::encode(group_pk_bytes);

        let pubkey_pkg_json = serde_json::to_string(&pubkey_package)
            .map_err(|e| format!("pubkey package serialize failed: {e}"))?;

        let mut key_shares = Vec::new();
        for (idx, (_id, share)) in shares.into_iter().enumerate() {
            let share_json = serde_json::to_string(&share)
                .map_err(|e| format!("serializing secret share failed: {e}"))?;
            key_shares.push(FrostKeyShare {
                participant_id: (idx + 1) as u32,
                threshold,
                total_participants,
                secret_share_hex: share_json,
                group_pubkey_hex: pubkey_pkg_json.clone(),
            });
        }

        Ok((key_shares, group_pubkey_hex))
    }

    /// Round 1: Juror generates nonces and public commitments.
    pub fn create_round1_commitment(key_share: &FrostKeyShare) -> Result<(String, String), String> {
        let secret_share: SecretShare = serde_json::from_str(&key_share.secret_share_hex)
            .map_err(|e| format!("deserializing secret share failed: {e}"))?;
        let key_package = KeyPackage::try_from(secret_share)
            .map_err(|e| format!("converting key package failed: {e:?}"))?;

        let mut rng = OsRng;
        let (nonces, commitments) = frost::round1::commit(key_package.signing_share(), &mut rng);
        let nonces_json = serde_json::to_string(&nonces).map_err(|e| e.to_string())?;
        let commitments_json = serde_json::to_string(&commitments).map_err(|e| e.to_string())?;
        Ok((nonces_json, commitments_json))
    }

    /// Coordinator packages gathered commitments into a SigningPackage JSON.
    pub fn create_signing_package(
        commitments: &[(u32, String)],
        message_bytes: &[u8],
    ) -> Result<String, String> {
        let mut map = BTreeMap::new();
        for (id_num, comm_json) in commitments {
            let id = Identifier::try_from((*id_num as u16).max(1))
                .map_err(|e| format!("invalid id: {e:?}"))?;
            let comm: SigningCommitments = serde_json::from_str(comm_json)
                .map_err(|e| format!("invalid commitment json: {e}"))?;
            map.insert(id, comm);
        }
        let pkg = SigningPackage::new(map, message_bytes);
        serde_json::to_string(&pkg).map_err(|e| e.to_string())
    }

    /// Round 2: Juror signs the signing package using their nonces.
    pub fn sign_share(
        key_share: &FrostKeyShare,
        nonces_json: &str,
        signing_package_json: &str,
    ) -> Result<FrostSignatureShare, String> {
        let secret_share: SecretShare = serde_json::from_str(&key_share.secret_share_hex)
            .map_err(|e| format!("deserializing secret share failed: {e}"))?;
        let key_package = KeyPackage::try_from(secret_share)
            .map_err(|e| format!("converting key package failed: {e:?}"))?;
        let nonces: frost::round1::SigningNonces = serde_json::from_str(nonces_json)
            .map_err(|e| format!("deserializing nonces failed: {e}"))?;
        let signing_package: SigningPackage = serde_json::from_str(signing_package_json)
            .map_err(|e| format!("deserializing signing package failed: {e}"))?;

        let sig_share = frost::round2::sign(&signing_package, &nonces, &key_package)
            .map_err(|e| format!("signing share failed: {e:?}"))?;
        let sig_share_json = serde_json::to_string(&sig_share)
            .map_err(|e| format!("serializing sig share failed: {e}"))?;

        Ok(FrostSignatureShare {
            participant_id: key_share.participant_id,
            sig_share_hex: sig_share_json,
            commitments_hex: String::new(),
        })
    }

    /// Aggregate partial signature shares into a valid single Schnorr threshold signature.
    pub fn aggregate_signature(
        shares: &[FrostSignatureShare],
        threshold: u32,
        signing_package_json: &str,
        group_pubkey_or_package: &str,
    ) -> Result<FrostThresholdSignature, String> {
        if shares.len() < threshold as usize {
            return Err(format!(
                "insufficient signature shares: got {}, required threshold {}",
                shares.len(),
                threshold
            ));
        }

        let signing_package: SigningPackage = serde_json::from_str(signing_package_json)
            .map_err(|e| format!("invalid signing package json: {e}"))?;

        let mut sig_shares = BTreeMap::new();
        for share in shares.iter().take(threshold as usize) {
            let id = Identifier::try_from((share.participant_id as u16).max(1))
                .map_err(|e| format!("invalid participant identifier: {e:?}"))?;
            let sig_share: SignatureShare = serde_json::from_str(&share.sig_share_hex)
                .map_err(|e| format!("invalid sig share json: {e}"))?;
            sig_shares.insert(id, sig_share);
        }

        let pubkey_package: PublicKeyPackage =
            if let Ok(pkg) = serde_json::from_str(group_pubkey_or_package) {
                pkg
            } else {
                let group_pk_bytes = hex::decode(group_pubkey_or_package)
                    .map_err(|e| format!("invalid group pubkey hex: {e}"))?;
                let verifying_key = frost::VerifyingKey::deserialize(&group_pk_bytes)
                    .map_err(|e| format!("invalid verifying key deserialize: {e:?}"))?;
                PublicKeyPackage::new(BTreeMap::new(), verifying_key, Some(threshold as u16))
            };

        let signature = frost::aggregate(&signing_package, &sig_shares, &pubkey_package)
            .map_err(|e| format!("aggregation failed: {e:?}"))?;

        let sig_bytes = signature
            .serialize()
            .map_err(|e| format!("sig serialize failed: {e:?}"))?;
        let msg_digest = sha2::Sha256::digest(signing_package.message());
        let group_pk_bytes = pubkey_package
            .verifying_key()
            .serialize()
            .unwrap_or_default();

        Ok(FrostThresholdSignature {
            group_pubkey_hex: hex::encode(group_pk_bytes),
            schnorr_signature_hex: hex::encode(sig_bytes),
            message_hash_hex: hex::encode(msg_digest),
        })
    }
}

use sha2::Digest;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frost_keygen_and_threshold_signing() {
        let (shares, group_pk) = FrostSessionManager::generate_jury_keys(3, 5, "").unwrap();
        assert_eq!(shares.len(), 5);
        assert!(!group_pk.is_empty());

        let message = b"Ban spammer npub_123";

        // Round 1: Commitments
        let mut nonces = Vec::new();
        let mut commitments = Vec::new();
        for share in &shares[0..3] {
            let (n, c) = FrostSessionManager::create_round1_commitment(share).unwrap();
            nonces.push(n);
            commitments.push((share.participant_id, c));
        }

        // Coordinator creates signing package
        let pkg_json = FrostSessionManager::create_signing_package(&commitments, message).unwrap();

        // Round 2: Signing
        let mut sig_shares = Vec::new();
        for i in 0..3 {
            let sig_share =
                FrostSessionManager::sign_share(&shares[i], &nonces[i], &pkg_json).unwrap();
            sig_shares.push(sig_share);
        }

        // Coordinator aggregates signature
        let agg = FrostSessionManager::aggregate_signature(
            &sig_shares,
            3,
            &pkg_json,
            &shares[0].group_pubkey_hex,
        )
        .unwrap();

        assert_eq!(agg.group_pubkey_hex, group_pk);
        assert!(!agg.schnorr_signature_hex.is_empty());

        let group_pk_bytes = hex::decode(&agg.group_pubkey_hex).unwrap();
        let verifying_key = frost::VerifyingKey::deserialize(&group_pk_bytes).unwrap();
        let sig_bytes = hex::decode(&agg.schnorr_signature_hex).unwrap();
        let sig = frost::Signature::deserialize(&sig_bytes).unwrap();
        assert!(verifying_key.verify(message, &sig).is_ok());
    }
}
