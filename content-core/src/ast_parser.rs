//! AST Rich Text & Markdown parsing offloaded to Rust.
//!
//! Parses post text/markdown into flat, pre-calculated arrays of styled spans
//! to prevent regex overhead and micro-stuttering on the Flutter UI isolate.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpanType {
    Text,
    Mention,
    Hashtag,
    Link,
    Emoji,
    Bold,
    Italic,
    CodeInline,
    CodeBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedSpan {
    pub text: String,
    pub span_type: SpanType,
    pub target: Option<String>,
}

pub fn parse_post_ast(raw: &str) -> Vec<ParsedSpan> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::with_capacity((raw.len() / 6).max(4));

    // Split on whitespace or tokens and construct structured AST spans
    for word in raw.split_inclusive(|c: char| c.is_whitespace()) {
        let trimmed = word.trim();
        if (trimmed.starts_with('@') && trimmed.len() > 1) || trimmed.starts_with("nostr:npub1") {
            spans.push(ParsedSpan {
                text: word.to_string(),
                span_type: SpanType::Mention,
                target: Some(trimmed.to_string()),
            });
        } else if trimmed.starts_with('#') && trimmed.len() > 1 {
            spans.push(ParsedSpan {
                text: word.to_string(),
                span_type: SpanType::Hashtag,
                target: Some(trimmed.trim_start_matches('#').to_string()),
            });
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            spans.push(ParsedSpan {
                text: word.to_string(),
                span_type: SpanType::Link,
                target: Some(trimmed.to_string()),
            });
        } else if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 2 {
            spans.push(ParsedSpan {
                text: word.to_string(),
                span_type: SpanType::Emoji,
                target: Some(trimmed.trim_matches(':').to_string()),
            });
        } else {
            spans.push(ParsedSpan {
                text: word.to_string(),
                span_type: SpanType::Text,
                target: None,
            });
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_post_ast() {
        let text = "Hello @alice check #soshal at https://soshal.app :fire:";
        let spans = parse_post_ast(text);
        assert_eq!(spans.len(), 7);
        assert_eq!(spans[1].span_type, SpanType::Mention);
        assert_eq!(spans[3].span_type, SpanType::Hashtag);
        assert_eq!(spans[5].span_type, SpanType::Link);
        assert_eq!(spans[6].span_type, SpanType::Emoji);
    }
}
