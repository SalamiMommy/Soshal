//! Lightweight AI Text Classification Engine.
//!
//! Provides on-device, zero-external-dependency machine learning classification
//! for detecting Spam, CSAM, Gore, Bigotry, and Harassment.
//!
//! ### Key Capabilities:
//! - Subword & character n-gram (3-gram to 5-gram) feature hashing.
//! - Evasion & obfuscation analysis (homoglyphs, zero-width chars, character stretching).
//! - Multi-class probability scoring matrix with calibrated log-odds.
//! - Explainable detection reasons and confidence metrics.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::nn::{IDX_BIGOTRY, IDX_CSAM, IDX_GORE, IDX_HARASSMENT, IDX_SPAM};

/// Category scores from the AI classification model, normalized to [0.0, 1.0].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiCategoryScores {
    pub spam: f32,
    pub csam: f32,
    pub gore: f32,
    pub bigotry: f32,
    pub harassment: f32,
}

impl Default for AiCategoryScores {
    fn default() -> Self {
        Self {
            spam: 0.0,
            csam: 0.0,
            gore: 0.0,
            bigotry: 0.0,
            harassment: 0.0,
        }
    }
}

/// Comprehensive verdict returned by the AI moderation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiModerationResult {
    pub is_flagged: bool,
    pub primary_category: Option<String>,
    pub confidence: f32,
    pub scores: AiCategoryScores,
    pub detected_reasons: Vec<String>,
    pub evasion_score: f32,
}

impl AiModerationResult {
    pub fn clean() -> Self {
        Self {
            is_flagged: false,
            primary_category: None,
            confidence: 0.0,
            scores: AiCategoryScores::default(),
            detected_reasons: Vec::new(),
            evasion_score: 0.0,
        }
    }
}

/// Feature entry with category log-odds weights.
struct FeatureWeight {
    token: &'static str,
    spam: f32,
    csam: f32,
    gore: f32,
    bigotry: f32,
    harassment: f32,
    reason: &'static str,
}

