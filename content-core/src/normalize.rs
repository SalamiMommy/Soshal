//! Unicode normalization forms and anti-spoofing canonicalization.
//!
//! Provides canonical Unicode composition (NFC, NFKC) and decomposition (NFD, NFKD)
//! per Unicode Standard Annex #15 using `unicode-normalization`.

use unicode_normalization::UnicodeNormalization;

/// Canonical Decomposition, followed by Canonical Composition (NFC).
pub fn to_nfc(text: &str) -> String {
    text.nfc().collect()
}

/// Compatibility Decomposition, followed by Canonical Composition (NFKC).
///
/// Converts fullwidth characters, font variants, ligatures, and superscript
/// digits to standard forms (e.g. `"Ｓｏｓｈａｌ"` -> `"Soshal"`).
pub fn to_nfkc(text: &str) -> String {
    text.nfkc().collect()
}

/// Canonical Decomposition (NFD).
pub fn to_nfd(text: &str) -> String {
    text.nfd().collect()
}

/// Compatibility Decomposition (NFKD).
pub fn to_nfkd(text: &str) -> String {
    text.nfkd().collect()
}

/// Normalize a social hashtag or topic tag for deterministic indexing.
///
/// Strips leading `#` characters, applies NFKC normalization to resolve
/// fullwidth characters/ligatures, and converts to lowercase.
pub fn normalize_tag(tag: &str) -> String {
    let trimmed = tag.trim().trim_start_matches('#');
    to_nfkc(trimmed).to_lowercase()
}

/// Canonicalize and validate a NIP-05 user handle (`<name>@<domain>`).
///
/// Strips zero-width characters (ZWSP, ZWNJ, ZWJ, BOM), normalizes Unicode to NFKC,
/// and validates the structural split of `local@domain` to prevent homoglyph impersonation.
pub fn normalize_nip05_handle(handle: &str) -> Result<String, String> {
    let sanitized: String = handle
        .chars()
        .filter(|&c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}'))
        .collect();

    let nfkc = to_nfkc(&sanitized);
    let trimmed = nfkc.trim().to_ascii_lowercase();

    let parts: Vec<&str> = trimmed.split('@').collect();
    if parts.len() != 2 {
        return Err("NIP-05 handle must contain exactly one '@' separator".to_string());
    }

    let local = parts[0];
    let domain = parts[1];

    if local.is_empty() {
        return Err("NIP-05 local name cannot be empty".to_string());
    }
    if domain.is_empty() || !domain.contains('.') {
        return Err("NIP-05 domain must contain a valid TLD".to_string());
    }

    Ok(format!("{local}@{domain}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_and_nfkc_composition() {
        // Fullwidth "Ｓｏｓｈａｌ" -> "Soshal" under NFKC
        let fullwidth = "Ｓｏｓｈａｌ";
        assert_eq!(to_nfkc(fullwidth), "Soshal");
        assert_ne!(to_nfc(fullwidth), "Soshal"); // NFC preserves compatibility characters

        // Decomposed 'e' + combining acute -> composed 'é'
        let decomposed = "e\u{0301}";
        assert_eq!(to_nfc(decomposed), "é");
        assert_eq!(to_nfkc(decomposed), "é");
    }

    #[test]
    fn tag_normalization() {
        assert_eq!(normalize_tag("#Bitcoin"), "bitcoin");
        assert_eq!(normalize_tag("##nostr"), "nostr");
        assert_eq!(normalize_tag("#Ｂｉｔｃｏｉｎ"), "bitcoin");
        assert_eq!(normalize_tag("  #CRYPTO  "), "crypto");
    }

    #[test]
    fn nip05_handle_normalization_and_spoof_defense() {
        // Clean handle
        assert_eq!(
            normalize_nip05_handle("alice@soshal.net").unwrap(),
            "alice@soshal.net"
        );

        // Strips hidden zero-width spaces (ZWSP attack)
        let sneaky = "ali\u{200B}ce@soshal.net";
        assert_eq!(normalize_nip05_handle(sneaky).unwrap(), "alice@soshal.net");

        // Fullwidth domain normalization
        let fullwidth_handle = "bob@ｓｏｓｈａｌ.net";
        assert_eq!(
            normalize_nip05_handle(fullwidth_handle).unwrap(),
            "bob@soshal.net"
        );

        // Invalid handles
        assert!(normalize_nip05_handle("nodomain").is_err());
        assert!(normalize_nip05_handle("@domain.com").is_err());
        assert!(normalize_nip05_handle("user@nodot").is_err());
    }
}
