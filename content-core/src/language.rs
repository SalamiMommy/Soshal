//! On-device natural language detection for content feeds, notes, and profile bios.
//!
//! Uses `whichlang` (pure Rust 3-gram language detection with zero external dependencies)
//! to classify post texts, determine text direction (LTR vs RTL) for UI layout,
//! and support localized translation/filtering workflows.

use serde::{Deserialize, Serialize};

/// Supported natural languages for content classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentLanguage {
    Arabic,
    Chinese,
    German,
    English,
    French,
    Hindi,
    Italian,
    Japanese,
    Korean,
    Dutch,
    Portuguese,
    Russian,
    Spanish,
    Swedish,
    Turkish,
    Vietnamese,
    Other,
}

impl ContentLanguage {
    /// Two-letter ISO 639-1 language code (or "und" for undetermined/other).
    pub fn iso_639_1(&self) -> &'static str {
        match self {
            Self::Arabic => "ar",
            Self::Chinese => "zh",
            Self::German => "de",
            Self::English => "en",
            Self::French => "fr",
            Self::Hindi => "hi",
            Self::Italian => "it",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::Dutch => "nl",
            Self::Portuguese => "pt",
            Self::Russian => "ru",
            Self::Spanish => "es",
            Self::Swedish => "sv",
            Self::Turkish => "tr",
            Self::Vietnamese => "vi",
            Self::Other => "und",
        }
    }

    /// Three-letter ISO 639-3 language code.
    pub fn iso_639_3(&self) -> &'static str {
        match self {
            Self::Arabic => "ara",
            Self::Chinese => "cmn",
            Self::German => "deu",
            Self::English => "eng",
            Self::French => "fra",
            Self::Hindi => "hin",
            Self::Italian => "ita",
            Self::Japanese => "jpn",
            Self::Korean => "kor",
            Self::Dutch => "nld",
            Self::Portuguese => "por",
            Self::Russian => "rus",
            Self::Spanish => "spa",
            Self::Swedish => "swe",
            Self::Turkish => "tur",
            Self::Vietnamese => "vie",
            Self::Other => "und",
        }
    }

    /// English display name for the language.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Arabic => "Arabic",
            Self::Chinese => "Chinese",
            Self::German => "German",
            Self::English => "English",
            Self::French => "French",
            Self::Hindi => "Hindi",
            Self::Italian => "Italian",
            Self::Japanese => "Japanese",
            Self::Korean => "Korean",
            Self::Dutch => "Dutch",
            Self::Portuguese => "Portuguese",
            Self::Russian => "Russian",
            Self::Spanish => "Spanish",
            Self::Swedish => "Swedish",
            Self::Turkish => "Turkish",
            Self::Vietnamese => "Vietnamese",
            Self::Other => "Other / Undetermined",
        }
    }

    /// Returns `true` if the language uses a Right-to-Left (RTL) script.
    pub fn is_rtl(&self) -> bool {
        matches!(self, Self::Arabic)
    }

    /// Parse from an ISO 639-1 two-letter code.
    pub fn from_iso_639_1(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "ar" => Some(Self::Arabic),
            "zh" => Some(Self::Chinese),
            "de" => Some(Self::German),
            "en" => Some(Self::English),
            "fr" => Some(Self::French),
            "hi" => Some(Self::Hindi),
            "it" => Some(Self::Italian),
            "ja" => Some(Self::Japanese),
            "ko" => Some(Self::Korean),
            "nl" => Some(Self::Dutch),
            "pt" => Some(Self::Portuguese),
            "ru" => Some(Self::Russian),
            "es" => Some(Self::Spanish),
            "sv" => Some(Self::Swedish),
            "tr" => Some(Self::Turkish),
            "vi" => Some(Self::Vietnamese),
            "und" => Some(Self::Other),
            _ => None,
        }
    }

    /// Convert from `whichlang::Lang`.
    pub fn from_whichlang(lang: whichlang::Lang) -> Self {
        match lang {
            whichlang::Lang::Ara => Self::Arabic,
            whichlang::Lang::Cmn => Self::Chinese,
            whichlang::Lang::Deu => Self::German,
            whichlang::Lang::Eng => Self::English,
            whichlang::Lang::Fra => Self::French,
            whichlang::Lang::Hin => Self::Hindi,
            whichlang::Lang::Ita => Self::Italian,
            whichlang::Lang::Jpn => Self::Japanese,
            whichlang::Lang::Kor => Self::Korean,
            whichlang::Lang::Nld => Self::Dutch,
            whichlang::Lang::Por => Self::Portuguese,
            whichlang::Lang::Rus => Self::Russian,
            whichlang::Lang::Spa => Self::Spanish,
            whichlang::Lang::Swe => Self::Swedish,
            whichlang::Lang::Tur => Self::Turkish,
            whichlang::Lang::Vie => Self::Vietnamese,
        }
    }
}

