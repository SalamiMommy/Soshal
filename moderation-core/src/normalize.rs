//! Text normalization and de-obfuscation utilities for moderation.
//!
//! Provides defense against evasion tactics:
//! - Strips zero-width characters, soft hyphens, and bidi override marks.
//! - Replaces Cyrillic, Greek, and other unicode homoglyphs with Latin equivalents.
//! - Maps common leetspeak substitutions (e.g., '1' -> 'i', '0' -> 'o', '3' -> 'e', '4' -> 'a', '$' -> 's', '@' -> 'a').
//! - Collapses consecutive character runs to prevent spaced-out or stretched evasion (e.g., "ffffrrreeee" -> "free").
//! - Collapses inter-character spacing (e.g., "s p a m" -> "spam").

/// Characters that should be completely stripped out (zero-width, invisible, bidi).
fn is_invisible_or_control(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' // Zero-Width Space
        | '\u{200C}' // Zero-Width Non-Joiner
        | '\u{200D}' // Zero-Width Joiner
        | '\u{200E}' // Left-to-Right Mark
        | '\u{200F}' // Right-to-Left Mark
        | '\u{202A}' // Left-to-Right Embedding
        | '\u{202B}' // Right-to-Left Embedding
        | '\u{202C}' // Pop Directional Formatting
        | '\u{202D}' // Left-to-Right Override
        | '\u{202E}' // Right-to-Left Override
        | '\u{2060}' // Word Joiner
        | '\u{FEFF}' // Zero Width No-Break Space (BOM)
        | '\u{00AD}' // Soft Hyphen
        | '\u{0300}'..='\u{036F}' // Combining Diacritical Marks
        | '\u{1AB0}'..='\u{1AFF}' // Combining Diacritical Marks Extended
        | '\u{1DC0}'..='\u{1DFF}' // Combining Diacritical Marks Supplement
        | '\u{20D0}'..='\u{20FF}' // Combining Diacritical Marks for Symbols
        | '\u{FE20}'..='\u{FE2F}' // Combining Half Marks
        | '\u{0000}'..='\u{0008}'
        | '\u{000E}'..='\u{001F}'
        | '\u{007F}'..='\u{009F}'
    )
}

/// Maps Cyrillic, Greek, Fullwidth, Mathematical, and other homoglyphs to standard ASCII Latin.
pub fn map_homoglyph(c: char) -> char {
    match c {
        // Cyrillic lookalikes
        'а' | 'А' => 'a',
        'б' | 'Б' => 'b',
        'в' | 'В' => 'b',
        'г' | 'Г' => 'r',
        'д' | 'Д' => 'd',
        'е' | 'Е' | 'ё' | 'Ё' | 'є' | 'Є' => 'e',
        'ж' | 'Ж' => 'z',
        'з' | 'З' => 'z',
        'и' | 'И' | 'і' | 'І' | 'ї' | 'Ї' => 'i',
        'й' | 'Й' => 'i',
        'к' | 'К' => 'k',
        'л' | 'Л' => 'l',
        'м' | 'М' => 'm',
        'н' | 'Н' => 'h',
        'о' | 'О' => 'o',
        'п' | 'П' => 'n',
        'р' | 'Р' => 'p',
        'с' | 'С' => 'c',
        'т' | 'Т' => 't',
        'у' | 'У' => 'y',
        'ф' | 'Ф' => 'f',
        'х' | 'Х' => 'x',
        'ц' | 'Ц' => 'u',
        'ч' | 'Ч' => 'h',
        'ш' | 'Ш' | 'щ' | 'Щ' => 'w',
        'ъ' | 'Ъ' | 'ь' | 'Ь' => 'b',
        'ы' | 'Ы' => 'y',
        'э' | 'Э' => 'e',
        'ю' | 'Ю' => 'u',
        'я' | 'Я' => 'r',
        // Greek lookalikes
        'α' | 'Α' => 'a',
        'β' | 'Β' => 'b',
        'γ' | 'Γ' => 'y',
        'δ' | 'Δ' => 'd',
        'ε' | 'Ε' => 'e',
        'ζ' | 'Ζ' => 'z',
        'η' | 'Η' => 'h',
        'θ' | 'Θ' => 'o',
        'ι' | 'Ι' => 'i',
        'κ' | 'Κ' => 'k',
        'λ' | 'Λ' => 'l',
        'μ' | 'Μ' => 'm',
        'ν' | 'Ν' => 'n',
        'ξ' | 'Ξ' => 'x',
        'ο' | 'Ο' => 'o',
        'π' | 'Π' => 'n',
        'ρ' | 'Ρ' => 'p',
        'σ' | 'Σ' | 'ς' => 's',
        'τ' | 'Τ' => 't',
        'υ' | 'Υ' => 'u',
        'φ' | 'Φ' => 'f',
        'χ' | 'Χ' => 'x',
        'ψ' | 'Ψ' => 'y',
        'ω' | 'Ω' => 'w',
        // Fullwidth ASCII (FF01..FF5E)
        '\u{FF01}'..='\u{FF5E}' => {
            let code = (c as u32) - 0xFEE0;
            char::from_u32(code).unwrap_or(c)
        }
        // Latin letters with diacritics / accents
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' => 'a',
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'Ǎ' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ǐ' => 'i',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'Ǐ' => 'i',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ŏ' | 'ő' | 'ǒ' | 'ø' => 'o',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ō' | 'Ŏ' | 'Ő' | 'Ǒ' | 'Ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ǔ' => 'u',
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ǔ' => 'u',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'Ý' | 'Ÿ' | 'Ŷ' => 'y',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'Ś' | 'Ŝ' | 'Ş' | 'Š' => 's',
        _ => c,
    }
}

