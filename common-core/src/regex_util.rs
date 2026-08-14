use regex::Regex;

/// Compiles a regex, panicking on invalid patterns (all internal patterns are
/// static literals; a panic here is a programming error, not user input).
pub fn compile_re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("invalid regex")
}

pub fn escape_regex(s: &str) -> String {
    regex::escape(s)
}

pub fn is_match(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}
