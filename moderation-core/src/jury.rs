//! Decentralized Moderation Jury Engine powered by FROST threshold signatures.
//! Manages jury assignments, voting rounds, and threshold moderation action generation.

use serde::{Deserialize, Serialize};
use soshal_crypto_core::frost::{
    FrostSessionManager, FrostSignatureShare, FrostThresholdSignature,
};

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
    pub fn cast_vote(&mut self, vote_share: FrostSignatureShare) -> Result<bool, String> {
        if self
            .votes_collected
            .iter()
            .any(|v| v.participant_id == vote_share.participant_id)
        {
            return Err("juror has already voted".to_string());
        }
        self.votes_collected.push(vote_share);
        Ok(self.votes_collected.len() >= self.threshold as usize)
    }

    /// Finalize the jury verdict into a single network-wide threshold Schnorr signature event.
    pub fn finalize_verdict(&self) -> Result<FrostThresholdSignature, String> {
        let action_payload = format!(
            "MODERATION_ACTION:case={}:target={}:reason={}",
            self.case_id, self.target_pubkey, self.reason
        );

        FrostSessionManager::aggregate_signature(
            &self.votes_collected,
            self.threshold,
            &self.group_pubkey,
            action_payload.as_bytes(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jury_moderation_flow() {
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

        let shares = FrostSessionManager::generate_jury_keys(2, 3, group_pk);
        let msg = "MODERATION_ACTION:case=case_001:target=spammer_pubkey:reason=Spam behavior";

        let vote1 = FrostSessionManager::sign_share(&shares[0], msg.as_bytes()).unwrap();
        let vote2 = FrostSessionManager::sign_share(&shares[1], msg.as_bytes()).unwrap();

        assert!(!case.cast_vote(vote1).unwrap());
        assert!(case.cast_vote(vote2).unwrap()); // Reached threshold (2 of 3)

        let verdict = case.finalize_verdict().unwrap();
        assert_eq!(verdict.group_pubkey_hex, group_pk);
    }

    #[test]
    fn test_jury_rejects_duplicate_juror_vote() {
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

        let shares = FrostSessionManager::generate_jury_keys(2, 3, group_pk);
        let msg = "MODERATION_ACTION:case=case_002:target=spammer_pubkey:reason=Spam behavior";
        let vote1 = FrostSessionManager::sign_share(&shares[0], msg.as_bytes()).unwrap();

        assert!(!case.cast_vote(vote1.clone()).unwrap());
        let err = case.cast_vote(vote1.clone()).unwrap_err();
        assert!(err.contains("already voted"));
        assert!(!case.cast_vote(vote1).is_ok());
        assert_eq!(case.votes_collected.len(), 1);
    }

    #[test]
    fn test_jury_finalize_rejects_insufficient_shares() {
        let group_pk = "group_pubkey_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let mut case = ModerationJuryCase::new(
            "case_003".to_string(),
            "spammer_pubkey".to_string(),
            None,
            "Spam behavior".to_string(),
            3,
            3,
            group_pk.to_string(),
        );

        let shares = FrostSessionManager::generate_jury_keys(3, 3, group_pk);
        let msg = "MODERATION_ACTION:case=case_003:target=spammer_pubkey:reason=Spam behavior";

        for share in shares.iter().take(2) {
            let vote = FrostSessionManager::sign_share(share, msg.as_bytes()).unwrap();
            assert!(!case.cast_vote(vote).unwrap());
        }

        let err = case.finalize_verdict().unwrap_err();
        assert!(err.contains("insufficient signature shares"));
        assert_eq!(case.votes_collected.len(), 2);
    }
}
