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
