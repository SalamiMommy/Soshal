//! HTML entity decode utility.

/// Decodes a single named or numeric HTML entity to its character, or `None`
/// when the entity is unknown or malformed (caller re-emits it verbatim).
fn decode_one(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00a0}'),
        "ndash" => Some('\u{2013}'),
        "mdash" => Some('\u{2014}'),
        "lsquo" => Some('\u{2018}'),
        "rsquo" => Some('\u{2019}'),
        "ldquo" => Some('\u{201c}'),
        "rdquo" => Some('\u{201d}'),
        "bull" => Some('\u{2022}'),
        "hellip" => Some('\u{2026}'),
        "copy" => Some('\u{00a9}'),
        "reg" => Some('\u{00ae}'),
        "trade" => Some('\u{2122}'),
        "euro" => Some('\u{20ac}'),
        "pound" => Some('\u{00a3}'),
        "yen" => Some('\u{00a5}'),
        "cent" => Some('\u{00a2}'),
        "sect" => Some('\u{00a7}'),
        "deg" => Some('\u{00b0}'),
        "plusmn" => Some('\u{00b1}'),
        "sup2" => Some('\u{00b2}'),
        "sup3" => Some('\u{00b3}'),
        "frac14" => Some('\u{00bc}'),
        "frac12" => Some('\u{00bd}'),
        "frac34" => Some('\u{00be}'),
        "times" => Some('\u{00d7}'),
        "divide" => Some('\u{00f7}'),
        _ => {
            if let Some(num) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(num, 16).ok().and_then(char::from_u32)
            } else if let Some(num) = entity.strip_prefix('#') {
                num.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

/// Decodes common HTML entities to their character equivalents.
///
use std::borrow::Cow;

/// Single pass over the input: every `&` scans at most `MAX_ENTITY_LEN` bytes
/// ahead for the closing `;` (entity names are at most ~10 chars), so
/// adversarial input like a megabyte of `&` runs in linear time instead of
/// O(n²) rescans.
pub fn decode_html_entities<'a>(s: &'a str) -> Cow<'a, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    /// Longest lookahead considered for an entity. A `;` beyond this window
    /// is not part of an entity and `&` is treated as a literal.
    const MAX_ENTITY_LEN: usize = 24;
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            let Some(rel) = bytes[i + 1..]
                .iter()
                .take(MAX_ENTITY_LEN)
                .position(|&b| b == b';')
            else {
                out.push('&');
                i += 1;
                continue;
            };
            let entity = &s[i + 1..i + 1 + rel];
            match decode_one(entity) {
                Some(c) => {
                    out.push(c);
                    i += rel + 2;
                }
                None => {
                    out.push('&');
                    out.push_str(entity);
                    out.push(';');
                    i += rel + 2;
                }
            }
            continue;
        }
        if bytes[i] < 128 {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let ch = match s[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    Cow::Owned(out)
}

/// Decodes ASCII numeric HTML entities (`&#NNN;` / `&#xHH;`) to their
/// lowercase character equivalent (case-normalized for scheme detection).
/// Named entities and non-ASCII values are left verbatim. Sanitizers that
/// only need numeric ASCII entities (e.g. the glitter HTML sanitizer) share
/// this implementation.
pub fn decode_ascii_entities<'a>(s: &'a str) -> Cow<'a, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' && chars.peek() == Some(&'#') {
            chars.next();
            let is_hex = if chars.peek() == Some(&'x') || chars.peek() == Some(&'X') {
                chars.next();
                true
            } else {
                false
            };
            let mut buf = [0u8; 16];
            let mut buf_len = 0;
            while let Some(&nc) = chars.peek() {
                if (is_hex && nc.is_ascii_hexdigit()) || (!is_hex && nc.is_ascii_digit()) {
                    if buf_len < buf.len() {
                        buf[buf_len] = nc as u8;
                        buf_len += 1;
                    }
                    chars.next();
                } else {
                    break;
                }
            }
            if buf_len > 0 && chars.peek() == Some(&';') {
                chars.next();
            }
            let num_str = std::str::from_utf8(&buf[..buf_len]).unwrap_or("");
            if !num_str.is_empty() {
                let parsed_val = if is_hex {
                    u32::from_str_radix(num_str, 16)
                } else {
                    num_str.parse::<u32>()
                };
                if let Ok(val) = parsed_val {
                    if val > 0 && val < 128 {
                        if let Some(decoded_char) = char::from_u32(val) {
                            result.push(decoded_char.to_ascii_lowercase());
                            continue;
                        }
                    }
                }
            }
            result.push('&');
            result.push('#');
            if is_hex {
                result.push('x');
            }
            result.push_str(num_str);
            if buf_len > 0 {
                result.push(';');
            }
        } else {
            result.push(c);
        }
    }
    Cow::Owned(result)
}