/// Detect the natural language of a given text string.
///
/// Returns `None` if the text has fewer than 3 non-whitespace characters
/// (too short to reliably identify trigrams).
pub fn detect_content_language(text: &str) -> Option<ContentLanguage> {
    let trimmed = text.trim();
    if trimmed.chars().take(3).count() < 3 {
        return None;
    }
    let detected = whichlang::detect_language(trimmed);
    Some(ContentLanguage::from_whichlang(detected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_english() {
        let text = "Hello world, this is a social network client written in Flutter and Rust.";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::English));
        assert!(!lang.unwrap().is_rtl());
        assert_eq!(lang.unwrap().iso_639_1(), "en");
        assert_eq!(lang.unwrap().iso_639_3(), "eng");
    }

    #[test]
    fn detect_spanish() {
        let text = "Hola amigos, bienvenidos a nuestra plataforma descentralizada y segura.";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::Spanish));
        assert!(!lang.unwrap().is_rtl());
        assert_eq!(lang.unwrap().iso_639_1(), "es");
    }

    #[test]
    fn detect_french() {
        let text = "Bonjour à tous, nous développons une application sociale sécurisée.";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::French));
        assert_eq!(lang.unwrap().iso_639_1(), "fr");
    }

    #[test]
    fn detect_german() {
        let text = "Guten Tag, das ist ein dezentrales soziales Netzwerk ohne zentrale Server.";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::German));
        assert_eq!(lang.unwrap().iso_639_1(), "de");
    }

    #[test]
    fn detect_arabic_rtl() {
        let text = "مرحبا بكم في هذه الشبكة الاجتماعية الموزعة والآمنة";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::Arabic));
        assert!(lang.unwrap().is_rtl());
        assert_eq!(lang.unwrap().iso_639_1(), "ar");
    }

    #[test]
    fn detect_russian() {
        let text = "Привет мир, это клиент децентрализованной социальной сети.";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::Russian));
        assert_eq!(lang.unwrap().iso_639_1(), "ru");
    }

    #[test]
    fn detect_japanese() {
        let text = "こんにちは世界、分散型ソーシャルネットワークへようこそ。";
        let lang = detect_content_language(text);
        assert_eq!(lang, Some(ContentLanguage::Japanese));
        assert_eq!(lang.unwrap().iso_639_1(), "ja");
    }

    #[test]
    fn detect_short_or_empty_returns_none() {
        assert_eq!(detect_content_language(""), None);
        assert_eq!(detect_content_language("   "), None);
        assert_eq!(detect_content_language("hi"), None);
        assert_eq!(detect_content_language(" a "), None);
    }

    #[test]
    fn iso_code_roundtrip() {
        assert_eq!(
            ContentLanguage::from_iso_639_1("en"),
            Some(ContentLanguage::English)
        );
        assert_eq!(
            ContentLanguage::from_iso_639_1("ES"),
            Some(ContentLanguage::Spanish)
        );
        assert_eq!(
            ContentLanguage::from_iso_639_1("AR"),
            Some(ContentLanguage::Arabic)
        );
        assert_eq!(ContentLanguage::from_iso_639_1("xyz"), None);
    }
}
