use std::sync::OnceLock;

const MAX_MODERATION_INPUT_LEN: usize = 256 * 1024;

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

struct CompiledModerationPattern {
    regex: regex::Regex,
    category: &'static str,
    severity: i32,
}

fn get_compiled_moderation_patterns() -> &'static Vec<CompiledModerationPattern> {
    static COMPILED: OnceLock<Vec<CompiledModerationPattern>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        MODERATION_PATTERNS
            .iter()
            .map(|entry| {
                let re = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    regex::RegexBuilder::new(entry.pattern)
                        .case_insensitive(true)
                        .size_limit(1 << 30)
                        .dfa_size_limit(1 << 30)
                        .build()
                })) {
                    Ok(Ok(r)) => r,
                    _ => regex::Regex::new(r"^$").expect("fallback regex must compile"),
                };
                CompiledModerationPattern {
                    regex: re,
                    category: entry.category,
                    severity: entry.severity,
                }
            })
            .collect()
    })
}
fn build_result_json(passed: bool, category: Option<&str>) -> String {
    match category {
        Some(cat) => format!("{{\"passed\":{},\"category\":\"{}\"}}", passed, cat),
        None => format!("{{\"passed\":{},\"category\":null}}", passed),
    }
}

pub fn check_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return build_result_json(true, None);
    }
    // Oversized input fails closed: regex scanning is bounded by the size
    // limit, so anything larger is flagged instead of silently passing.
    if trimmed.len() > MAX_MODERATION_INPUT_LEN {
        return build_result_json(false, Some("oversize"));
    }
    let patterns = get_compiled_moderation_patterns();
    for entry in patterns {
        if entry.severity >= 2 && entry.regex.is_match(trimmed) {
            return build_result_json(false, Some(entry.category));
        }
    }
    build_result_json(true, None)
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
        let mut invalid = proof.clone();
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
        let blacklisted = vec![proof.blacklist_nullifier_hash.clone()];
        assert!(!check_zk_trust_proof(&json, "wot_root_123", &blacklisted));
    }
}
