use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::json_util::json_out;
use crate::url::is_valid_media_url;

/// Longest URL kept from an HTML scan.
#[doc(hidden)]
pub const MAX_PREVIEW_URL_LENGTH: usize = 8 * 1024;
/// Cap on raw HTML scanned.
#[doc(hidden)]
pub const MAX_HTML_LENGTH: usize = 5 * 1024 * 1024;
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

fn decode_html_entities(text: &str) -> String {
    crate::entities::decode_html_entities(text).into_owned()
}

fn get_meta_regexes(property: &str) -> &'static [Regex] {
    static OG_TITLE: OnceLock<Vec<Regex>> = OnceLock::new();
    static OG_DESC: OnceLock<Vec<Regex>> = OnceLock::new();
    static DESC: OnceLock<Vec<Regex>> = OnceLock::new();
    static OG_IMAGE: OnceLock<Vec<Regex>> = OnceLock::new();
    static TWITTER_IMAGE: OnceLock<Vec<Regex>> = OnceLock::new();
    static FALLBACK: OnceLock<Vec<Regex>> = OnceLock::new();

    let build_patterns = |prop: &str| {
        let escaped = regex::escape(prop);
        let patterns = [
            format!(
                r#"<meta[^>]+property=["']{}["'][^>]+content=["']([^"']*)["']"#,
                escaped
            ),
            format!(
                r#"<meta[^>]+content=["']([^"']*)["'][^>]+property=["']{}["']"#,
                escaped
            ),
            format!(
                r#"<meta[^>]+name=["']{}["'][^>]+content=["']([^"']*)["']"#,
                escaped
            ),
            format!(
                r#"<meta[^>]+content=["']([^"']*)["'][^>]+name=["']{}["']"#,
                escaped
            ),
        ];
        patterns
            .iter()
            .filter_map(|p| {
                regex::RegexBuilder::new(p)
                    .case_insensitive(true)
                    .size_limit(64 * 1024)
                    .dfa_size_limit(64 * 1024)
                    .build()
                    .ok()
            })
            .collect::<Vec<_>>()
    };

    match property {
        "og:title" => OG_TITLE
            .get_or_init(|| build_patterns("og:title"))
            .as_slice(),
        "og:description" => OG_DESC
            .get_or_init(|| build_patterns("og:description"))
            .as_slice(),
        "description" => DESC
            .get_or_init(|| build_patterns("description"))
            .as_slice(),
        "og:image" => OG_IMAGE
            .get_or_init(|| build_patterns("og:image"))
            .as_slice(),
        "twitter:image" => TWITTER_IMAGE
            .get_or_init(|| build_patterns("twitter:image"))
            .as_slice(),
        _ => FALLBACK.get_or_init(|| build_patterns(property)).as_slice(),
    }
}

fn extract_meta(html: &str, property: &str) -> Option<String> {
    let regexes = get_meta_regexes(property);
    for re in regexes {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                let value = m.as_str();
                if value.len() > MAX_PREVIEW_URL_LENGTH {
                    return None;
                }
                return Some(decode_html_entities(value));
            }
        }
    }
    None
}

fn title_tag_regex() -> &'static Regex {
    static TITLE_RE: OnceLock<Regex> = OnceLock::new();
    TITLE_RE.get_or_init(|| {
        regex::RegexBuilder::new(r"<title[^>]*>([^<]*)</title>")
            .case_insensitive(true)
            .size_limit(64 * 1024)
            .build()
            .unwrap_or_else(|_| compile_regex(MATCH_NOTHING_REGEX))
    })
}

fn extract_html_tag(html: &str, tag: &str) -> Option<String> {
    if tag.eq_ignore_ascii_case("title") {
        let re = title_tag_regex();
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                let trimmed = m.as_str().trim();
                if !trimmed.is_empty() && trimmed.len() <= MAX_PREVIEW_URL_LENGTH {
                    return Some(decode_html_entities(trimmed));
                }
            }
        }
        return None;
    }
    let t = regex::escape(tag);
    let pattern = format!(r"<{}[^>]*>([^<]*)</{t}>", t);
    if let Ok(re) = regex::RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .size_limit(64 * 1024)
        .build()
    {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                let trimmed = m.as_str().trim();
                if !trimmed.is_empty() && trimmed.len() <= MAX_PREVIEW_URL_LENGTH {
                    return Some(decode_html_entities(trimmed));
                }
            }
        }
    }
    None
}

fn favicon_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::RegexBuilder::new(
            r#"<link[^>]+rel=["'](?:shortcut )?icon["'][^>]+href=["']([^"']*)["']"#,
        )
        .case_insensitive(true)
        .size_limit(64 * 1024)
        .build()
        .unwrap_or_else(|_| compile_regex(MATCH_NOTHING_REGEX))
    })
}

pub fn extract_favicon(html: &str, page_url: &str) -> Option<String> {
    let re = favicon_regex();
    let base = url::Url::parse(page_url).ok()?;
    if let Some(caps) = re.captures(html) {
        if let Some(m) = caps.get(1) {
            let href = m.as_str();
            if href.len() > MAX_PREVIEW_URL_LENGTH {
                return None;
            }
            if let Ok(resolved) = base.join(href) {
                let s = resolved.to_string();
                // Only return renderable http(s) URLs; base.join("//x") and
                // javascript:/data: hrefs must never survive.
                return is_valid_media_url(&s).then_some(s);
            }
        }
    }
    base.join("/favicon.ico")
        .ok()
        .map(|u| u.to_string())
        .filter(|s| is_valid_media_url(s))
}

#[derive(Serialize, Deserialize)]
pub struct LinkPreviewOut {
    /// Page title.
    pub title: String,
    /// Page description.
    pub description: String,
    /// OpenGraph image URL.
    pub image: Option<String>,
    /// Site favicon URL.
    pub favicon: Option<String>,
    /// Presentable domain.
    pub domain: String,
}

#[derive(Deserialize)]
struct ParseLinkPreviewInput {
    html: String,
    url: String,
}

/// Parse HTML into a link preview.
#[doc(hidden)]
pub fn parse_link_preview_html(html: &str, url_str: &str) -> Option<LinkPreviewOut> {
    if html.len() > MAX_HTML_LENGTH {
        return None;
    }
    let title = extract_meta(html, "og:title")
        .or_else(|| extract_html_tag(html, "title"))
        .unwrap_or_else(|| url_str.to_string());
    let description = extract_meta(html, "og:description")
        .or_else(|| extract_meta(html, "description"))
        .unwrap_or_default();
    if description.len() > MAX_PREVIEW_URL_LENGTH {
        return None;
    }
    let image = extract_meta(html, "og:image")
        .or_else(|| extract_meta(html, "twitter:image"))
        .filter(|u| is_valid_media_url(u));
    let favicon = extract_favicon(html, url_str);
    let domain = url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_string();
    Some(LinkPreviewOut {
        title,
        description,
        image,
        favicon,
        domain,
    })
}

pub fn parse_link_preview_html_json(input: &str) -> String {
    let parsed: ParseLinkPreviewInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return "null".to_string(),
    };
    let out = parse_link_preview_html(&parsed.html, &parsed.url);
    json_out(&out, "null")
}