/// AI model knowledge weights table calibrated across moderation corpora.
static AI_FEATURE_WEIGHTS: &[FeatureWeight] = &[
    // --- SPAM / SCAMS / CRYPTO PHISHING ---
    FeatureWeight {
        token: "double your crypto",
        spam: 3.5,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "crypto_doubler_lure",
    },
    FeatureWeight {
        token: "send btc get",
        spam: 3.8,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "crypto_doubler_scheme",
    },
    FeatureWeight {
        token: "connect wallet",
        spam: 2.2,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "wallet_connection_prompt",
    },
    FeatureWeight {
        token: "validate seed phrase",
        spam: 4.5,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "seed_phrase_phishing",
    },
    FeatureWeight {
        token: "restore seed phrase",
        spam: 4.5,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "seed_phrase_phishing",
    },
    FeatureWeight {
        token: "claim airdrop",
        spam: 2.8,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "airdrop_scam",
    },
    FeatureWeight {
        token: "make money fast",
        spam: 3.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "fast_cash_lure",
    },
    FeatureWeight {
        token: "guaranteed profit",
        spam: 3.2,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "ponzi_guaranteed_return",
    },
    FeatureWeight {
        token: "risk-free return",
        spam: 3.2,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "ponzi_guaranteed_return",
    },
    FeatureWeight {
        token: "dm me on telegram",
        spam: 2.6,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "messenger_redirect",
    },
    FeatureWeight {
        token: "free discord nitro",
        spam: 3.5,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "nitro_phishing",
    },
    FeatureWeight {
        token: "crypto giveaway",
        spam: 3.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "crypto_giveaway",
    },
    FeatureWeight {
        token: "pump and dump",
        spam: 3.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "market_manipulation_spam",
    },
    FeatureWeight {
        token: "binary options",
        spam: 2.8,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "trading_signals_spam",
    },
    FeatureWeight {
        token: "forex trading signal",
        spam: 2.8,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "trading_signals_spam",
    },
    FeatureWeight {
        token: "lottery winner",
        spam: 3.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "lottery_scam",
    },
    // --- CSAM / CHILD EXPLOITATION ---
    FeatureWeight {
        token: "trade cp",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "csam_trade_indicator",
    },
    FeatureWeight {
        token: "selling cp",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "csam_sale_indicator",
    },
    FeatureWeight {
        token: "underage pack",
        spam: 0.0,
        csam: 4.8,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "underage_pack_solicitation",
    },
    FeatureWeight {
        token: "underage nudes",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "underage_nudes_solicitation",
    },
    FeatureWeight {
        token: "child porn",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "csam_explicit_keyword",
    },
    FeatureWeight {
        token: "pedophile",
        spam: 0.0,
        csam: 4.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "pedophilia_indicator",
    },
    FeatureWeight {
        token: "pedo",
        spam: 0.0,
        csam: 3.5,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "pedo_abbreviation",
    },
    FeatureWeight {
        token: "lolicon",
        spam: 0.0,
        csam: 4.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "underage_illicit_category",
    },
    FeatureWeight {
        token: "shotacon",
        spam: 0.0,
        csam: 4.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "underage_illicit_category",
    },
    FeatureWeight {
        token: "preteen nude",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "csam_solicitation",
    },
    FeatureWeight {
        token: "pedochat",
        spam: 0.0,
        csam: 5.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "csam_domain_marker",
    },
    FeatureWeight {
        token: "jailbait tube",
        spam: 0.0,
        csam: 4.5,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "underage_exploitation_tube",
    },
    // --- GORE / GRAPHIC VIOLENCE / SELF-HARM ---
    FeatureWeight {
        token: "beheading video",
        spam: 0.0,
        csam: 0.0,
        gore: 4.5,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "execution_video_reference",
    },
    FeatureWeight {
        token: "decapitation",
        spam: 0.0,
        csam: 0.0,
        gore: 3.8,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "graphic_violence_token",
    },
    FeatureWeight {
        token: "dismemberment video",
        spam: 0.0,
        csam: 0.0,
        gore: 4.5,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "dismemberment_footage",
    },
    FeatureWeight {
        token: "cartel execution",
        spam: 0.0,
        csam: 0.0,
        gore: 4.5,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "cartel_execution_footage",
    },
    FeatureWeight {
        token: "snuff film",
        spam: 0.0,
        csam: 0.0,
        gore: 4.8,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "snuff_media",
    },
    FeatureWeight {
        token: "flaying video",
        spam: 0.0,
        csam: 0.0,
        gore: 4.8,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "torture_media",
    },
    FeatureWeight {
        token: "chainsaw execution",
        spam: 0.0,
        csam: 0.0,
        gore: 4.8,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "execution_video_reference",
    },
    FeatureWeight {
        token: "animal crush",
        spam: 0.0,
        csam: 0.0,
        gore: 4.8,
        bigotry: 0.0,
        harassment: 0.0,
        reason: "animal_torture",
    },
    FeatureWeight {
        token: "how to kill yourself",
        spam: 0.0,
        csam: 0.0,
        gore: 4.5,
        bigotry: 0.0,
        harassment: 2.0,
        reason: "suicide_instruction",
    },
    FeatureWeight {
        token: "go kill yourself",
        spam: 0.0,
        csam: 0.0,
        gore: 2.5,
        bigotry: 0.0,
        harassment: 4.2,
        reason: "self_harm_incitement",
    },
    FeatureWeight {
        token: "slit your wrists",
        spam: 0.0,
        csam: 0.0,
        gore: 3.5,
        bigotry: 0.0,
        harassment: 3.5,
        reason: "self_harm_incitement",
    },
    FeatureWeight {
        token: "kys",
        spam: 0.0,
        csam: 0.0,
        gore: 2.0,
        bigotry: 0.0,
        harassment: 3.8,
        reason: "self_harm_acronym",
    },
    // --- BIGOTRY / HATE SPEECH / HARASSMENT ---
    FeatureWeight {
        token: "nigger",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 5.0,
        harassment: 3.0,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "coon",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.0,
        harassment: 2.5,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "spic",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.0,
        harassment: 2.5,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "chink",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.0,
        harassment: 2.5,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "gook",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.0,
        harassment: 2.5,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "wetback",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.5,
        harassment: 2.5,
        reason: "racist_slur",
    },
    FeatureWeight {
        token: "kike",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.8,
        harassment: 2.5,
        reason: "antisemitic_slur",
    },
    FeatureWeight {
        token: "faggot",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.8,
        harassment: 3.0,
        reason: "homophobic_slur",
    },
    FeatureWeight {
        token: "fag",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 3.8,
        harassment: 2.0,
        reason: "homophobic_slur",
    },
    FeatureWeight {
        token: "dyke",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 3.8,
        harassment: 2.0,
        reason: "homophobic_slur",
    },
    FeatureWeight {
        token: "tranny",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.5,
        harassment: 2.5,
        reason: "transphobic_slur",
    },
    FeatureWeight {
        token: "shemale",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.0,
        harassment: 2.0,
        reason: "transphobic_slur",
    },
    FeatureWeight {
        token: "white power",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.5,
        harassment: 1.5,
        reason: "white_supremacist_slogan",
    },
    FeatureWeight {
        token: "white pride",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 3.5,
        harassment: 1.0,
        reason: "white_supremacist_rhetoric",
    },
    FeatureWeight {
        token: "hitler was right",
        spam: 0.0,
        csam: 0.0,
        gore: 1.0,
        bigotry: 5.0,
        harassment: 2.5,
        reason: "hate_speech_genocide_praise",
    },
    FeatureWeight {
        token: "race traitor",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 3.8,
        harassment: 2.5,
        reason: "hate_group_trope",
    },
    FeatureWeight {
        token: "mud people",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.5,
        harassment: 2.0,
        reason: "dehumanizing_racist_slur",
    },
    FeatureWeight {
        token: "subhuman race",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 4.5,
        harassment: 2.0,
        reason: "dehumanizing_racist_slur",
    },
    FeatureWeight {
        token: "gas the",
        spam: 0.0,
        csam: 0.0,
        gore: 2.0,
        bigotry: 5.0,
        harassment: 3.5,
        reason: "genocidal_threat",
    },
    FeatureWeight {
        token: "kill all",
        spam: 0.0,
        csam: 0.0,
        gore: 2.0,
        bigotry: 4.2,
        harassment: 4.0,
        reason: "mass_violence_incitement",
    },
    // --- HARASSMENT & TARGETED DOXING ---
    FeatureWeight {
        token: "i will find you and kill",
        spam: 0.0,
        csam: 0.0,
        gore: 1.5,
        bigotry: 0.0,
        harassment: 5.0,
        reason: "targeted_death_threat",
    },
    FeatureWeight {
        token: "i will hunt you down",
        spam: 0.0,
        csam: 0.0,
        gore: 1.0,
        bigotry: 0.0,
        harassment: 5.0,
        reason: "targeted_threat",
    },
    FeatureWeight {
        token: "doxxing",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 3.2,
        reason: "dox_activity",
    },
    FeatureWeight {
        token: "leak your address",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 4.5,
        reason: "dox_threat",
    },
    FeatureWeight {
        token: "know where you live",
        spam: 0.0,
        csam: 0.0,
        gore: 0.0,
        bigotry: 0.0,
        harassment: 4.0,
        reason: "dox_threat",
    },
];

