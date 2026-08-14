use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::json_util::{json_in, json_out};

/// Longest URL truncated for preview scanning.
#[doc(hidden)]
pub const MAX_PREVIEW_URL_LENGTH: usize = 8 * 1024;
const MATCH_NOTHING_REGEX: &str = r"[^\s\S]";

fn compile_regex(pattern: &str) -> Regex {
    static FALLBACK_RE: OnceLock<Regex> = OnceLock::new();
    Regex::new(pattern).unwrap_or_else(|_| {
        FALLBACK_RE
            .get_or_init(|| {
                Regex::new(MATCH_NOTHING_REGEX).expect("MATCH_NOTHING_REGEX must compile")
            })
            .clone()
    })
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::RegexBuilder::new(
            r##"https?://[^\s\u{2000}-\u{206F}\u{2E00}-\u{2E7F}\s!"#$%&'()*+,./:;<=>?@\[\]^`{|}~]+\.[^\s\u{2000}-\u{206F}\u{2E00}-\u{2E7F}\s!"#$%&'()*+,./:;<=>?@\[\]^`{|}~]{2,}(?:/[^\s\u{2000}-\u{206F}\u{2E00}-\u{2E7F}\s!"#$%&'()*+,./:;<=>?@\[\]^`{|}~]*)?"##,
        )
        .case_insensitive(true)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .unwrap_or_else(|_| compile_regex(MATCH_NOTHING_REGEX))
    })
}

/// Extract candidate URLs from plain text.
#[doc(hidden)]
pub fn extract_urls(text: &str) -> Vec<String> {
    let re = url_regex();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut urls: Vec<String> = Vec::new();
    for m in re.find_iter(text) {
        let s = m.as_str();
        if s.len() > MAX_PREVIEW_URL_LENGTH {
            continue;
        }
        if seen.insert(s) {
            urls.push(s.to_string());
        }
        if urls.len() >= 1024 {
            break;
        }
    }
    urls
}

#[derive(Deserialize)]
struct ExtractUrlsInput {
    text: String,
}

pub fn extract_urls_json(input: &str) -> String {
    let parsed = json_in(
        input,
        ExtractUrlsInput {
            text: String::new(),
        },
    );
    let out = extract_urls(&parsed.text);
    json_out(&out, "[]")
}
