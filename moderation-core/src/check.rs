use std::sync::OnceLock;

pub(crate) const MAX_MODERATION_INPUT_LEN: usize = 256 * 1024;

/// Result of evaluating content for moderation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModerationVerdict {
    pub passed: bool,
    pub category: Option<String>,
    pub severity: i32,
    pub reason: Option<String>,
}

impl ModerationVerdict {
    pub fn pass() -> Self {
        Self {
            passed: true,
            category: None,
            severity: 0,
            reason: None,
        }
    }

    pub fn flag(category: &str, severity: i32, reason: Option<String>) -> Self {
        Self {
            passed: false,
            category: Some(category.to_string()),
            severity,
            reason,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| match &self.category {
            Some(cat) => format!("{{\"passed\":{},\"category\":\"{}\"}}", self.passed, cat),
            None => format!("{{\"passed\":{},\"category\":null}}", self.passed),
        })
    }
}

struct ModerationPattern {
    pattern: &'static str,
    category: &'static str,
    severity: i32,
}

const MODERATION_PATTERNS: &[ModerationPattern] = &[
    ModerationPattern {
        pattern: r"\bn[i1]gg[e3]r\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bcoon\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bsp[i1][ck]\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bch[i1]nk\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bgook\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bwetback\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bkike\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bsand n[i1]gg[e3]r\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bwhite power\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bwhite pride\b",
        category: "racist",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\brace traitor\b",
        category: "racist",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\bmud people\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bmonkey\b.*\b(?:black|african)\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\b(?:black|african)\b.*\bmonkey\b",
        category: "racist",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bf[a4]gg[o0]t\b",
        category: "homophobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bf[a4]g\b",
        category: "homophobic",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\bdyke\b",
        category: "homophobic",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\bqueer\b",
        category: "homophobic",
        severity: 1,
    },
    ModerationPattern {
        pattern: r"\b(?:gays?|homo|lesbians?)\b.*\b(?:die|kill|burn|hate|disgusting|sick|wrong|sin|evil|abomination)\b",
        category: "homophobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\b(?:die|kill|burn|hate|disgusting|sick|wrong|sin|evil|abomination)\b.*\b(?:gays?|homo|lesbians?)\b",
        category: "homophobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bsodomite\b",
        category: "homophobic",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\btr[ae]nny\b",
        category: "transphobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bshemale\b",
        category: "transphobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\btr[ae]nsgender\b.*\b(?:die|kill|burn|hate|disgusting|sick|wrong|sin|evil|abomination|delusion|mental illness)\b",
        category: "transphobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\b(?:die|kill|burn|hate|disgusting|sick|wrong|sin|evil|abomination|delusion|mental illness)\b.*\btr[ae]nsgender\b",
        category: "transphobic",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bautogynephile\b",
        category: "transphobic",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"\b(?:exterminat|eliminat|eradicat|wip[eo] out)\b.*\b(?:gay|homo|trans|jews?|muslim|mexican|asian)\b",
        category: "hate",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\b(?:gay|homo|trans|jews?|muslim|mexican|asian)\b.*\b(?:exterminat|eliminat|eradicat|wip[eo] out)\b",
        category: "hate",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bhitler\b.*\b(?:was right|did nothing wrong|should have finished)\b",
        category: "hate",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\bgas.*(?:jews?|gay|trans|black|muslim)\b",
        category: "hate",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"\b(?:jews?|gay|trans|black|muslim)\b.*\bgas\b",
        category: "hate",
        severity: 3,
    },
    ModerationPattern {
        pattern: r"(?:check out|follow me|subscribe).*https?://[^\s]+",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"https?://[^\s]+\s*(?:check out|follow|subscribe)",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"(?:click here|limited time|act now|exclusive offer)",
        category: "spam",
        severity: 1,
    },
    ModerationPattern {
        pattern: r"(?:buy now|free money|earn crypto|make money fast|crypto (?:giveaway|bonus))",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"(?:@everyone|@channel|@here)\s+(?:check|follow|join|click|buy)",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"(?:win a|congratulations.*winner|you.*won|claim.*prize)",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"(?:100%\s*(?:free|guaranteed)|double.*(?:money|btc|eth)|get rich)",
        category: "spam",
        severity: 2,
    },
    ModerationPattern {
        pattern: r"(?:referral|refer).*https?://[^\s]+",
        category: "spam",
        severity: 1,
    },
    ModerationPattern {
        pattern: r"https?://bit\.ly/[^\s]+\s*(?:earn|crypto|free|money)",
        category: "spam",
        severity: 2,
    },
];

