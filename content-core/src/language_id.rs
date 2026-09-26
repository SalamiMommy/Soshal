//! BCP-47 / Unicode CLDR language identifier parsing, canonicalization, and matching.
//!
//! Complies with RFC 5646 / BCP 47 standards for language tagging across Nostr
//! NIP-56 / NIP-01 events and multilingual feed filtering.

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use unic_langid::LanguageIdentifier;

/// Parsed and normalized BCP-47 language tag components.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalLanguageTag {
    /// Full canonical BCP-47 string representation (e.g. `en-US`, `zh-Hans-CN`).
    pub canonical: String,
    /// Primary language subtag (e.g. `en`, `es`, `zh`).
    pub language: String,
    /// Optional script subtag (e.g. `Hans`, `Hant`, `Latn`, `Cyrl`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Optional region subtag (e.g. `US`, `GB`, `CN`, `419`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// Parse and extract canonical BCP-47 language components from a raw string.
pub fn parse_language_tag(tag: &str) -> Option<CanonicalLanguageTag> {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lang_id = LanguageIdentifier::from_str(trimmed).ok()?;
    Some(CanonicalLanguageTag {
        canonical: lang_id.to_string(),
        language: lang_id.language.as_str().to_string(),
        script: lang_id.script.map(|s| s.as_str().to_string()),
        region: lang_id.region.map(|r| r.as_str().to_string()),
    })
}

/// Check if a string is a valid BCP-47 language identifier.
pub fn is_valid_bcp47(tag: &str) -> bool {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return false;
    }
    LanguageIdentifier::from_str(trimmed).is_ok()
}

/// Canonicalize a language tag to standard BCP-47 casing and formatting.
///
/// For example, `EN-us` -> `en-US`, `zh-hans-cn` -> `zh-Hans-CN`.
pub fn canonicalize_bcp47(tag: &str) -> Option<String> {
    let lang_id = LanguageIdentifier::from_str(tag.trim()).ok()?;
    Some(lang_id.to_string())
}

/// Check if content language matches a user's language preference or filter.
///
/// Implements RFC 4647 prefix and subtag matching:
/// - If `preference` is `en`, it matches `en`, `en-US`, `en-GB`.
/// - If `preference` is `en-US`, it matches `en-US` but NOT `en-GB`.
/// - If `preference` specifies a script like `zh-Hans`, it matches `zh-Hans-CN` but NOT `zh-Hant`.
pub fn matches_language_filter(content_tag: &str, preference_tag: &str) -> bool {
    let content_id = match LanguageIdentifier::from_str(content_tag.trim()) {
        Ok(id) => id,
        Err(_) => return false,
    };
    let pref_id = match LanguageIdentifier::from_str(preference_tag.trim()) {
        Ok(id) => id,
        Err(_) => return false,
    };

    // Primary language must always match
    if content_id.language != pref_id.language {
        return false;
    }

    // If preference specifies a script, content must match that script
    if let Some(pref_script) = pref_id.script {
        if content_id.script != Some(pref_script) {
            return false;
        }
    }

    // If preference specifies a region, content must match that region
    if let Some(pref_region) = pref_id.region {
        if content_id.region != Some(pref_region) {
            return false;
        }
    }

    true
}

/// Extract and canonicalize a BCP-47 language tag from Nostr event `["l", ...]` tags (NIP-56 / NIP-01).
pub fn extract_bcp47_from_nostr_tags(tags: &[Vec<String>]) -> Option<String> {
    for tag in tags {
        if tag.len() >= 2 && tag[0] == "l" {
            let candidate = &tag[1];
            if let Some(canonical) = canonicalize_bcp47(candidate) {
                return Some(canonical);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_language_tags() {
        let parsed = parse_language_tag("en-US").expect("valid tag");
        assert_eq!(parsed.canonical, "en-US");
        assert_eq!(parsed.language, "en");
        assert_eq!(parsed.region.as_deref(), Some("US"));
        assert_eq!(parsed.script, None);

        let parsed_zh = parse_language_tag("zh-hans-cn").expect("valid tag");
        assert_eq!(parsed_zh.canonical, "zh-Hans-CN");
        assert_eq!(parsed_zh.language, "zh");
        assert_eq!(parsed_zh.script.as_deref(), Some("Hans"));
        assert_eq!(parsed_zh.region.as_deref(), Some("CN"));
    }

    #[test]
    fn test_canonicalize_and_validity() {
        assert!(is_valid_bcp47("es-419"));
        assert!(is_valid_bcp47("de-DE"));
        assert!(!is_valid_bcp47(""));
        assert!(!is_valid_bcp47("1234567890"));

        assert_eq!(canonicalize_bcp47("EN-gb").as_deref(), Some("en-GB"));
        assert_eq!(canonicalize_bcp47("JA").as_deref(), Some("ja"));
    }

    #[test]
    fn test_language_matching() {
        // Broad preference 'en' matches regional variants
        assert!(matches_language_filter("en-US", "en"));
        assert!(matches_language_filter("en-GB", "en"));
        assert!(!matches_language_filter("fr-FR", "en"));

        // Narrow preference 'en-US' only matches en-US
        assert!(matches_language_filter("en-US", "en-US"));
        assert!(!matches_language_filter("en-GB", "en-US"));

        // Script matching for Chinese
        assert!(matches_language_filter("zh-Hans-CN", "zh-Hans"));
        assert!(!matches_language_filter("zh-Hant-TW", "zh-Hans"));
    }

    #[test]
    fn test_extract_from_nostr_tags() {
        let tags = vec![
            vec!["t".to_string(), "nostr".to_string()],
            vec![
                "l".to_string(),
                "en-us".to_string(),
                "ISO-639-1".to_string(),
            ],
        ];
        assert_eq!(
            extract_bcp47_from_nostr_tags(&tags).as_deref(),
            Some("en-US")
        );

        let no_lang_tags = vec![vec!["p".to_string(), "abc123".to_string()]];
        assert_eq!(extract_bcp47_from_nostr_tags(&no_lang_tags), None);
    }
}