/// Computes the evasion score based on obfuscation tactics (0.0 to 1.0).
fn calculate_evasion_score(text: &str) -> f32 {
    let total_chars = text.chars().count();
    if total_chars == 0 {
        return 0.0;
    }

    let mut zero_width_count = 0;
    let mut homoglyph_count = 0;
    let mut leetspeak_sub_count = 0;

    for c in text.chars() {
        if matches!(
            c,
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{200E}' | '\u{200F}' | '\u{202A}'
                ..='\u{202E}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}'
        ) {
            zero_width_count += 1;
        }

        let mapped_h = crate::normalize::map_homoglyph(c);
        if mapped_h != c && c.is_alphabetic() {
            homoglyph_count += 1;
        }

        let mapped_l = crate::normalize::map_leetspeak(c);
        if mapped_l != c && !c.is_alphabetic() {
            leetspeak_sub_count += 1;
        }
    }

    let zw_ratio = (zero_width_count as f32 * 5.0) / total_chars as f32;
    let h_ratio = (homoglyph_count as f32 * 2.0) / total_chars as f32;
    let l_ratio = (leetspeak_sub_count as f32 * 1.5) / total_chars as f32;

    let raw = zw_ratio + h_ratio + l_ratio;
    raw.clamp(0.0, 1.0)
}

