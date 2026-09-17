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

/// Validates and extracts the `(user, domain)` slices of a LUD16 address without allocation.
pub fn validate_lud16_parts(lud16: &str) -> Result<(&str, &str), String> {
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
    if user.starts_with('.') || user.ends_with('.') || user.contains("..") {
        return Err(
            "invalid lud16: user part cannot start or end with '.' or contain consecutive dots"
                .into(),
        );
    }
    if !domain
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b':')
    {
        return Err("invalid lud16: domain contains disallowed characters".into());
    }
    if domain.len() > 255
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
    {
        return Err("invalid lud16: domain invalid".into());
    }
    Ok((user, domain))
}

/// Parses lud16 with the strict user-part allowlist used before embedding the
/// value in a URL path. Without the allowlist a crafted value like
/// `../admin@domain` produces `https://domain/.well-known/lnurlp/../admin`,
/// potentially reaching unintended server endpoints.
pub fn parse_lud16_url_secure(lud16: &str) -> Result<(String, String, String), String> {
    let (user, domain) = validate_lud16_parts(lud16)?;
    let url = format!("https://{}/.well-known/lnurlp/{}", domain, user);
    if !soshal_common_core::url::is_valid_media_url(&url) {
        return Err("invalid lud16: unsafe destination domain or URL".into());
    }
    Ok((user.to_string(), domain.to_string(), url))
}

/// Extracts the host string from a URL.
pub fn host_of(url_str: &str) -> Result<String, String> {
    soshal_common_core::url::domain(url_str).ok_or_else(|| "url has no host".into())
}

#[cfg(test)]
mod tests {
    #[allow(deprecated)]
    use super::parse_lud16_url;
    use super::{host_of, parse_lud16_url_secure};

    #[test]
    fn valid_lud16_parses() {
        let (user, domain, url) = parse_lud16_url_secure("alice@example.com").unwrap();
        assert_eq!(user, "alice");
        assert_eq!(domain, "example.com");
        assert_eq!(url, "https://example.com/.well-known/lnurlp/alice");
        let (u, _, _) = parse_lud16_url_secure("bob-1_x.y@sub.domain.org").unwrap();
        assert_eq!(u, "bob-1_x.y");
    }

    #[test]
    fn lud16_url_is_https_only() {
        for lud16 in ["alice@example.com", "bob@sub.domain.org"] {
            assert!(
                parse_lud16_url_secure(lud16)
                    .unwrap()
                    .2
                    .starts_with("https://"),
                "non-https url for {lud16}"
            );
        }
    }

    #[test]
    fn malformed_lud16_rejected() {
        for bad in [
            "",
            "no-at-sign",
            "@domain.com",
            "user@",
            "user@domain.com@evil.org",
            "../admin@domain.com",
            "a/b@domain.com",
            "a?b@domain.com",
            "a#b@domain.com",
            "sp ace@domain.com",
            "usér@domain.com",
        ] {
            assert!(parse_lud16_url_secure(bad).is_err(), "accepted: {bad}");
        }
        let long_user = format!("{}@domain.com", "u".repeat(65));
        assert!(parse_lud16_url_secure(&long_user).is_err());
    }

    #[test]
    #[allow(deprecated)]
    fn legacy_parse_does_not_allowlist_user() {
        let (user, _, url) = parse_lud16_url("../admin@domain.com").unwrap();
        assert_eq!(user, "../admin");
        assert_eq!(url, "https://domain.com/.well-known/lnurlp/../admin");
        assert!(parse_lud16_url("").is_err());
    }

    #[test]
    fn host_of_behavior() {
        assert_eq!(host_of("https://example.com/path").unwrap(), "example.com");
        assert_eq!(host_of("wss://relay.nostr.com").unwrap(), "relay.nostr.com");
        assert!(host_of("not a url").is_err());
        assert!(host_of("https://").is_err());
    }
}
