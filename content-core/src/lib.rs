//! Content processing core: hashtag/mention/URL extraction, safe-JSON and
//! sanitization, link previews, custom profiles, story content, FTS5 helpers.

pub mod ast_parser;
pub mod chunk;
pub mod compress;
pub mod custom_profile;
pub mod diff;
pub mod entities;
pub mod extension;
pub mod forcelayout;
pub mod fts5;
pub mod hashtag;
pub mod language;
pub mod language_id;
pub mod linkpreview;
pub mod markdown;
pub mod mention;
pub mod normalize;
pub mod safe_json;
pub mod sanitize;
pub mod stories;
pub mod tags;

pub use diff::{
    compute_diff_summary, compute_line_diff, compute_word_diff, render_unified_diff, DiffItem,
    DiffSummary, DiffTag,
};
pub use language::{detect_content_language, ContentLanguage};
pub use language_id::{
    canonicalize_bcp47, extract_bcp47_from_nostr_tags, is_valid_bcp47, matches_language_filter,
    parse_language_tag, CanonicalLanguageTag,
};
pub use markdown::{extract_markdown_metadata, parse_markdown_spans, MarkdownMetadata};
pub use normalize::{normalize_nip05_handle, normalize_tag, to_nfc, to_nfd, to_nfkc, to_nfkd};

// Generic utils shared across cores; re-exported from soshal-common-core so
// existing `soshal_content_core::{json_util,url,mime,format,ui_safe,regex_util}`
// call sites keep compiling.
pub use soshal_common_core::{format, json_util, mime, regex_util, ui_safe, url};
