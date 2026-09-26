//! Multilingual Snowball word stemming for full-text search indexing and query expansion.
//!
//! Normalizes words to their grammatical stems across languages using `rust-stemmers`.

use rust_stemmers::{Algorithm, Stemmer};

/// Supported stemming algorithms / natural languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StemLanguage {
    #[default]
    English,
    Spanish,
    French,
    German,
    Russian,
    Portuguese,
    Italian,
    Dutch,
    Swedish,
}

impl StemLanguage {
    /// Convert to internal `rust-stemmers` Algorithm enum.
    pub fn to_algorithm(self) -> Algorithm {
        match self {
            Self::English => Algorithm::English,
            Self::Spanish => Algorithm::Spanish,
            Self::French => Algorithm::French,
            Self::German => Algorithm::German,
            Self::Russian => Algorithm::Russian,
            Self::Portuguese => Algorithm::Portuguese,
            Self::Italian => Algorithm::Italian,
            Self::Dutch => Algorithm::Dutch,
            Self::Swedish => Algorithm::Swedish,
        }
    }

    /// Map from ISO 639-1 code (e.g. "en", "es", "fr", "de", "ru").
    pub fn from_iso(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Self::English),
            "es" => Some(Self::Spanish),
            "fr" => Some(Self::French),
            "de" => Some(Self::German),
            "ru" => Some(Self::Russian),
            "pt" => Some(Self::Portuguese),
            "it" => Some(Self::Italian),
            "nl" => Some(Self::Dutch),
            "sv" => Some(Self::Swedish),
            _ => None,
        }
    }
}

/// Reduce a single word to its grammatical root stem.
pub fn stem_word(word: &str, lang: StemLanguage) -> String {
    let lower = word.trim().to_lowercase();
    if lower.is_empty() {
        return String::new();
    }
    let stemmer = Stemmer::create(lang.to_algorithm());
    stemmer.stem(&lower).to_string()
}

/// Tokenize a text string, strip surrounding punctuation, and return stemmed tokens.
pub fn stem_text(text: &str, lang: Option<StemLanguage>) -> Vec<String> {
    let stem_lang = lang.unwrap_or_default();
    let stemmer = Stemmer::create(stem_lang.to_algorithm());

    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(|token| {
            let lower = token.to_lowercase();
            stemmer.stem(&lower).to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_stemming() {
        assert_eq!(stem_word("running", StemLanguage::English), "run");
        assert_eq!(stem_word("runs", StemLanguage::English), "run");
        assert_eq!(stem_word("easily", StemLanguage::English), "easili");
        assert_eq!(stem_word("connecting", StemLanguage::English), "connect");
        assert_eq!(stem_word("connections", StemLanguage::English), "connect");
    }

    #[test]
    fn spanish_stemming() {
        assert_eq!(stem_word("caminando", StemLanguage::Spanish), "camin");
        assert_eq!(stem_word("caminaron", StemLanguage::Spanish), "camin");
    }

    #[test]
    fn french_stemming() {
        assert_eq!(stem_word("mangeait", StemLanguage::French), "mang");
        assert_eq!(stem_word("marchons", StemLanguage::French), "marchon");
    }

    #[test]
    fn german_stemming() {
        assert_eq!(stem_word("häuser", StemLanguage::German), "haus");
        assert_eq!(stem_word("hauses", StemLanguage::German), "haus");
    }

    #[test]
    fn stem_text_tokenization() {
        let text = "Fast-running nodes connect together smoothly!";
        let stems = stem_text(text, Some(StemLanguage::English));
        assert!(stems.contains(&"fast".to_string()));
        assert!(stems.contains(&"run".to_string()));
        assert!(stems.contains(&"node".to_string()));
        assert!(stems.contains(&"connect".to_string()));
        assert!(stems.contains(&"smooth".to_string()));
    }

    #[test]
    fn iso_mapping() {
        assert_eq!(StemLanguage::from_iso("en"), Some(StemLanguage::English));
        assert_eq!(StemLanguage::from_iso("ES"), Some(StemLanguage::Spanish));
        assert_eq!(StemLanguage::from_iso("xyz"), None);
    }
}
