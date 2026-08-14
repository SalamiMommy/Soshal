use serde::Deserialize;

use crate::json_util::{json_in, json_out};

/// Longest URL kept in a meta-tag scan.
#[doc(hidden)]
pub const MAX_PREVIEW_URL_LENGTH: usize = 8 * 1024;
/// Cap on scanned meta tags.
#[doc(hidden)]
pub const MAX_IMETA_TAGS: usize = 100_000;

pub fn extract_imeta_video_urls(tags: &[Vec<String>]) -> Vec<String> {
    let mut videos = Vec::new();
    let scan_limit = tags.len().min(MAX_IMETA_TAGS);
    for tag in &tags[..scan_limit] {
        if tag.is_empty() || tag[0] != "imeta" {
            continue;
        }
        let mut url_val: Option<String> = None;
        let mut m_val: Option<String> = None;
        for entry in tag {
            if let Some(rest) = entry.strip_prefix("url=") {
                if rest.len() <= MAX_PREVIEW_URL_LENGTH {
                    url_val = Some(rest.to_string());
                }
            } else if let Some(rest) = entry.strip_prefix("m=") {
                if rest.len() <= 100 {
                    m_val = Some(rest.to_string());
                }
            }
        }
        if let (Some(url), Some(m)) = (url_val, m_val) {
            if m.contains("video") || m.contains("gif") {
                videos.push(url);
            }
        }
    }
    videos
}

#[derive(Deserialize)]
struct ImetaInput {
    tags: Vec<Vec<String>>,
}

pub fn extract_imeta_video_urls_json(input: &str) -> String {
    let parsed = json_in(input, ImetaInput { tags: Vec::new() });
    if parsed.tags.len() > MAX_IMETA_TAGS {
        return "[]".to_string();
    }
    let out = extract_imeta_video_urls(&parsed.tags);
    json_out(&out, "[]")
}