/// Maps common leetspeak character substitutions to standard letters.
pub fn map_leetspeak(c: char) -> char {
    match c {
        '0' => 'o',
        '1' | '!' | '|' => 'i',
        '3' => 'e',
        '4' | '@' => 'a',
        '5' | '$' => 's',
        '7' | '+' => 't',
        '8' => 'b',
        _ => c,
    }
}

/// Basic normalization: strips invisibles and maps homoglyphs to lower-case ASCII.
pub fn normalize_basic(text: &str) -> String {
    text.chars()
        .filter(|c| !is_invisible_or_control(*c))
        .map(|c| map_homoglyph(c).to_ascii_lowercase())
        .collect()
}

/// Leetspeak + homoglyph normalization for keyword matching.
pub fn normalize_leetspeak(text: &str) -> String {
    text.chars()
        .filter(|c| !is_invisible_or_control(*c))
        .map(|c| {
            let h = map_homoglyph(c);
            let l = map_leetspeak(h);
            l.to_ascii_lowercase()
        })
        .collect()
}

/// Collapses consecutive runs of identical characters down to a single character (e.g. "sssspppaaaammmm" -> "spam").
pub fn collapse_repeats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_char: Option<char> = None;

    for c in text.chars() {
        if Some(c) != last_char {
            last_char = Some(c);
            out.push(c);
        }
    }
    out
}

/// Collapses isolated spaces between individual characters (e.g. "f r e e  m o n e y  now" -> "free money now").
pub fn collapse_spaced_words(text: &str) -> String {
    let mut intermediate = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        intermediate.push(c);
        if c.is_alphanumeric()
            && i + 2 < len
            && chars[i + 1] == ' '
            && chars[i + 2].is_alphanumeric()
            && (i + 3 >= len || chars[i + 3] == ' ' || !chars[i + 3].is_alphanumeric())
        {
            i += 2;
            continue;
        }
        i += 1;
    }

    let words: Vec<&str> = intermediate.split_whitespace().collect();
    words.join(" ")
}

/// Produces multiple normalized variants of the input text for anti-evasion scanning:
/// 1. Original text (trimmed)
/// 2. Basic normalized (homoglyphs mapped, invisibles stripped, lowercased)
/// 3. Leetspeak normalized
/// 4. Repetition-collapsed leetspeak
/// 5. Spaced-word collapsed
pub fn generate_normalized_variants(text: &str) -> Vec<String> {
    let mut variants = Vec::with_capacity(5);
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return variants;
    }

    let basic = normalize_basic(&trimmed);
    let leet = normalize_leetspeak(&trimmed);
    let collapsed = collapse_repeats(&leet);
    let unspaced = collapse_spaced_words(&trimmed);
    let unspaced_leet = collapse_spaced_words(&collapsed);

    variants.push(trimmed);
    if !variants.contains(&basic) {
        variants.push(basic);
    }
    if !variants.contains(&leet) {
        variants.push(leet);
    }
    if !variants.contains(&collapsed) {
        variants.push(collapsed);
    }
    if !variants.contains(&unspaced) {
        variants.push(unspaced);
    }
    if !variants.contains(&unspaced_leet) {
        variants.push(unspaced_leet);
    }

    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strips_zero_width_and_bidi() {
        let dirty = "f\u{200B}r\u{200C}e\u{200D}e\u{FEFF} m\u{00AD}oney";
        let clean = normalize_basic(dirty);
        assert_eq!(clean, "free money");
    }

    #[test]
    fn test_maps_cyrillic_homoglyphs() {
        // 'а' and 'о' Cyrillic
        let cyrillic = "f\u{0430}gg\u{043E}t";
        let mapped = normalize_basic(cyrillic);
        assert_eq!(mapped, "faggot");
    }

    #[test]
    fn test_maps_leetspeak() {
        let leet = "fr33 m0n3y $p@m";
        let mapped = normalize_leetspeak(leet);
        assert_eq!(mapped, "free money spam");
    }

    #[test]
    fn test_collapses_repeats() {
        let stretched = "sssspppaaaammmm";
        let collapsed = collapse_repeats(stretched);
        assert_eq!(collapsed, "spam");
    }

    #[test]
    fn test_collapses_spaced_words() {
        let spaced = "f r e e  m o n e y  now";
        let unspaced = collapse_spaced_words(spaced);
        assert_eq!(unspaced, "free money now");
    }
}
