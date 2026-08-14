//! LNURL and LUD16 helper functions.

/// Parses lud16 address (user@domain) into the well-known HTTP URL path.
///
/// # Deprecated
///
/// Use [`parse_lud16_url_secure`] instead. This variant does not validate the
/// user-part allowlist and is susceptible to path-traversal if a caller
/// supplies attacker-controlled input (e.g. `../admin@domain` yields
/// `https://domain/.well-known/lnurlp/../admin`).
#[deprecated(since = "0.1.0", note = "use parse_lud16_url_secure instead")]
pub fn parse_lud16_url(lud16: &str) -> Result<(String, String, String), String> {
    let (user, domain) = lud16.rsplit_once('@').ok_or("invalid lud16 format")?;
    if user.is_empty() || domain.is_empty() {
        return Err("invalid lud16 user or domain".into());
    }
    let url = format!("https://{}/.well-known/lnurlp/{}", domain, user);
    Ok((user.to_string(), domain.to_string(), url))
}

/// Parses lud16 with the strict user-part allowlist used before embedding the
/// value in a URL path. Without the allowlist a crafted value like
/// `../admin@domain` produces `https://domain/.well-known/lnurlp/../admin`,
/// potentially reaching unintended server endpoints.
pub fn parse_lud16_url_secure(lud16: &str) -> Result<(String, String, String), String> {
    let (user, domain) = lud16.rsplit_once('@').ok_or("invalid lud16")?;
    if user.is_empty() || domain.is_empty() {
        return Err("invalid lud16".into());
    }
    if !user
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return Err("invalid lud16: user part contains disallowed characters".into());
    }
    if user.len() > 64 {
        return Err("invalid lud16: user part too long".into());
    }
    let url = format!("https://{}/.well-known/lnurlp/{}", domain, user);
    Ok((user.to_string(), domain.to_string(), url))
}

/// Extracts the host string from a URL.
pub fn host_of(url_str: &str) -> Result<String, String> {
    url::Url::parse(url_str)
        .map_err(|e| format!("invalid url: {e}"))?
        .host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| "url has no host".into())
}
