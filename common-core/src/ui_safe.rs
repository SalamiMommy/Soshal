//! Helpers for safely rendering untrusted data in the WASM UI.

/// Validates a user-supplied CSS color: hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`)
/// or functional `hsl(...)`/`hsla(...)` with numeric 0-360 hue and 0-100% components.
/// Blocks CSS property injection and tracking via `url(...)`.
pub fn is_valid_css_color(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    if let Some(hex) = s.strip_prefix('#') {
        return (hex.len() == 3 || hex.len() == 4 || hex.len() == 6 || hex.len() == 8)
            && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let rest = if let Some(stripped) = s.strip_prefix("hsla") {
        stripped
    } else if let Some(stripped) = s.strip_prefix("hsl") {
        stripped
    } else {
        return false;
    };
    parse_hsl(rest).is_some()
}

fn parse_hsl(rest: &str) -> Option<()> {
    let (body, close) = rest.split_once('(')?;
    if !body.is_empty() || !close.ends_with(')') {
        return None;
    }
    let inner = &close[..close.len() - 1];
    let mut parts = inner.split(',').map(str::trim);
    let hue: f64 = parts.next()?.trim_end_matches("deg").parse().ok()?;
    if !(0.0..=360.0).contains(&hue) {
        return None;
    }
    for _ in 0..2 {
        let p = parts.next()?;
        let v: f64 = p.trim_end_matches('%').parse().ok()?;
        if !(0.0..=100.0).contains(&v) {
            return None;
        }
    }
    if let Some(alpha) = parts.next() {
        let a: f64 = alpha.trim_end_matches('%').parse().ok()?;
        if !(0.0..=100.0).contains(&a) {
            return None;
        }
    }
    if parts.next().is_some() {
        return None;
    }
    Some(())
}

/// Truncates a hex pubkey for display without ever panicking on multi-byte UTF-8
/// boundaries or short inputs. Returns at most `n` characters.
pub fn short_pk(pk: &str, n: usize) -> String {
    if pk.is_empty() || n == 0 {
        return String::new();
    }
    if pk.len() <= n {
        return pk.to_string();
    }
    let mut end = n;
    while end > 0 && !pk.is_char_boundary(end) {
        end -= 1;
    }
    pk[..end].to_string()
}

/// Panic-free prefix truncation: returns at most `n` bytes of `s`, walking back
/// to a char boundary when the cut point splits multi-byte UTF-8. Never panics,
/// even when `s` is shorter than `n`.
pub fn truncate_str(s: &str, n: usize) -> &str {
    if s.is_empty() || n == 0 {
        return "";
    }
    if s.len() <= n {
        return s;
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Encodes `s` as a double-quoted JavaScript string literal. Uses JSON escaping,
/// so quotes, backslashes, newlines and control characters can never break out
/// of the literal. Safe to interpolate into `document::eval` strings.
pub fn js_string_literal(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
