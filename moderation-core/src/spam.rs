//! Advanced Spam Detection Engine.
//!
//! Combines signature pattern matching (crypto scams, phishing, fast cash,
//! bot recruitment) with structural heuristics (link density, mention storms,
//! token entropy, and line repetition).

use std::collections::HashSet;
use std::sync::OnceLock;

/// Result of evaluating text against spam detection rules and heuristics.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpamVerdict {
    pub is_spam: bool,
    pub confidence: f32,
    pub reason: Option<String>,
}

struct SpamPattern {
    pattern: &'static str,
    reason: &'static str,
    weight: f32,
}

const SPAM_PATTERNS: &[SpamPattern] = &[
    // Crypto Doubler & Multiplier Scams
    SpamPattern {
        pattern: r"(?i)\b(?:send|deposit)\s+(?:0?\.\d+|\d+)\s*(?:btc|eth|sol|usdt|bnb)\b.*?\b(?:and\s+get|to\s+get|receive|get)\s*(?:double|\d+x|2x|return)",
        reason: "crypto_doubler_scam",
        weight: 1.0,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:double|multiply)\s+your\s+(?:crypto|btc|eth|sol|money|investment)\b",
        reason: "crypto_doubler_scam",
        weight: 0.9,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:crypto|bitcoin|ethereum|solana)\s+(?:giveaway|airdrop)\s*(?:live|bonus|instant|official)\b",
        reason: "crypto_giveaway_scam",
        weight: 0.85,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:claim|verify)\s+(?:your\s+)?(?:airdrop|tokens?|reward|whitelist)\s+(?:now|here|today)\b",
        reason: "airdrop_phishing",
        weight: 0.85,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:validate|restore|verify|input|enter|import)\s+(?:your\s+)?(?:seed\s*phrase|private\s*key|secret\s*recovery|mnemonic)\b",
        reason: "wallet_drainer_phishing",
        weight: 1.0,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:connect|unlock)\s+(?:your\s+)?(?:wallet|metamask|phantom|trust\s*wallet)\s+(?:to\s+(?:claim|receive|verify|get|double|mint)|and\s+(?:claim|verify|receive|get|validate))\b",
        reason: "wallet_drainer_phishing",
        weight: 1.0,
    },
    // Fast Cash, Work from Home & Ponzi Schemes
    SpamPattern {
        pattern: r"(?i)\b(?:make|earn)\s*\$?\d{2,6}\s*(?:a\s+day|daily|per\s+day|a\s+week|weekly|from\s+home|fast)\b",
        reason: "fast_cash_scam",
        weight: 0.85,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:guaranteed|risk[- ]free)\s+(?:profit|return|income|payout|daily\s+return)\b",
        reason: "ponzi_investment_scam",
        weight: 0.85,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:binary\s+options|forex\s+trading\s+signals?|pump\s+and\s+dump\s+group|vip\s+crypto\s+signals?)\b",
        reason: "trading_signal_spam",
        weight: 0.8,
    },
    // Bot Recruitment / External Messengers
    SpamPattern {
        pattern: r"(?i)\b(?:dm|message|text|inbox|contact)\s+me\s+(?:on\s+|via\s+)?(?:telegram|whatsapp|wa\.me|signal|t\.me)\b",
        reason: "messenger_redirect_spam",
        weight: 0.75,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:t\.me|wa\.me|discord\.gg)/[a-zA-Z0-9_+%-]+\s*(?:earn|crypto|free|money|vip|leak|trade|signal)",
        reason: "spam_invite_link",
        weight: 0.9,
    },
    SpamPattern {
        pattern: r"(?i)\bwhatsapp\s*(?:number|\+?\d{7,15})\s*(?:invest|earn|crypto|profit)",
        reason: "whatsapp_investment_spam",
        weight: 0.9,
    },
    // Phishing & Malicious Freebies
    SpamPattern {
        pattern: r"(?i)\bfree\s+(?:discord\s+nitro|onlyfans\s+leak|steam\s+gift\s*card|robux|v-bucks|amazon\s+gift\s*card)\b",
        reason: "phishing_giveaway",
        weight: 0.9,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:congratulations|winner)\b.*\b(?:you\s+have\s+been\s+selected|claim\s+your\s+prize|won\s+\$?\d+)\b",
        reason: "lottery_winner_scam",
        weight: 0.85,
    },
    SpamPattern {
        pattern: r"(?i)\b(?:click\s+here|act\s+now|limited\s+time\s+offer)\s+https?://[^\s]+",
        reason: "clickbait_spam_link",
        weight: 0.75,
    },
    SpamPattern {
        pattern: r"(?i)(?:@everyone|@channel|@here)\s+(?:check|follow|join|click|buy|claim|free|airdrop)",
        reason: "mass_mention_spam",
        weight: 0.85,
    },
];