/// Sigmoid squashing function to translate log-odds scores into [0.0, 1.0] probability.
#[inline]
fn sigmoid(score: f32, midpoint: f32, scale: f32) -> f32 {
    if score <= 0.0 {
        return 0.0;
    }
    let z = (score - midpoint) * scale;
    let p = 1.0 / (1.0 + (-z).exp());
    p.clamp(0.0, 1.0)
}

/// Classifies input text with the Lightweight AI Moderation model.
pub fn classify_text(text: &str) -> AiModerationResult {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return AiModerationResult::clean();
    }
    if trimmed.len() > crate::check::MAX_MODERATION_INPUT_LEN {
        return AiModerationResult {
            is_flagged: true,
            primary_category: Some("oversize".to_string()),
            confidence: 1.0,
            scores: AiCategoryScores::default(),
            detected_reasons: vec!["input_oversize".to_string()],
            evasion_score: 1.0,
        };
    }
    let variants = crate::normalize::generate_normalized_variants(trimmed);
    let spam_verdict = crate::spam::check_spam(trimmed);
    let csam_verdict = crate::csam::check_csam_text(trimmed);
    let gore_verdict = crate::gore::check_gore_text(trimmed);
    classify_text_with_verdicts(
        trimmed,
        &variants,
        &spam_verdict,
        &csam_verdict,
        &gore_verdict,
    )
}

