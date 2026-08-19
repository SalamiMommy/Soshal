//! Gore, Graphic Violence, Shock Sites, and Self-Harm Content Filter.
//!
//! Scans content for:
//! - Graphic violence, executions, decapitations, dismemberment, and snuff footage.
//! - Known gore and shock-site domains.
//! - Animal torture and extreme cruelty.
//! - Self-harm incitement and suicide instruction.
//! - Integration with NIP-36 content-warning / sensitive categorization.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Verdict returned when evaluating text for gore, violence, or self-harm content.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GoreVerdict {
    pub is_gore: bool,
    pub severity: i32,
    pub rule: Option<String>,
}

struct GorePattern {
    pattern: &'static str,
    rule: &'static str,
    severity: i32,
}

const GORE_PATTERNS: &[GorePattern] = &[
    // Graphic violence, executions, and dismemberment
    GorePattern {
        pattern: r"(?i)\b(?:beheading|decapitation|dismemberment|flaying|electrocution)\s*(?:video|footage|tape|clip|leak|gore|uncensored|hd)\b",
        rule: "graphic_execution_footage",
        severity: 3,
    },
    GorePattern {
        pattern: r"(?i)\b(?:cartel|isis|taliban|gang)\s*(?:execution|torture|beheading|chainsaw|mutilation|skinning)\s*(?:video|clip|leak)?\b",
        rule: "cartel_terrorist_torture_video",
        severity: 3,
    },
    GorePattern {
        pattern: r"(?i)\b(?:snuff\s*film|snuff\s*movie|snuff\s*video|real\s*death\s*video|live\s*suicide\s*stream)\b",
        rule: "snuff_or_death_stream",
        severity: 3,
    },
    GorePattern {
        pattern: r"(?i)\b(?:autopsy|corpse|mutilated\s*body|rotting\s*body|severed\s*head|crushed\s*skull)\s*(?:photos?|pics?|leak|video)\b",
        rule: "graphic_mutilation_media",
        severity: 2,
    },
    // Animal cruelty / crush
    GorePattern {
        pattern: r"(?i)\b(?:animal\s*crush|crush\s*fetish|tortur(?:e|ing)\s*(?:cats?|dogs?|kittens?|puppies?|animals?)|dog\s*fighting\s*video)\b",
        rule: "animal_cruelty_torture",
        severity: 3,
    },
    // Suicide & Self-Harm Incitement
    GorePattern {
        pattern: r"(?i)\b(?:how\s+to\s+(?:kill|hang|shoot|poison|suffocate)\s+yourself|suicide\s*(?:instructions?|guide|method|pills?\s*dose))\b",
        rule: "suicide_instruction",
        severity: 3,
    },
    GorePattern {
        pattern: r"(?i)\b(?:go\s+kill\s+yourself|kys|drink\s+bleach|slit\s+your\s+wrists|hang\s+yourself)\b",
        rule: "self_harm_incitement",
        severity: 2,
    },
];

struct GoreRegexSet {
    set: regex::RegexSet,
    rules: Vec<&'static str>,
    severities: Vec<i32>,
}

fn get_gore_regex_set() -> &'static GoreRegexSet {
    static SET: OnceLock<GoreRegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        let patterns: Vec<&str> = GORE_PATTERNS.iter().map(|p| p.pattern).collect();
        let rules: Vec<&'static str> = GORE_PATTERNS.iter().map(|p| p.rule).collect();
        let severities: Vec<i32> = GORE_PATTERNS.iter().map(|p| p.severity).collect();
        let set = regex::RegexSetBuilder::new(patterns)
            .case_insensitive(true)
            .size_limit(1 << 30)
            .dfa_size_limit(1 << 30)
            .build()
            .unwrap_or_else(|_| regex::RegexSet::empty());
        GoreRegexSet {
            set,
            rules,
            severities,
        }
    })
}

/// Known gore, graphic shock, and death repository domains.
static SHOCK_DOMAINS: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_shock_domains() -> &'static HashSet<&'static str> {
    SHOCK_DOMAINS.get_or_init(|| {
        let mut set = HashSet::new();
        set.insert("bestgore.com");
        set.insert("theync.com");
        set.insert("kaotic.com");
        set.insert("leakreality.com");
        set.insert("goregrish.com");
        set.insert("heavy-r.com");
        set.insert("crazyshit.com");
        set.insert("seegore.com");
        set.insert("watchpeopledie.tv");
        set.insert("documentingreality.com");
        set.insert("deathaddict.co");
        set.insert("horriblevideos.com");
        set
    })
}

/// Evaluates text for gore, extreme graphic violence, shock sites, and self-harm.
pub fn check_gore_text(text: &str) -> GoreVerdict {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return GoreVerdict {
            is_gore: false,
            severity: 0,
            rule: None,
        };
    }

    // Check shock-site domain matches
    let lower_raw = trimmed.to_ascii_lowercase();
    let shock_domains = get_shock_domains();
    for domain in shock_domains.iter() {
        if lower_raw.contains(domain) {
            return GoreVerdict {
                is_gore: true,
                severity: 3,
                rule: Some(format!("shock_site_domain: {domain}")),
            };
        }
    }

    // Check patterns across normalized variants
    let variants = crate::normalize::generate_normalized_variants(trimmed);
    let gore_set = get_gore_regex_set();

    for variant in &variants {
        if let Some(idx) = gore_set.set.matches(variant).into_iter().next() {
            return GoreVerdict {
                is_gore: true,
                severity: gore_set.severities[idx],
                rule: Some(gore_set.rules[idx].to_string()),
            };
        }
    }

    GoreVerdict {
        is_gore: false,
        severity: 0,
        rule: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beheading_footage_flagged() {
        let text = "watch this beheading video uncensored";
        let v = check_gore_text(text);
        assert!(v.is_gore);
        assert_eq!(v.severity, 3);
        assert!(v.rule.unwrap().contains("graphic_execution"));
    }

    #[test]
    fn test_shock_site_domain_flagged() {
        let text = "check out https://theync.com/video123";
        let v = check_gore_text(text);
        assert!(v.is_gore);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_animal_cruelty_flagged() {
        let text = "animal crush video clip";
        let v = check_gore_text(text);
        assert!(v.is_gore);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_suicide_instruction_flagged() {
        let text = "how to hang yourself suicide guide";
        let v = check_gore_text(text);
        assert!(v.is_gore);
        assert_eq!(v.severity, 3);
    }

    #[test]
    fn test_clean_medical_discussion_passes() {
        let text =
            "The surgeon performed a delicate coronary artery bypass procedure with precision.";
        let v = check_gore_text(text);
        assert!(!v.is_gore);
    }
}
