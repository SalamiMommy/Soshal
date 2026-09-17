use serde::Deserialize;

use crate::json_util::{json_in, json_out};

/// Longest URL truncated for preview scanning.
#[doc(hidden)]
pub const MAX_PREVIEW_URL_LENGTH: usize = 8 * 1024;

/// Extract candidate URLs from plain text.
#[doc(hidden)]
pub fn extract_urls(text: &str) -> Vec<String> {
    soshal_common_core::url::extract(text)
}

#[derive(Deserialize)]
struct ExtractUrlsInput {
    text: String,
}

pub fn extract_urls_json(input: &str) -> String {
    if input.len() > 1024 * 1024 {
        return "[]".to_string();
    }
    let parsed = json_in(
        input,
        ExtractUrlsInput {
            text: String::new(),
        },
    );
    let out = extract_urls(&parsed.text);
    json_out(&out, "[]")
}
