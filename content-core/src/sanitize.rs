use regex::{Regex, RegexBuilder};
use std::sync::OnceLock;

use crate::regex_util::compile_re;

const MATCH_NOTHING_REGEX: &str = "a^";

// ─── Error / log message sanitization ─────────────────────────────────

/// Returns compiled error sanitization regexes (with backtracking size limits).
fn error_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            static FALLBACK_RE: OnceLock<Regex> = OnceLock::new();
            let build = |pat: &str| -> Regex {
                RegexBuilder::new(pat)
                    .size_limit(1_000_000)
                    .build()
                    .unwrap_or_else(|_| {
                        FALLBACK_RE
                            .get_or_init(|| {
                                Regex::new(MATCH_NOTHING_REGEX)
                                    .expect("MATCH_NOTHING_REGEX must compile")
                            })
                            .clone()
                    })
            };
            vec![
                build(r"\bnsec1[ac-hj-np-z02-9]{40,65}\b"),
                build(
                    r"(?i)(?:nsec1|private_?key|secret_?key|sk|privkey|seckey)[=:]\s*[a-f0-9]{64}",
                ),
                build(r"(?i)(?:api[_-]?key|token|secret|password|passwd)[:\s]+\S{8,}"),
                build(r"/etc/(?:passwd|shadow|hosts|ssh)\b"),
                build(r"/proc/self/environ\b"),
                build(r"(?:mongodb|mysql|postgres|sqlite)://[^\s]+"),
                build(r"\?[^&\s]+"),
            ]
        })
        .as_slice()
}

/// Returns compiled log sanitization regexes (with backtracking size limits).
fn log_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            let build = |pat: &str| -> Regex {
                RegexBuilder::new(pat)
                    .size_limit(1_000_000)
                    .build()
                    .unwrap_or_else(|_| {
                        regex::Regex::new(MATCH_NOTHING_REGEX)
                            .expect("MATCH_NOTHING_REGEX must compile")
                    })
            };
            vec![
                build(r"\b[a-fA-F0-9]{64}\b"),
                build(r"\bnsec1[ac-hj-np-z02-9]{40,65}"),
                build(r"\b0x[a-fA-F0-9]{64}\b"),
            ]
        })
        .as_slice()
}

/// Redacts sensitive patterns from an error message.
/// Replaces matches with `[REDACTED]` and truncates to 500 chars.
pub fn sanitize_error_message(msg: &str) -> String {
    let mut sanitized = std::borrow::Cow::Borrowed(msg);
    for pattern in error_patterns() {
        if let std::borrow::Cow::Owned(replaced) = pattern.replace_all(&sanitized, "[REDACTED]") {
            sanitized = std::borrow::Cow::Owned(replaced);
        }
    }
    const MAX_LENGTH: usize = 500;
    if sanitized.len() > MAX_LENGTH {
        let mut s = sanitized.into_owned();
        s.truncate(MAX_LENGTH);
        s.push_str("... [truncated]");
        return s;
    }
    sanitized.into_owned()
}

/// Redacts private key patterns from log messages.
/// Replaces 64-char hex, nsec bech32, and 0x-prefixed hex with `[REDACTED_KEY]`.
pub fn sanitize_log_message(msg: &str) -> String {
    let mut sanitized = std::borrow::Cow::Borrowed(msg);
    for pattern in log_patterns() {
        if let std::borrow::Cow::Owned(replaced) = pattern.replace_all(&sanitized, "[REDACTED_KEY]")
        {
            sanitized = std::borrow::Cow::Owned(replaced);
        }
    }
    sanitized.into_owned()
}