struct ModerationSet {
    set: regex::RegexSet,
    categories: Vec<&'static str>,
    severities: Vec<i32>,
}

fn get_moderation_set() -> &'static ModerationSet {
    static SET: OnceLock<ModerationSet> = OnceLock::new();
    SET.get_or_init(|| {
        let patterns: Vec<&str> = MODERATION_PATTERNS.iter().map(|p| p.pattern).collect();
        let categories: Vec<&'static str> =
            MODERATION_PATTERNS.iter().map(|p| p.category).collect();
        let severities: Vec<i32> = MODERATION_PATTERNS.iter().map(|p| p.severity).collect();
        let set = regex::RegexSetBuilder::new(patterns)
            .case_insensitive(true)
            .size_limit(1 << 30)
            .dfa_size_limit(1 << 30)
            .build()
            .unwrap_or_else(|_| regex::RegexSet::empty());
        ModerationSet {
            set,
            categories,
            severities,
        }
    })
}

fn build_result_json(passed: bool, category: Option<&str>) -> String {
    match category {
        Some(cat) => format!("{{\"passed\":{},\"category\":\"{}\"}}", passed, cat),
        None => format!("{{\"passed\":{},\"category\":null}}", passed),
    }
}

/// Evaluates content across all layers (CSAM, Gore, Hate/Harassment, Spam, Custom Word Filters).
pub fn check_text_comprehensive(text: &str, custom_words: &[String]) -> ModerationVerdict {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ModerationVerdict::pass();
    }
    if trimmed.len() > MAX_MODERATION_INPUT_LEN {
        return ModerationVerdict::flag("oversize", 3, Some("input_oversize".to_string()));
    }

    // 1. CP / CSAM keyword filter (rule-based; no external blocklist)
    let csam_res = crate::csam::check_csam_text(trimmed);
    if csam_res.is_csam {
        return ModerationVerdict::flag("cp", csam_res.severity, csam_res.rule);
    }

    // 2. Gore, Extreme Violence & Self-Harm Filter
    let gore_res = crate::gore::check_gore_text(trimmed);
    if gore_res.is_gore && gore_res.severity >= 2 {
        return ModerationVerdict::flag("gore", gore_res.severity, gore_res.rule);
    }

    // 3. Normalized Hate Speech, Harassment & Base Patterns
    let variants = crate::normalize::generate_normalized_variants(trimmed);
    let mod_set = get_moderation_set();

    for variant in &variants {
        let matches = mod_set.set.matches(variant);
        for idx in matches.into_iter() {
            if mod_set.severities[idx] >= 2 {
                return ModerationVerdict::flag(
                    mod_set.categories[idx],
                    mod_set.severities[idx],
                    Some(format!("pattern_match: {}", mod_set.categories[idx])),
                );
            }
        }
    }

    // 4. Advanced Spam & Scam Filter
    let spam_res = crate::spam::check_spam(trimmed);
    if spam_res.is_spam {
        return ModerationVerdict::flag("spam", 2, spam_res.reason);
    }

    // 5. Lightweight AI Model Multi-Class Classification
    let ai_res = crate::ai_classifier::classify_text(trimmed);
    if ai_res.is_flagged {
        if let Some(cat) = ai_res.primary_category {
            let severity = if cat == "csam" { 3 } else { 2 };
            let reason = ai_res
                .detected_reasons
                .first()
                .cloned()
                .unwrap_or_else(|| format!("ai_classified_{cat}"));
            return ModerationVerdict::flag(&cat, severity, Some(reason));
        }
    }

    // 6. Custom Word Filters
    if !custom_words.is_empty() {
        let lower = trimmed.to_ascii_lowercase();
        let normalized = crate::normalize::normalize_basic(trimmed);
        for word in custom_words {
            let w = word.trim().to_ascii_lowercase();
            if !w.is_empty() && (lower.contains(&w) || normalized.contains(&w)) {
                return ModerationVerdict::flag(
                    "custom",
                    2,
                    Some(format!("custom_filter_match: {w}")),
                );
            }
        }
    }

    ModerationVerdict::pass()
}

