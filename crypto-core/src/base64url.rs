pub fn to_base64url(b64: &str) -> String {
    b64.replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

pub fn from_base64url(b64u: &str) -> String {
    let mut result = b64u.replace('-', "+").replace('_', "/");
    match result.len() % 4 {
        2 => result.push_str("=="),
        3 => result.push('='),
        _ => {}
    }
    result
}
