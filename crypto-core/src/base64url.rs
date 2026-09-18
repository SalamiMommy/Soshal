use base64::Engine;

pub fn base64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let clean = s.trim().trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(clean.as_bytes())
        .ok()
}

pub fn to_base64url(b64: &str) -> String {
    let mut out = String::with_capacity(b64.len());
    for b in b64.bytes() {
        match b {
            b'+' => out.push('-'),
            b'/' => out.push('_'),
            b'=' => {}
            _ => out.push(b as char),
        }
    }
    out
}

pub fn from_base64url(b64u: &str) -> String {
    let b64u = b64u.trim().trim_end_matches('=');
    let mut out = String::with_capacity(b64u.len() + 2);
    for b in b64u.bytes() {
        match b {
            b'-' => out.push('+'),
            b'_' => out.push('/'),
            _ => out.push(b as char),
        }
    }
    match out.len() % 4 {
        2 => out.push_str("=="),
        3 => out.push('='),
        _ => {}
    }
    out
}