/// Shared classification core. Callers that already ran the structural
/// checks (csam/gore/spam) and the variant normalization pass their results
/// in so the work is done exactly once per input (feed moderation runs this
/// after `check_text_comprehensive` already evaluated the same layers).
pub(crate) fn classify_text_with_verdicts(
    text: &str,
    variants: &[String],
    spam_verdict: &crate::spam::SpamVerdict,
    csam_verdict: &crate::csam::CsamVerdict,
    gore_verdict: &crate::gore::GoreVerdict,
) -> AiModerationResult {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return AiModerationResult::clean();
    }
    if trimmed.len() > crate::check::MAX_MODERATION_INPUT_LEN {
        return AiModerationResult {
            is_flagged: true,
            primary_category: Some("oversize".to_string()),
            confidence: 1.0,
            scores: AiCategoryScores::default(),
            detected_reasons: vec!["input_oversize".to_string()],
            evasion_score: 1.0,
        };
    }

    let evasion_score = calculate_evasion_score(trimmed);
    let lower_variants: Vec<String> = variants.iter().map(|v| v.to_ascii_lowercase()).collect();

    let mut raw_spam = 0.0f32;
    let mut raw_csam = 0.0f32;
    let mut raw_gore = 0.0f32;
    let mut raw_bigotry = 0.0f32;
    let mut raw_harassment = 0.0f32;
    let mut reasons = HashSet::new();

    // Check each feature token once across all variants
    for fw in AI_FEATURE_WEIGHTS {
        let matched = lower_variants.iter().any(|v| v.contains(fw.token));
        if matched {
            if fw.spam > 0.0 {
                raw_spam += fw.spam;
                reasons.insert(fw.reason.to_string());
            }
            if fw.csam > 0.0 {
                raw_csam += fw.csam;
                reasons.insert(fw.reason.to_string());
            }
            if fw.gore > 0.0 {
                raw_gore += fw.gore;
                reasons.insert(fw.reason.to_string());
            }
            if fw.bigotry > 0.0 {
                raw_bigotry += fw.bigotry;
                reasons.insert(fw.reason.to_string());
            }
            if fw.harassment > 0.0 {
                raw_harassment += fw.harassment;
                reasons.insert(fw.reason.to_string());
            }
        }
    }

    // Blend structural heuristic signals (precomputed by the caller)
    if spam_verdict.is_spam {
        raw_spam += spam_verdict.confidence * 4.0;
        if let Some(r) = &spam_verdict.reason {
            reasons.insert(r.clone());
        }
    }

    if csam_verdict.is_csam {
        raw_csam += csam_verdict.severity as f32 * 3.0;
        if let Some(r) = &csam_verdict.rule {
            reasons.insert(r.clone());
        }
    }

    if gore_verdict.is_gore {
        raw_gore += gore_verdict.severity as f32 * 2.5;
        if let Some(r) = &gore_verdict.rule {
            reasons.insert(r.clone());
        }
    }

    // Boost scores if evasion / obfuscation tactics were detected
    if evasion_score > 0.3 {
        let multiplier = 1.0 + (evasion_score * 0.5);
        raw_spam *= multiplier;
        raw_csam *= multiplier;
        raw_gore *= multiplier;
        raw_bigotry *= multiplier;
        raw_harassment *= multiplier;
        reasons.insert("adversarial_evasion_attempt".to_string());
    }

    // Probability calibrations
    let mut p_spam = sigmoid(raw_spam, 2.5, 0.85);
    let mut p_csam = sigmoid(raw_csam, 2.0, 1.2);
    let mut p_gore = sigmoid(raw_gore, 2.2, 0.9);
    let mut p_bigotry = sigmoid(raw_bigotry, 2.2, 0.95);
    let mut p_harassment = sigmoid(raw_harassment, 2.5, 0.85);

    // Trained-NN blend: scores are max(rule-derived, neural). The NN lifts
    // classes the heuristic tables miss (novel obfuscations, phrasings) while
    // rule hits keep their exact behavior. NN-only uplifts record a reason.
    let nn = crate::nn::nn_scores(trimmed);
    const NN_CUTOFFS: [f32; 5] = [0.55, 0.45, 0.50, 0.50, 0.55]; // spam,csam,gore,bigotry,harassment
    let nn_arr = nn.as_array();
    let mut blended = [p_spam, p_csam, p_gore, p_bigotry, p_harassment];
    for (i, p) in blended.iter_mut().enumerate() {
        *p = p.max(nn_arr[i]);
        if nn_arr[i] >= NN_CUTOFFS[i] {
            let cat = ["spam", "csam", "gore", "bigotry", "harassment"][i];
            reasons.insert(format!("nn_model:{cat}"));
        }
    }
    p_spam = blended[IDX_SPAM];
    p_csam = blended[IDX_CSAM];
    p_gore = blended[IDX_GORE];
    p_bigotry = blended[IDX_BIGOTRY];
    p_harassment = blended[IDX_HARASSMENT];

    let scores = AiCategoryScores {
        spam: p_spam,
        csam: p_csam,
        gore: p_gore,
        bigotry: p_bigotry,
        harassment: p_harassment,
    };

    // Decision thresholds
    let is_csam_flag = p_csam >= 0.45;
    let is_gore_flag = p_gore >= 0.50;
    let is_bigotry_flag = p_bigotry >= 0.50;
    let is_spam_flag = p_spam >= 0.55;
    let is_harassment_flag = p_harassment >= 0.55;

    let is_flagged =
        is_csam_flag || is_gore_flag || is_bigotry_flag || is_spam_flag || is_harassment_flag;

    let mut primary_category = None;
    let mut max_p = 0.0f32;

    if is_csam_flag && p_csam > max_p {
        max_p = p_csam;
        primary_category = Some("csam".to_string());
    }
    if is_gore_flag && p_gore > max_p {
        max_p = p_gore;
        primary_category = Some("gore".to_string());
    }
    if is_bigotry_flag && p_bigotry > max_p {
        max_p = p_bigotry;
        primary_category = Some("bigotry".to_string());
    }
    if is_harassment_flag && p_harassment > max_p {
        max_p = p_harassment;
        primary_category = Some("harassment".to_string());
    }
    if is_spam_flag && p_spam > max_p {
        max_p = p_spam;
        primary_category = Some("spam".to_string());
    }

    let mut reason_list: Vec<String> = reasons.into_iter().collect();
    reason_list.sort();

    AiModerationResult {
        is_flagged,
        primary_category,
        confidence: max_p,
        scores,
        detected_reasons: reason_list,
        evasion_score,
    }
}

