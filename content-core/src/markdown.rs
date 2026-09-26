use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::ast_parser::{ParsedSpan, SpanType};

/// Extracted structural metadata from a Markdown/CommonMark document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MarkdownMetadata {
    pub links: Vec<String>,
    pub images: Vec<String>,
    pub headings: Vec<(u32, String)>,
    pub plain_excerpt: String,
}

/// Cap on input length to prevent denial-of-service via huge markdown documents.
pub const MAX_MARKDOWN_INPUT_BYTES: usize = 256 * 1024;

/// Parse CommonMark text into structured `ParsedSpan`s using event-driven streaming.
pub fn parse_markdown_spans(input: &str, max_bytes: usize) -> Vec<ParsedSpan> {
    let limit = max_bytes.min(MAX_MARKDOWN_INPUT_BYTES);
    let slice = if input.len() > limit {
        &input[..limit]
    } else {
        input
    };

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(slice, options);
    let mut spans = Vec::new();

    let mut current_link: Option<String> = None;
    let mut is_bold = false;
    let mut is_italic = false;

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => {
                is_bold = true;
            }
            Event::End(TagEnd::Strong) => {
                is_bold = false;
            }
            Event::Start(Tag::Emphasis) => {
                is_italic = true;
            }
            Event::End(TagEnd::Emphasis) => {
                is_italic = false;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                current_link = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                current_link = None;
            }
            Event::Start(Tag::Heading { .. }) => {
                // Headings can be styled as bold
                is_bold = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                is_bold = false;
            }
            Event::Code(text) => {
                spans.push(ParsedSpan {
                    text: text.to_string(),
                    span_type: SpanType::CodeInline,
                    target: None,
                });
            }
            Event::Text(text) => {
                let span_type = if current_link.is_some() {
                    SpanType::Link
                } else if is_bold {
                    SpanType::Bold
                } else if is_italic {
                    SpanType::Italic
                } else {
                    SpanType::Text
                };

                let target = current_link.clone();
                spans.push(ParsedSpan {
                    text: text.to_string(),
                    span_type,
                    target,
                });
            }
            Event::SoftBreak | Event::HardBreak => {
                spans.push(ParsedSpan {
                    text: "\n".to_string(),
                    span_type: SpanType::Text,
                    target: None,
                });
            }
            _ => {}
        }
    }

    spans
}

/// Extract links, images, headings, and plain-text excerpt in a single pass.
pub fn extract_markdown_metadata(input: &str, max_bytes: usize) -> MarkdownMetadata {
    let limit = max_bytes.min(MAX_MARKDOWN_INPUT_BYTES);
    let slice = if input.len() > limit {
        &input[..limit]
    } else {
        input
    };

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(slice, options);

    let mut metadata = MarkdownMetadata::default();
    let mut current_heading_level: Option<u32> = None;
    let mut current_heading_text = String::new();
    let mut excerpt_length = 0;
    const MAX_EXCERPT_CHARS: usize = 280;

    for event in parser {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                metadata.links.push(dest_url.to_string());
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                metadata.images.push(dest_url.to_string());
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let lvl = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                current_heading_level = Some(lvl);
                current_heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(lvl) = current_heading_level.take() {
                    metadata
                        .headings
                        .push((lvl, current_heading_text.trim().to_string()));
                }
            }
            Event::Text(text) => {
                if current_heading_level.is_some() {
                    current_heading_text.push_str(&text);
                } else if excerpt_length < MAX_EXCERPT_CHARS {
                    let needed = MAX_EXCERPT_CHARS - excerpt_length;
                    let to_add: String = text.chars().take(needed).collect();
                    excerpt_length += to_add.chars().count();
                    metadata.plain_excerpt.push_str(&to_add);
                }
            }
            Event::Code(text) if excerpt_length < MAX_EXCERPT_CHARS => {
                let needed = MAX_EXCERPT_CHARS - excerpt_length;
                let to_add: String = text.chars().take(needed).collect();
                excerpt_length += to_add.chars().count();
                metadata.plain_excerpt.push_str(&to_add);
            }
            _ => {}
        }
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_spans() {
        let md = "Hello **world**, check this [Soshal](https://soshal.app) and `code`!";
        let spans = parse_markdown_spans(md, 1000);

        assert_eq!(spans[0].text, "Hello ");
        assert_eq!(spans[0].span_type, SpanType::Text);

        assert_eq!(spans[1].text, "world");
        assert_eq!(spans[1].span_type, SpanType::Bold);

        assert_eq!(spans[2].text, ", check this ");

        assert_eq!(spans[3].text, "Soshal");
        assert_eq!(spans[3].span_type, SpanType::Link);
        assert_eq!(spans[3].target.as_deref(), Some("https://soshal.app"));

        assert_eq!(spans[5].text, "code");
        assert_eq!(spans[5].span_type, SpanType::CodeInline);
    }

    #[test]
    fn test_extract_markdown_metadata() {
        let md = "# Welcome to Soshal\n\nRead more at [Docs](https://docs.soshal.app).\n\n![Logo](https://soshal.app/logo.png)";
        let meta = extract_markdown_metadata(md, 5000);

        assert_eq!(meta.headings.len(), 1);
        assert_eq!(meta.headings[0], (1, "Welcome to Soshal".to_string()));

        assert_eq!(meta.links, vec!["https://docs.soshal.app"]);
        assert_eq!(meta.images, vec!["https://soshal.app/logo.png"]);

        assert!(meta.plain_excerpt.contains("Read more at Docs"));
    }
}
