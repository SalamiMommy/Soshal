use crate::{MAX_FTS5_TERMS, MAX_FTS5_TERM_LEN};
use serde::Deserialize;
use soshal_common_core::json_util::json_in;
use std::collections::HashSet;

/// Input for format_fts5_query.
#[derive(Deserialize)]
pub struct FormatFts5Input {
    pub query: String,
}

/// Sanitizes a single FTS5 term.
pub fn sanitize_fts5_term(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if raw.chars().all(|c| c.is_alphanumeric()) {
        if raw.chars().count() > MAX_FTS5_TERM_LEN {
            return None;
        }
        return Some(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        }
    }
    if out.is_empty() || out.chars().count() > MAX_FTS5_TERM_LEN {
        return None;
    }
    Some(out)
}

/// Formats a raw search query as an FTS5 query string. Input is capped (the
/// JSON API also caps at 4096) so the direct path cannot be used for an
/// unbounded query.
pub fn format_fts5_query(query: &str) -> String {
    if query.len() > 4096 {
        return String::new();
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(trimmed.len() + 16);
    let mut seen: HashSet<(Option<String>, String)> = HashSet::new();
    let mut count = 0;
    for word in trimmed.split_whitespace() {
        let (field, term) = match word.find(':') {
            Some(idx) => {
                let f: String = word[..idx]
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect();
                (if f.is_empty() { None } else { Some(f) }, &word[idx + 1..])
            }
            None => (None, word),
        };
        let Some(t) = sanitize_fts5_term(term) else {
            continue;
        };
        let entry = (field, t);
        if !seen.insert(entry.clone()) {
            continue;
        }
        if count > 0 {
            out.push_str(" AND ");
        }
        match &entry.0 {
            Some(f) => {
                out.push_str(f);
                out.push_str(":\"");
                out.push_str(&entry.1);
                out.push_str("\"*");
            }
            None => {
                out.push('"');
                out.push_str(&entry.1);
                out.push_str("\"*");
            }
        }
        count += 1;
        if count >= MAX_FTS5_TERMS {
            break;
        }
    }
    out
}

/// JSON-based public API: formats a raw search query as an FTS5 query string.
/// Input: `{"query":"..."}`, Output: the FTS5 query string (empty on error).
pub fn format_fts5_query_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<FormatFts5Input>>(input_json, None) else {
        return String::new();
    };
    if input.query.len() > 4096 {
        return String::new();
    }
    format_fts5_query(&input.query)
}

/// Returns the SQL statement to execute an FTS5 index optimization / compaction pass.
pub fn optimize_fts5_index_query() -> &'static str {
    "INSERT INTO posts_fts(posts_fts) VALUES('optimize');"
}
