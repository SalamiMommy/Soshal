//! Decentralized Moderation Jury Engine powered by FROST threshold signatures.
//! Manages jury assignments, voting rounds, and threshold moderation action generation.

use serde::{Deserialize, Serialize};
use soshal_crypto_core::frost::{FrostSignatureShare, FrostThresholdSignature};

/// Hard cap on collected votes to bound growth from spoofed participants.
const MAX_VOTES_COLLECTED: usize = 100_000;

/// State of a decentralized moderation jury case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationJuryCase {
    pub case_id: String,
    pub target_pubkey: String,
    pub target_post_id: Option<String>,
    pub reason: String,
    pub threshold: u32,
    pub total_jurors: u32,
    pub group_pubkey: String,
    pub votes_collected: Vec<FrostSignatureShare>,
}

impl ModerationJuryCase {
    pub fn new(
        case_id: String,
        target_pubkey: String,
        target_post_id: Option<String>,
        reason: String,
        threshold: u32,
        total_jurors: u32,
        group_pubkey: String,
    ) -> Self {
        Self {
            case_id,
            target_pubkey,
            target_post_id,
            reason,
            threshold,
            total_jurors,
            group_pubkey,
            votes_collected: Vec::new(),
        }
    }

    /// Submit a juror's FROST partial signature vote share on this case.
    pub fn cast_vote(&mut self, _vote_share: FrostSignatureShare) -> Result<bool, String> {
        if self.votes_collected.len() >= MAX_VOTES_COLLECTED {
            return Err("vote cap reached".to_string());
        }
        // Honest gate: real FROST is bridge-disabled ("non-cryptographic
        // simulation, disabled"); no share-verification path exists, so a
        // fabricated participant_id + sig hex cannot be told apart from a real
        // juror vote. Accepting one would block legitimate votes and stuff the
        // ballot, so voting is unavailable until real FROST verification lands.
        Err(
            "jury voting unavailable (roadmap): FROST share verification not implemented"
                .to_string(),
        )
    }

    /// Finalize the jury verdict into a single network-wide threshold Schnorr signature event.
    pub fn finalize_verdict(&self) -> Result<FrostThresholdSignature, String> {
        // Honest gate: aggregation is a forgeable string-hash simulation, not
        // a verifiable threshold Schnorr signature. Emitting it as Ok would
        // present forged verdicts as cryptographically valid.
        Err(
            "jury threshold signature unavailable (roadmap): non-cryptographic simulation, disabled"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forged_share(participant_id: u32) -> FrostSignatureShare {
        FrostSignatureShare {
            participant_id,
            sig_share_hex: "deadbeef_forged".to_string(),
        }
    }

    #[test]
    fn test_jury_voting_unavailable_roadmap() {
        let group_pk = "group_pubkey_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let mut case = ModerationJuryCase::new(
            "case_001".to_string(),
            "spammer_pubkey".to_string(),
            None,
            "Spam behavior".to_string(),
            2,
            3,
            group_pk.to_string(),
        );

        let err = case.cast_vote(forged_share(1)).unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
        assert!(case.votes_collected.is_empty());

        let err = case.finalize_verdict().unwrap_err();
        assert!(err.contains("simulation"), "err: {err}");
    }

    #[test]
    fn test_jury_rejects_forged_share() {
        let group_pk = "group_pubkey_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let mut case = ModerationJuryCase::new(
            "case_002".to_string(),
            "spammer_pubkey".to_string(),
            None,
            "Spam behavior".to_string(),
            2,
            3,
            group_pk.to_string(),
        );

        // Fabricated participant_id + sig hex: rejected, nothing stored.
        let err = case.cast_vote(forged_share(999)).unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
        assert!(case.votes_collected.is_empty());
    }

    #[test]
    fn test_jury_finalize_rejects_unverifiable_threshold_signature() {
        let group_pk = "group_pubkey_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let case = ModerationJuryCase::new(
            "case_003".to_string(),
            "spammer_pubkey".to_string(),
            None,
            "Spam behavior".to_string(),
            3,
            3,
            group_pk.to_string(),
        );

        let err = case.finalize_verdict().unwrap_err();
        assert!(err.contains("simulation"), "err: {err}");
        assert!(case.votes_collected.is_empty());
    }
}