/// Detailed AI classification entry point returning structured `AiModerationResult`.
pub fn check_text_ai(text: &str) -> crate::ai_classifier::AiModerationResult {
    crate::ai_classifier::classify_text(text)
}

/// Detailed AI classification entry point returning JSON string.
pub fn check_text_ai_json(text: &str) -> String {
    crate::ai_classifier::classify_text_json(text)
}

/// 2-Tier Hybrid text evaluation returning JSON string.
pub fn check_text_hybrid_json(text: &str, force_deep_scan: bool) -> String {
    crate::hybrid::evaluate_text_hybrid_json(text, force_deep_scan)
}

/// Check text with user custom word filters.
pub fn check_with_custom_words(text: &str, custom_words: &[String]) -> ModerationVerdict {
    check_text_comprehensive(text, custom_words)
}

/// Standard check_text entry point (JSON output for FFI compatibility).
pub fn check_text(text: &str) -> String {
    let verdict = check_text_comprehensive(text, &[]);
    build_result_json(verdict.passed, verdict.category.as_deref())
}

/// Verifies incoming zk-SNARK Web-of-Trust moderation proof.
pub fn check_zk_trust_proof(
    proof_json: &str,
    expected_wot_root: &str,
    blacklisted_nullifiers: &[String],
) -> bool {
    let Ok(proof) = serde_json::from_str::<soshal_crypto_core::zk_trust::ZkTrustProof>(proof_json)
    else {
        return false;
    };
    soshal_crypto_core::zk_trust::verify_zk_wot_proof(
        &proof,
        expected_wot_root,
        blacklisted_nullifiers,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_crypto_core::zk_trust::generate_zk_wot_proof;

    #[test]
    fn check_zk_trust_proof_valid_generated_proof() {
        let proof = generate_zk_wot_proof("pubkey_alice", "wot_root_123", "black_root_456");
        let json = serde_json::to_string(&proof).unwrap();
        assert!(check_zk_trust_proof(&json, "wot_root_123", &[]));
    }

    #[test]
    fn check_zk_trust_proof_tampered_bytes_rejected() {
        let proof = generate_zk_wot_proof("pubkey_alice", "wot_root_123", "black_root_456");
        let mut short = proof.clone();
        short.proof_bytes_b64 = "AAAA".to_string();
        let json = serde_json::to_string(&short).unwrap();
        assert!(!check_zk_trust_proof(&json, "wot_root_123", &[]));
        let mut invalid = proof;
        invalid.proof_bytes_b64 = "not base64 !!".to_string();
        let json = serde_json::to_string(&invalid).unwrap();
        assert!(!check_zk_trust_proof(&json, "wot_root_123", &[]));
    }

    #[test]
    fn check_zk_trust_proof_malformed_or_empty_rejected() {
        assert!(!check_zk_trust_proof("", "wot_root_123", &[]));
        assert!(!check_zk_trust_proof("not json", "wot_root_123", &[]));
        assert!(!check_zk_trust_proof("{}", "wot_root_123", &[]));
    }

    #[test]
    fn check_zk_trust_proof_wrong_root_or_blacklisted_rejected() {
        let proof = generate_zk_wot_proof("pubkey_alice", "wot_root_123", "black_root_456");
        let json = serde_json::to_string(&proof).unwrap();
        assert!(!check_zk_trust_proof(&json, "forged_root", &[]));
        let forged = generate_zk_wot_proof("pubkey_alice", "evil_root", "black_root_456");
        let forged_json = serde_json::to_string(&forged).unwrap();
        assert!(!check_zk_trust_proof(&forged_json, "wot_root_123", &[]));
        let blacklisted = vec![proof.blacklist_nullifier_hash];
        assert!(!check_zk_trust_proof(&json, "wot_root_123", &blacklisted));
    }
}