/// Strips HTML tags, normalizes whitespace, and truncates notification content.
/// Mirrors TS `sanitizeNotifContent` in NotificationService.ts.
pub fn sanitize_notif_content(raw: &str, max_len: usize) -> String {
    static RE_TAGS: OnceLock<Regex> = OnceLock::new();
    let re = RE_TAGS.get_or_init(|| {
        RegexBuilder::new(r"<[^>]*>")
            .size_limit(64 * 1024)
            .build()
            .unwrap_or_else(|_| Regex::new(MATCH_NOTHING_REGEX).expect("compile nothing"))
    });
    let no_tags = re.replace_all(raw, "");
    static RE_WS: OnceLock<Regex> = OnceLock::new();
    let re_ws = RE_WS.get_or_init(|| {
        RegexBuilder::new(r"\s+")
            .size_limit(64 * 1024)
            .build()
            .unwrap_or_else(|_| Regex::new(MATCH_NOTHING_REGEX).expect("compile nothing"))
    });
    let normalized = re_ws.replace_all(&no_tags, " ");
    let trimmed = normalized.trim();
    if trimmed.len() > max_len {
        let boundary = trimmed.floor_char_boundary(max_len);
        trimmed[..boundary].to_string()
    } else {
        trimmed.to_string()
    }
}

// ─── Security audit sanitization ──────────────────────────────────────

/// Sensitive key sub-strings that trigger value redaction.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "pin",
    "key",
    "secret",
    "token",
    "privateKey",
    "seed",
];

/// Returns a compiled regex that matches forward-slash–prefixed hex strings
/// of 64–128 characters (pubkeys, event ids, signatures in log paths).
fn hex_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| compile_re(r"/[a-f0-9]{64,128}"))
}

/// Returns a compiled regex that matches base64-encoded blobs of 32+
/// characters with optional padding.
fn base64_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| compile_re(r"\b[A-Za-z0-9+/]{32,}={0,2}"))
}

/// Redacts sensitive values (password, pin, key, secret, token, privateKey,
/// seed) from a context JSON object. Returns the redacted JSON string, or
/// `None` if the input is empty, null, or not a JSON object.
pub fn sanitize_context(input_json: &str) -> Option<String> {
    if input_json.is_empty() || input_json == "null" {
        return None;
    }

    let mut obj: serde_json::Value = serde_json::from_str(input_json).ok()?;

    if let Some(map) = obj.as_object_mut() {
        for (key, value) in map.iter_mut() {
            let lower = key.to_lowercase();
            if SENSITIVE_KEYS.iter().any(|s| lower.contains(s)) {
                *value = serde_json::Value::String("[REDACTED]".to_string());
            }
        }
        serde_json::to_string(&obj).ok()
    } else {
        None
    }
}

/// Redacts sensitive patterns (hex keys, base64 blobs) from a detail string.
pub fn sanitize_details(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let result = hex_path_regex().replace_all(input, "/[HEX]");
    let result = base64_regex().replace_all(&result, "[BASE64]");
    result.to_string()
}

// ─── Security log sanitization kernel ─────────────────────────────────

static SENSITIVE_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();

fn get_patterns() -> &'static Vec<(Regex, &'static str)> {
    SENSITIVE_PATTERNS.get_or_init(|| {
        vec![
            (
                compile_re(r"(?i)\bnsec1[ac-hj-np-z02-9]{40,65}\b"),
                "[REDACTED_NSEC]",
            ),
            (
                compile_re(
                    r"(?i)\b(privkey|nsec|secret|private_key|privateKey)\s*[:=]\s*([a-f0-9]{64})\b",
                ),
                "$1:[REDACTED_KEY]",
            ),
            (
                compile_re(r"(?i)bearer\s+[a-z0-9\-\._~\+\/]+=*"),
                "Bearer [REDACTED_TOKEN]",
            ),
            (
                compile_re(
                    r"\b(127\.0\.0\.1|10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})\b",
                ),
                "[REDACTED_IP]",
            ),
        ]
    })
}

/// Scrubs sensitive keys, tokens, and local IP addresses from log messages.
pub fn scrub_sensitive_data(text: &str) -> String {
    let mut sanitized = text.to_string();
    for (regex, replacement) in get_patterns().iter() {
        sanitized = regex.replace_all(&sanitized, *replacement).to_string();
    }
    sanitized
}
