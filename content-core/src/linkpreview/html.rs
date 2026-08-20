use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::json_util::json_out;
use crate::url::is_valid_media_url;

/// Longest URL kept from an HTML scan.
#[doc(hidden)]
pub use super::urls::MAX_PREVIEW_URL_LENGTH;
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

fn scan_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::RegexBuilder::new(r"<(meta|link)[^>]*>|<title[^>]*>([^<]*)</title>")
            .case_insensitive(true)
            .size_limit(64 * 1024)
            .build()
            .unwrap_or_else(|_| compile_regex(MATCH_NOTHING_REGEX))
    })
}

#[derive(Clone, Copy, PartialEq)]
enum AttrKind {
    Property,
    Name,
    Content,
    Rel,
    Href,
}

impl AttrKind {
    fn from_prefix(rest: &str) -> Option<(AttrKind, usize)> {
        let bytes = rest.as_bytes();
        if bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"property") {
            Some((AttrKind::Property, 8))
        } else if bytes.len() >= 4 && bytes[..4].eq_ignore_ascii_case(b"name") {
            Some((AttrKind::Name, 4))
        } else if bytes.len() >= 7 && bytes[..7].eq_ignore_ascii_case(b"content") {
            Some((AttrKind::Content, 7))
        } else if bytes.len() >= 3 && bytes[..3].eq_ignore_ascii_case(b"rel") {
            Some((AttrKind::Rel, 3))
        } else if bytes.len() >= 4 && bytes[..4].eq_ignore_ascii_case(b"href") {
            Some((AttrKind::Href, 4))
        } else {
            None
        }
    }
}

struct TagEntry<'a> {
    kind: AttrKind,
    value: &'a str,
}

fn scan_entries(body: &str) -> Vec<TagEntry<'_>> {
    let bytes = body.as_bytes();
    let mut entries = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if body.is_char_boundary(i) {
            if let Some((kind, name_len)) = AttrKind::from_prefix(&body[i..]) {
                let eq = i + name_len;
                let open = eq + 1;
                if bytes.get(eq) == Some(&b'=')
                    && matches!(bytes.get(open), Some(&b'"') | Some(&b'\''))
                {
                    let mut end = open + 1;
                    while end < bytes.len() && bytes[end] != b'"' && bytes[end] != b'\'' {
                        end += 1;
                    }
                    entries.push(TagEntry {
                        kind,
                        value: &body[open + 1..end],
                    });
                }
            }
        }
        i += 1;
    }
    entries
}

fn match_attr_before_content<'a>(
    entries: &[TagEntry<'a>],
    kind: AttrKind,
    prop: &str,
) -> Option<&'a str> {
    for idx in (0..entries.len()).rev() {
        if entries[idx].kind != kind || !entries[idx].value.eq_ignore_ascii_case(prop) {
            continue;
        }
        let after = &entries[idx + 1..];
        if let Some(content) = after.iter().rev().find(|e| e.kind == AttrKind::Content) {
            return Some(content.value);
        }
    }
    None
}

fn match_content_before_attr<'a>(
    entries: &[TagEntry<'a>],
    kind: AttrKind,
    prop: &str,
) -> Option<&'a str> {
    for idx in (0..entries.len()).rev() {
        if entries[idx].kind != AttrKind::Content {
            continue;
        }
        let after = &entries[idx + 1..];
        if after
            .iter()
            .rev()
            .any(|e| e.kind == kind && e.value.eq_ignore_ascii_case(prop))
        {
            return Some(entries[idx].value);
        }
    }
    None
}

fn match_favicon<'a>(entries: &[TagEntry<'a>]) -> Option<&'a str> {
    for idx in (0..entries.len()).rev() {
        if entries[idx].kind != AttrKind::Rel {
            continue;
        }
        let rel = entries[idx].value;
        if !(rel.eq_ignore_ascii_case("icon") || rel.eq_ignore_ascii_case("shortcut icon")) {
            continue;
        }
        let after = &entries[idx + 1..];
        if let Some(href) = after.iter().rev().find(|e| e.kind == AttrKind::Href) {
            return Some(href.value);
        }
    }
    None
}

fn shape_results<'a>(entries: &[TagEntry<'a>], prop: &str) -> [Option<&'a str>; 4] {
    [
        match_attr_before_content(entries, AttrKind::Property, prop),
        match_content_before_attr(entries, AttrKind::Property, prop),
        match_attr_before_content(entries, AttrKind::Name, prop),
        match_content_before_attr(entries, AttrKind::Name, prop),
    ]
}