struct SpamRegexSet {
    set: regex::RegexSet,
    reasons: Vec<&'static str>,
    weights: Vec<f32>,
}

fn get_spam_regex_set() -> &'static SpamRegexSet {
    static SET: OnceLock<SpamRegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        let patterns: Vec<&str> = SPAM_PATTERNS.iter().map(|p| p.pattern).collect();
        let reasons: Vec<&'static str> = SPAM_PATTERNS.iter().map(|p| p.reason).collect();
        let weights: Vec<f32> = SPAM_PATTERNS.iter().map(|p| p.weight).collect();
        let set = regex::RegexSetBuilder::new(patterns)
            .case_insensitive(true)
            .size_limit(1 << 30)
            .dfa_size_limit(1 << 30)
            .build()
            .unwrap_or_else(|_| regex::RegexSet::empty());
        SpamRegexSet {
            set,
            reasons,
            weights,
        }
    })
}

/// Computes link density: fraction of non-whitespace characters that belong to URLs.
fn compute_link_density(text: &str) -> f32 {
    let mut url_chars = 0;
    let total_chars = text.chars().filter(|c| !c.is_whitespace()).count();
    if total_chars == 0 {
        return 0.0;
    }

    for word in text.split_whitespace() {
        if word.starts_with("http://")
            || word.starts_with("https://")
            || word.starts_with("www.")
            || word.starts_with("t.me/")
            || word.starts_with("wa.me/")
        {
            url_chars += word.chars().count();
        }
    }

    (url_chars as f32) / (total_chars as f32)
}

/// Detects excessive repeated identical lines (flooding).
fn check_line_flooding(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 4 {
        return false;
    }

    let mut line_counts = std::collections::HashMap::new();
    for line in &lines {
        *line_counts.entry(*line).or_insert(0) += 1;
    }

    for count in line_counts.values() {
        if *count >= 4 && (*count as f32 / lines.len() as f32) >= 0.5 {
            return true;
        }
    }

    false
}

/// Detects mass mention storms (more than 7 distinct mentions or @everyone spam).
fn check_mention_storm(text: &str) -> bool {
    let mentions: HashSet<&str> = text
        .split_whitespace()
        .filter(|w| {
            w.starts_with('@') || w.starts_with("nostr:npub") || w.starts_with("nostr:nprofile")
        })
        .collect();

    mentions.len() >= 8
}

/// Computes simple Shannon entropy of character frequencies to flag low-entropy keyboard mash / wall-of-emoji.
fn check_low_entropy_spam(text: &str) -> bool {
    let clean: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() < 30 {
        return false;
    }

    let mut counts = std::collections::HashMap::new();
    for c in &clean {
        *counts.entry(*c).or_insert(0) += 1;
    }

    let len = clean.len() as f32;
    let mut entropy = 0.0f32;
    for &count in counts.values() {
        let p = (count as f32) / len;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }

    // Extremely low character entropy (< 1.5 bits) for a 30+ char string is character flooding
    entropy < 1.5
}

