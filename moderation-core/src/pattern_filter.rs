//! Wildcard and shell-pattern content filtering for user mute rules and moderation.
//!
//! Provides linear-time $O(n)$ pattern matching using `glob::Pattern`, eliminating
//! the risk of catastrophic backtracking (ReDoS) from user-authored regex patterns.

use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};

/// Result of evaluating text against a compiled wildcard rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternMatch {
    pub pattern: String,
    pub matched_text: String,
    pub reason: Option<String>,
}

/// A single compiled wildcard rule.
#[derive(Debug, Clone)]
struct CompiledRule {
    pattern: Pattern,
    raw_pattern: String,
    options: MatchOptions,
    reason: Option<String>,
}

/// Set of compiled wildcard patterns for high-speed content, tag, and handle filtering.
#[derive(Debug, Clone, Default)]
pub struct WildcardFilterSet {
    rules: Vec<CompiledRule>,
}

impl WildcardFilterSet {
    /// Create a new empty filter set.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a wildcard pattern (e.g. `*airdrop*`, `tg://*`, `crypto_bot_*`).
    ///
    /// Case-insensitive by default.
    pub fn add_pattern(
        &mut self,
        pattern_str: &str,
        case_sensitive: bool,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let pattern = Pattern::new(pattern_str)
            .map_err(|e| format!("Invalid wildcard pattern '{pattern_str}': {e}"))?;

        let options = MatchOptions {
            case_sensitive,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };

        self.rules.push(CompiledRule {
            pattern,
            raw_pattern: pattern_str.to_string(),
            options,
            reason: reason.map(|s| s.to_string()),
        });

        Ok(())
    }

    /// Check if a single target string (word, hashtag, URL, or handle) matches any rule.
    pub fn matches(&self, target: &str) -> bool {
        self.rules
            .iter()
            .any(|r| r.pattern.matches_with(target, r.options))
    }

    /// Find all rule matches for a given target string.
    pub fn find_matches(&self, target: &str) -> Vec<PatternMatch> {
        self.rules
            .iter()
            .filter(|r| r.pattern.matches_with(target, r.options))
            .map(|r| PatternMatch {
                pattern: r.raw_pattern.clone(),
                matched_text: target.to_string(),
                reason: r.reason.clone(),
            })
            .collect()
    }

    /// Scan a collection of tokens (e.g. whitespace-split words or tags) and return all matches.
    pub fn scan_tokens<'a, I>(&self, tokens: I) -> Vec<PatternMatch>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut matches = Vec::new();
        for token in tokens {
            let clean = token.trim();
            if clean.is_empty() {
                continue;
            }
            matches.extend(self.find_matches(clean));
        }
        matches
    }

    /// Filter a slice of tags, removing those that match any mute rule.
    pub fn filter_tags<'a>(&self, tags: &[&'a str]) -> Vec<&'a str> {
        tags.iter()
            .copied()
            .filter(|tag| !self.matches(tag))
            .collect()
    }

    /// Number of active rules in the set.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the filter set is empty.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Clear all rules from the filter set.
    pub fn clear(&mut self) {
        self.rules.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_filter_matching() {
        let mut set = WildcardFilterSet::new();
        set.add_pattern("*airdrop*", false, Some("scam"))
            .expect("valid pattern");
        set.add_pattern("crypto_bot_*", false, Some("bot"))
            .expect("valid pattern");
        set.add_pattern("*.onion", false, Some("darknet"))
            .expect("valid pattern");

        assert!(set.matches("free_airdrop_now"));
        assert!(set.matches("AIRDROP"));
        assert!(set.matches("crypto_bot_42"));
        assert!(set.matches("secret.onion"));
        assert!(!set.matches("bitcoin"));
        assert!(!set.matches("telegram.org"));
    }

    #[test]
    fn test_case_sensitivity() {
        let mut set = WildcardFilterSet::new();
        set.add_pattern("ExactMatch*", true, None).expect("valid");

        assert!(set.matches("ExactMatch123"));
        assert!(!set.matches("exactmatch123"));
    }

    #[test]
    fn test_filter_tags() {
        let mut set = WildcardFilterSet::new();
        set.add_pattern("#crypto*", false, None).expect("valid");
        set.add_pattern("#spam*", false, None).expect("valid");

        let tags = vec!["#nostr", "#crypto_giveaway", "#photography", "#spam123"];
        let clean = set.filter_tags(&tags);
        assert_eq!(clean, vec!["#nostr", "#photography"]);
    }

    #[test]
    fn test_scan_tokens() {
        let mut set = WildcardFilterSet::new();
        set.add_pattern("*giveaway*", false, Some("scam"))
            .expect("valid");

        let words = ["Check", "out", "this", "huge_giveaway!", "now"];
        // Strip punctuation for tokens
        let matches = set.scan_tokens(words.iter().map(|w| w.trim_matches('!')));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "huge_giveaway");
        assert_eq!(matches[0].reason.as_deref(), Some("scam"));
    }

    #[test]
    fn test_invalid_pattern() {
        let mut set = WildcardFilterSet::new();
        // Unclosed character class `[` is an invalid glob pattern
        assert!(set.add_pattern("[unclosed", false, None).is_err());
    }
}