#[derive(Clone, Copy, Default)]
struct ShapeSet<'a> {
    shapes: [Option<&'a str>; 4],
}

impl<'a> ShapeSet<'a> {
    fn record(&mut self, results: [Option<&'a str>; 4]) {
        for (slot, result) in self.shapes.iter_mut().zip(results.iter()) {
            if slot.is_none() {
                *slot = *result;
            }
        }
    }

    fn winner(&self) -> Option<&'a str> {
        self.shapes.iter().find_map(|s| *s)
    }
}

struct LinkScan<'a> {
    og_title: ShapeSet<'a>,
    og_description: ShapeSet<'a>,
    description: ShapeSet<'a>,
    og_image: ShapeSet<'a>,
    twitter_image: ShapeSet<'a>,
    title_tag: Option<&'a str>,
    favicon: Option<&'a str>,
}

fn scan_html(html: &str) -> LinkScan<'_> {
    let re = scan_regex();
    let mut scan = LinkScan {
        og_title: ShapeSet::default(),
        og_description: ShapeSet::default(),
        description: ShapeSet::default(),
        og_image: ShapeSet::default(),
        twitter_image: ShapeSet::default(),
        title_tag: None,
        favicon: None,
    };
    for caps in re.captures_iter(html) {
        if let Some(tag_name) = caps.get(1) {
            let tag = caps.get(0).unwrap().as_str();
            let name = tag_name.as_str();
            let prefix = if name.eq_ignore_ascii_case("meta") {
                5
            } else {
                6
            };
            let body = &tag[prefix..tag.len() - 1];
            let entries = scan_entries(body);
            if name.eq_ignore_ascii_case("meta") {
                scan.og_title.record(shape_results(&entries, "og:title"));
                scan.og_description
                    .record(shape_results(&entries, "og:description"));
                scan.description
                    .record(shape_results(&entries, "description"));
                scan.og_image.record(shape_results(&entries, "og:image"));
                scan.twitter_image
                    .record(shape_results(&entries, "twitter:image"));
            } else if scan.favicon.is_none() {
                scan.favicon = match_favicon(&entries);
            }
        } else if let Some(title) = caps.get(2) {
            if scan.title_tag.is_none() {
                scan.title_tag = Some(title.as_str());
            }
        }
    }
    scan
}

fn resolve_favicon(href: Option<&str>, page_url: &str) -> Option<String> {
    let base = url::Url::parse(page_url).ok()?;
    if let Some(href) = href {
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
    base.join("/favicon.ico")
        .ok()
        .map(|u| u.to_string())
        .filter(|s| is_valid_media_url(s))
}

pub fn extract_favicon(html: &str, page_url: &str) -> Option<String> {
    resolve_favicon(scan_html(html).favicon, page_url)
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
    let scan = scan_html(html);
    let og_title = scan.og_title.winner();
    let title = match og_title {
        Some(v) if v.len() <= MAX_PREVIEW_URL_LENGTH => decode_html_entities(v),
        _ => match scan.title_tag {
            Some(t) => {
                let trimmed = t.trim();
                if !trimmed.is_empty() && trimmed.len() <= MAX_PREVIEW_URL_LENGTH {
                    decode_html_entities(trimmed)
                } else {
                    url_str.to_string()
                }
            }
            None => url_str.to_string(),
        },
    };
    let og_description = scan.og_description.winner();
    let description = match og_description {
        Some(v) if v.len() <= MAX_PREVIEW_URL_LENGTH => decode_html_entities(v),
        _ => match scan.description.winner() {
            Some(v) if v.len() <= MAX_PREVIEW_URL_LENGTH => decode_html_entities(v),
            _ => String::new(),
        },
    };
    if description.len() > MAX_PREVIEW_URL_LENGTH {
        return None;
    }
    let og_image = scan.og_image.winner();
    let twitter_image = scan.twitter_image.winner();
    let image = match og_image {
        Some(v) if v.len() <= MAX_PREVIEW_URL_LENGTH => Some(decode_html_entities(v)),
        _ => match twitter_image {
            Some(v) if v.len() <= MAX_PREVIEW_URL_LENGTH => Some(decode_html_entities(v)),
            _ => None,
        },
    };
    let image = image.filter(|u| is_valid_media_url(u));
    let favicon = resolve_favicon(scan.favicon, url_str);
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