/// Evaluates text across all normalized variants against spam patterns and heuristics.
pub fn check_spam(text: &str) -> SpamVerdict {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return SpamVerdict {
            is_spam: false,
            confidence: 0.0,
            reason: None,
        };
    }

    // 1. Structural heuristic checks on original text
    if check_line_flooding(trimmed) {
        return SpamVerdict {
            is_spam: true,
            confidence: 0.95,
            reason: Some("line_flooding_spam".to_string()),
        };
    }

    if check_mention_storm(trimmed) {
        return SpamVerdict {
            is_spam: true,
            confidence: 0.9,
            reason: Some("mention_storm_spam".to_string()),
        };
    }

    if check_low_entropy_spam(trimmed) {
        return SpamVerdict {
            is_spam: true,
            confidence: 0.85,
            reason: Some("low_entropy_character_spam".to_string()),
        };
    }

    let link_density = compute_link_density(trimmed);
    if link_density > 0.85 && trimmed.len() > 60 {
        return SpamVerdict {
            is_spam: true,
            confidence: 0.8,
            reason: Some("excessive_link_density".to_string()),
        };
    }

    // 2. Pattern checks across normalized variants
    let variants = crate::normalize::generate_normalized_variants(trimmed);
    let spam_set = get_spam_regex_set();

    for variant in &variants {
        let matches = spam_set.set.matches(variant);
        for idx in matches.into_iter() {
            let weight = spam_set.weights[idx];
            if weight >= 0.75 {
                return SpamVerdict {
                    is_spam: true,
                    confidence: weight,
                    reason: Some(spam_set.reasons[idx].to_string()),
                };
            }
        }
    }

    SpamVerdict {
        is_spam: false,
        confidence: 0.0,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_doubler_detected() {
        let text = "Send 0.5 BTC to this address and get 2x return instantly!";
        let v = check_spam(text);
        assert!(v.is_spam);
        assert!(v.reason.as_deref().unwrap().contains("crypto_doubler"));
    }

    #[test]
    fn test_airdrop_seed_phrase_phishing_detected() {
        let text = "Claim your airdrop! Connect wallet and validate seed phrase now";
        let v = check_spam(text);
        assert!(v.is_spam);
        assert!(v.reason.as_deref().unwrap().contains("wallet_drainer"));
    }

    #[test]
    fn test_fast_cash_scam_detected() {
        let text = "Make $5000 a day working from home guaranteed!";
        let v = check_spam(text);
        assert!(v.is_spam);
    }

    #[test]
    fn test_obfuscated_spam_detected() {
        // Leetspeak + homoglyphs
        let text = "d0ubl3 y0ur cryp+0 n0w";
        let v = check_spam(text);
        assert!(v.is_spam);
    }

    #[test]
    fn test_line_flooding_detected() {
        let text = "BUY NOW\nBUY NOW\nBUY NOW\nBUY NOW\nBUY NOW";
        let v = check_spam(text);
        assert!(v.is_spam);
        assert_eq!(v.reason.as_deref(), Some("line_flooding_spam"));
    }

    #[test]
    fn test_clean_post_passes() {
        let text =
            "Hello everyone! Just deployed the new Nostr relay on my home server. Works great!";
        let v = check_spam(text);
        assert!(!v.is_spam);
    }

    #[test]
    fn test_link_density_codepoint_not_byte() {
        // "ñ" is 2 bytes but 1 codepoint.  URL "https://example.com" = 19 codepoints.
        // Total non-whitespace: 2 (ññ) + 2 (is) + 1 (a) + 4 (test) + 19 (url) = 28 codepoints.
        // Link density = 19/28 ≈ 0.6786, below the 0.85 spam threshold.
        let text = "ññ is a test https://example.com";
        let d = compute_link_density(text);
        assert!(
            (d - 19.0 / 28.0).abs() < f32::EPSILON,
            "expected ~0.6786, got {d}"
        );
        assert!(d < 0.85, "multi-byte post wrongly flagged: density {d}");
    }
}
