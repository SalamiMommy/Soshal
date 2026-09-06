//! Zero-Tolerance CP / CSAM (Child Sexual Abuse Material & Exploitation) Filter.
//!
//! Provides strict, fail-closed detection against illegal child exploitation material:
//! - Illicit text keywords, trade codes, solicitation slang, and pedophilic lingo.
//! - Known illegal media hashes (SHA-256, BLAKE3, MD5, SHA-1).
//! - Illicit darknet domains, onion gateways, magnet URIs, and IPFS CIDs.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Verdict returned when scanning content for child sexual abuse material / exploitation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CsamVerdict {
    pub is_csam: bool,
    pub severity: i32,
    pub rule: Option<String>,
}

struct CsamPattern {
    pattern: &'static str,
    rule: &'static str,
}

const CSAM_PATTERNS: &[CsamPattern] = &[
    // Child exploitation explicit trade / solicitation keywords
    CsamPattern {
        pattern: r"(?i)\b(?:child|underage|minor|prepubescent|toddler|infant|pedo|pedophile)\s*(?:porn|pornography|sex|nude|nudes|naked|erotica|abuse|video|trade|collection|pack|mega|archive|dropbox|link|links|leak)\b",
        rule: "csam_solicitation_keyword",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:pedophile|pedophilia|pederasty|paedophile|paedophilia|hebephilia|lolicon|shotacon)\b",
        rule: "pedophilia_term",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:cp|csam)\s*(?:links?|pack|trade|mega|dropbox|vids?|collection|folder|archive|telegram|onion|darknet)\b",
        rule: "csam_trading_abbreviation",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:trade|selling|trading|share|buy|download|leaked)\s*(?:cp|csam|underage|loli|shota|preteen)\b",
        rule: "csam_trade_request",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:preteen|pre-teen|jailbait|pedo)\s*(?:porn|nudes?|sex|pics?|vids?|tube)\b",
        rule: "csam_illicit_category",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:t\.me|mega\.nz|dropbox\.com|onion)/[^\s]*(?:cp|csam|underage|pedo|childporn|preteen)",
        rule: "csam_distribution_link",
    },
    CsamPattern {
        pattern: r"(?i)\b(?:ageplay|age-play)\s*(?:abuse|porn|sex|minor|toddler)\b",
        rule: "csam_ageplay_abuse",
    },
];

struct CsamRegexSet {
    set: regex::RegexSet,
    rules: Vec<&'static str>,
}

fn get_csam_regex_set() -> &'static CsamRegexSet {
    static SET: OnceLock<CsamRegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        let patterns: Vec<&str> = CSAM_PATTERNS.iter().map(|p| p.pattern).collect();
        let rules: Vec<&'static str> = CSAM_PATTERNS.iter().map(|p| p.rule).collect();
        let set = crate::regex_util::build_regex_set(patterns);
        CsamRegexSet { set, rules }
    })
}

/// Known illegal media hash blocklist (canonical lowercase hex).
static KNOWN_CSAM_HASHES: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_known_csam_hashes() -> &'static HashSet<&'static str> {
    KNOWN_CSAM_HASHES.get_or_init(|| {
        let mut set = HashSet::new();
        // Synthetic sentinel blocklist test hashes for verification and regression testing
        set.insert(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855_sentinel_csam",
        );
        set.insert(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad_sentinel_csam",
        );
        set.insert("c27a20ff44e8bc1a3b1a8d052d9a6c4df103c80a2b0e8b1ef380b0b8e8f85f31");
        set.insert("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
        set
    })
}

/// Known illicit domains / onion distribution endpoints.
static KNOWN_CSAM_DOMAINS: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_known_csam_domains() -> &'static HashSet<&'static str> {
    KNOWN_CSAM_DOMAINS.get_or_init(|| {
        let mut set = HashSet::new();
        set.insert("pedochat.onion");
        set.insert("childporn.onion");
        set.insert("lolita.onion");
        set.insert("cparchive.onion");
        set
    })
}

/// Evaluates text for CSAM indicators across all normalized anti-evasion variants.
pub fn check_csam_text(text: &str) -> CsamVerdict {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CsamVerdict {
            is_csam: false,
            severity: 0,
            rule: None,
        };
    }

    // Check darknet domains
    let lower_raw = trimmed.to_ascii_lowercase();
    let csam_domains = get_known_csam_domains();
    for domain in csam_domains.iter() {
        if lower_raw.contains(domain) {
            return CsamVerdict {
                is_csam: true,
                severity: 3,
                rule: Some(format!("csam_domain_match: {domain}")),
            };
        }
    }

    // Check patterns across normalized variants
    let variants = crate::normalize::generate_normalized_variants(trimmed);
    let csam_set = get_csam_regex_set();

    for variant in &variants {
        if let Some(idx) = csam_set.set.matches(variant).into_iter().next() {
            return CsamVerdict {
                is_csam: true,
                severity: 3,
                rule: Some(csam_set.rules[idx].to_string()),
            };
        }
    }

    CsamVerdict {
        is_csam: false,
        severity: 0,
        rule: None,
    }
}

/// Evaluates a media hash (SHA-256, BLAKE3, MD5, SHA-1) against known illicit CSAM blocklists.
pub fn check_csam_hash(hash: &str) -> CsamVerdict {
    let cleaned = hash.trim().to_ascii_lowercase();
    if cleaned.is_empty() {
        return CsamVerdict {
            is_csam: false,
            severity: 0,
            rule: None,
        };
    }

    let hashes = get_known_csam_hashes();
    if hashes.contains(cleaned.as_str()) {
        return CsamVerdict {
            is_csam: true,
            severity: 3,
            rule: Some("known_csam_media_hash".to_string()),
        };
    }

    CsamVerdict {
        is_csam: false,
        severity: 0,
        rule: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csam_keywords_flagged() {
        let text = "trade cp pack on telegram";
        let v = check_csam_text(text);
        assert!(v.is_csam);
        assert_eq!(v.severity, 3);
        assert!(v.rule.unwrap().contains("csam"));
    }

    #[test]
    fn test_csam_obfuscated_flagged() {
        let text = "p3d0ph1l3";
        let v = check_csam_text(text);
        assert!(v.is_csam);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_csam_domain_flagged() {
        let text = "visit http://pedochat.onion for files";
        let v = check_csam_text(text);
        assert!(v.is_csam);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_csam_hash_flagged() {
        let hash = "c27a20ff44e8bc1a3b1a8d052d9a6c4df103c80a2b0e8b1ef380b0b8e8f85f31";
        let v = check_csam_hash(hash);
        assert!(v.is_csam);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_innocuous_text_passes() {
        let text = "The minor league baseball championship was exciting today!";
        let v = check_csam_text(text);
        assert!(!v.is_csam);
    }
}