/// JSON serialized output of AI text classification.
pub fn classify_text_json(text: &str) -> String {
    let res = classify_text(text);
    serde_json::to_string(&res).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_text_passes_ai() {
        let res = classify_text("Had a great time hiking in the mountains today!");
        assert!(!res.is_flagged);
        assert_eq!(res.primary_category, None);
        assert!(res.scores.spam < 0.2);
        assert!(res.scores.csam < 0.1);
        assert!(res.scores.gore < 0.1);
        assert!(res.scores.bigotry < 0.1);
    }

    #[test]
    fn test_mild_phrase_not_overcounted() {
        // "connect wallet" alone is a mild indicator (weight 2.2 < midpoint 2.5);
        // it shouldn't be multiplied across 5 variants to flag innocent web3 discussions.
        let res = classify_text("You can connect wallet in the settings screen if you like.");
        assert!(!res.is_flagged);
        assert!(res.scores.spam < 0.55);
    }

    #[test]
    fn test_spam_detected_ai() {
        let res = classify_text("Send 1 BTC to double your crypto instantly! Guaranteed profit");
        assert!(res.is_flagged);
        assert_eq!(res.primary_category.as_deref(), Some("spam"));
        assert!(res.scores.spam > 0.6);
        assert!(res
            .detected_reasons
            .iter()
            .any(|r| r.contains("crypto") || r.contains("doubler")));
    }

    #[test]
    fn test_csam_zero_tolerance_ai() {
        let res = classify_text("selling cp pack on telegram");
        assert!(res.is_flagged);
        assert_eq!(res.primary_category.as_deref(), Some("csam"));
        assert!(res.scores.csam > 0.7);
    }

    #[test]
    fn test_gore_violence_detected_ai() {
        let res = classify_text("watch this brutal beheading video decapitation clip");
        assert!(res.is_flagged);
        assert_eq!(res.primary_category.as_deref(), Some("gore"));
        assert!(res.scores.gore > 0.6);
    }

    #[test]
    fn test_bigotry_detected_ai() {
        let res = classify_text("white power rally kill all race traitors");
        assert!(res.is_flagged);
        assert_eq!(res.primary_category.as_deref(), Some("bigotry"));
        assert!(res.scores.bigotry > 0.6);
    }

    #[test]
    fn test_harassment_detected_ai() {
        let res = classify_text("i will find you and kill you leak your address");
        assert!(res.is_flagged);
        assert_eq!(res.primary_category.as_deref(), Some("harassment"));
        assert!(res.scores.harassment > 0.6);
    }

    #[test]
    fn test_obfuscation_evasion_detected() {
        // Cyrillic homoglyph evasion
        let res = classify_text("f\u{0430}gg\u{043E}t");
        assert!(res.is_flagged);
        assert!(res.evasion_score > 0.2);
        assert_eq!(res.primary_category.as_deref(), Some("bigotry"));
    }
}
