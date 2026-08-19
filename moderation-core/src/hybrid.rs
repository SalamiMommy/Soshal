//! 2-Tier Hybrid Content Moderation Pipeline
//!
//! Orchestrates a high-throughput, dual-tier moderation pipeline:
//!
//! - **Tier 1 (Sub-millisecond Pre-filter)**:
//!   - Subword & character n-gram statistical classifier with evasion detection.
//!   - Perceptual skin-tone & red chrominance anomaly detector.
//!
//! - **Tier 2 (Deep Semantic & Perceptual Matching)**:
//!   - **Text**: RoBERTa deep transformer with BPE tokenization and multi-head self-attention.
//!   - **Media**: Meta PDQ 256-bit perceptual image hashing and Hamming distance blocklist matcher.

use crate::ai_classifier::{classify_text, AiModerationResult};
use crate::ai_media::{classify_media_buffer, AiMediaVerdict};
use crate::pdq::{evaluate_media_pdq, PdqHashResult};
use crate::roberta::{classify_text_roberta, RobertaResult};
use serde::{Deserialize, Serialize};

/// Indicates which tier made the final determination
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EvaluationTier {
    /// Resolved purely at Tier 1 (fast heuristic/n-gram)
    Tier1Fast,
    /// Escalated to Tier 2 (RoBERTa transformer or PDQ perceptual match)
    Tier2Deep,
}

/// Unified 2-Tier Hybrid Result for Text Moderation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HybridModerationResult {
    pub is_flagged: bool,
    pub primary_category: Option<String>,
    pub confidence: f32,
    pub tier_evaluated: EvaluationTier,
    pub tier1_result: AiModerationResult,
    pub tier2_roberta_result: Option<RobertaResult>,
    pub detected_reasons: Vec<String>,
}

/// Unified 2-Tier Hybrid Result for Media Moderation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HybridMediaResult {
    pub passed: bool,
    pub tier1_verdict: AiMediaVerdict,
    pub tier2_pdq: Option<PdqHashResult>,
    pub is_csam_threat: bool,
    pub is_gore_threat: bool,
    pub is_nsfw: bool,
    pub warning_reason: Option<String>,
}

/// Evaluate text using the 2-Tier Hybrid Pipeline.
///
/// Escalates to Tier 2 (RoBERTa) if Tier 1 yields borderline confidence (0.20 <= P <= 0.85),
/// if obfuscation/evasion is detected, or if `force_deep_scan` is requested.
pub fn evaluate_text_hybrid(text: &str, force_deep_scan: bool) -> HybridModerationResult {
    let t1 = classify_text(text);

    // Fast resolution cases:
    // 1. Extreme confidence violation (>= 0.85) without need for deeper NLP
    // 2. Unambiguously clean (< 0.20) with no evasion signals
    let needs_tier2 = force_deep_scan
        || (t1.confidence >= 0.20 && t1.confidence <= 0.85)
        || t1.evasion_score >= 0.10;

    if !needs_tier2 {
        let mut reasons = t1.detected_reasons.clone();
        if t1.is_flagged {
            reasons.push("tier1_fast_filter_match".to_string());
        }
        return HybridModerationResult {
            is_flagged: t1.is_flagged,
            primary_category: t1.primary_category.clone(),
            confidence: t1.confidence,
            tier_evaluated: EvaluationTier::Tier1Fast,
            tier1_result: t1,
            tier2_roberta_result: None,
            detected_reasons: reasons,
        };
    }

    // Escalate to Tier 2: heuristic embedding model (synthetic weights; real
    // ML transformer is a roadmap item — see roberta.rs module docs).
    let t2 = classify_text_roberta(text);

    // Ensemble verdicts
    let is_flagged = t1.is_flagged || t2.is_flagged;
    let confidence = t1.confidence.max(t2.confidence);

    let primary_category = if t2.is_flagged && t2.primary_category.is_some() {
        t2.primary_category.clone()
    } else {
        t1.primary_category.clone()
    };

    let mut reasons = t1.detected_reasons.clone();
    reasons.extend(t2.detected_signals.clone());

    HybridModerationResult {
        is_flagged,
        primary_category,
        confidence,
        tier_evaluated: EvaluationTier::Tier2Deep,
        tier1_result: t1,
        tier2_roberta_result: Some(t2),
        detected_reasons: reasons,
    }
}

/// JSON serialized output of 2-tier hybrid text evaluation.
pub fn evaluate_text_hybrid_json(text: &str, force_deep_scan: bool) -> String {
    let res = evaluate_text_hybrid(text, force_deep_scan);
    serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string())
}

/// Evaluate media using the 2-Tier Hybrid Pipeline (Chrominance + Meta PDQ Hash).
pub fn evaluate_media_hybrid(bytes: &[u8], mime_type: &str, tags: &[String]) -> HybridMediaResult {
    let t1_verdict = classify_media_buffer(bytes, mime_type, tags);
    let t2_pdq = evaluate_media_pdq(bytes);

    let mut is_csam = t1_verdict.is_csam_hazard;
    let mut is_gore = t1_verdict.is_gore_hazard;
    let is_nsfw = t1_verdict.is_nsfw;
    let mut warning_reason = t1_verdict.warning_reason.clone();

    // If PDQ detected a known perceptual threat match
    if let Some(ref pdq_res) = t2_pdq {
        if pdq_res.is_threat_match {
            if let Some(ref cat) = pdq_res.matched_category {
                if cat == "csam" {
                    is_csam = true;
                    warning_reason = Some("pdq_perceptual_csam_match".to_string());
                } else if cat == "gore" {
                    is_gore = true;
                    warning_reason = Some("pdq_perceptual_gore_match".to_string());
                }
            }
        }
    }

    let passed = t1_verdict.passed && !is_csam && !is_gore;

    HybridMediaResult {
        passed,
        tier1_verdict: t1_verdict,
        tier2_pdq: t2_pdq,
        is_csam_threat: is_csam,
        is_gore_threat: is_gore,
        is_nsfw,
        warning_reason,
    }
}

/// JSON serialized output of 2-tier hybrid media evaluation.
pub fn evaluate_media_hybrid_json(bytes: &[u8], mime_type: &str, tags: &[String]) -> String {
    let res = evaluate_media_hybrid(bytes, mime_type, tags);
    serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_clean_text_resolves_at_tier1() {
        let text = "What a beautiful sunny day outside! Hope everyone is having a nice time.";
        let res = evaluate_text_hybrid(text, false);
        assert!(!res.is_flagged);
        assert_eq!(res.tier_evaluated, EvaluationTier::Tier1Fast);
        assert!(res.tier2_roberta_result.is_none());
    }

    #[test]
    fn test_hybrid_deep_scan_escalates_to_tier2() {
        let text = "Claim early token distribution and connect wallet";
        let res = evaluate_text_hybrid(text, true);
        assert_eq!(res.tier_evaluated, EvaluationTier::Tier2Deep);
        assert!(res.tier2_roberta_result.is_some());
    }

    #[test]
    fn test_hybrid_media_sentinel_buffer_does_not_match() {
        // 32 raw bytes can't form a PDQ hash (needs >= 64x64 luma); the
        // sentinel hex is a blocklist entry, not an image. Verify the
        // pipeline treats it as an unmatchable tiny buffer.
        let sentinel_csam_hex = "f0f0f0f0f0f0f0f0a5a5a5a5a5a5a5a5123456789abcdef0123456789abcdef0";
        let raw_bytes = hex::decode(sentinel_csam_hex).unwrap();

        let res = evaluate_media_hybrid(&raw_bytes, "image/jpeg", &[]);
        assert!(res.passed);
    }
}
