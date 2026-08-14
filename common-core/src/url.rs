use regex::Regex;
use std::sync::OnceLock;
use url::Url;

use crate::regex_util::compile_re;

const MAX_URL_LENGTH: usize = 2048;
const MAX_EXTRACT_COUNT: usize = 64;

static RE_DIGITS_5: OnceLock<Regex> = OnceLock::new();
static RE_HEX_HOST: OnceLock<Regex> = OnceLock::new();
static RE_IPV4_HOST: OnceLock<Regex> = OnceLock::new();
static RE_DIGITS_ONLY: OnceLock<Regex> = OnceLock::new();

fn re_digits_5() -> &'static Regex {
    RE_DIGITS_5.get_or_init(|| compile_re(r"^\d{5,}"))
}

fn re_hex_host() -> &'static Regex {
    RE_HEX_HOST.get_or_init(|| compile_re(r"(?i)^0x[0-9a-f]+$"))
}

fn re_ipv4_host() -> &'static Regex {
    RE_IPV4_HOST.get_or_init(|| compile_re(r"^\d{1,3}(\.\d{1,3}){3}$"))
}

fn re_digits_only() -> &'static Regex {
    RE_DIGITS_ONLY.get_or_init(|| compile_re(r"^\d+$"))
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^\s<>{}|\\^`\[\]]+").expect("valid url regex"))
}

fn blocked_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            vec![
                compile_re(r"(?i)localhost"),
                compile_re(r"127\.0\.0\.1"),
                compile_re(r"^127\."), // 127.1 / 127.0.1 loopback shorthand
                compile_re(r"(?i)0x7f"),
                compile_re(r"169\.254"),
                compile_re(r"192\.168\."),
                compile_re(r"10\."),
                compile_re(r"172\.(1[6-9]|2[0-9]|3[0-1])\."),
                compile_re(r"::1"),
                compile_re(r"(?i)fc00:"),
                compile_re(r"(?i)fe80:"),
                compile_re(r"^0\.0\.0\.0$"),
                compile_re(r"^0$"),
                compile_re(r"^0[0-7]+\."),
                compile_re(r"(?i)^0x[0-9a-f]+\."),
                compile_re(r"^\d{5,}"),
            ]
        })
        .as_slice()
}

fn dns_rebinding_domains() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            vec![
                compile_re(r"(?i)\.nip\.io$"),
                compile_re(r"(?i)\.xip\.io$"),
                compile_re(r"(?i)\.sslip\.io$"),
                compile_re(r"(?i)\.localtest\.me$"),
                compile_re(r"(?i)\.loca\.lt$"),
                compile_re(r"(?i)\.customer\.your-server\.de$"),
                compile_re(r"(?i)\.dns\.to$"),
                compile_re(r"(?i)\.traefik\.me$"),
            ]
        })
        .as_slice()
}

pub fn extract(text: &str) -> Vec<String> {
    url_re()
        .find_iter(text)
        .map(|m| m.as_str())
        // Cap per-match length and total count so adversarial text cannot
        // force unbounded output.
        .filter(|m| m.len() <= MAX_URL_LENGTH)
        .take(MAX_EXTRACT_COUNT)
        .map(|s| s.to_string())
        .collect()
}

pub fn is_valid(url_str: &str) -> bool {
    Url::parse(url_str).is_ok()
}

pub fn domain(url_str: &str) -> Option<String> {
    Url::parse(url_str)
        .ok()
        .map(|u| u.host_str().unwrap_or("").to_string())
}

pub fn is_valid_media_url(url: &str) -> bool {
    if url.len() > MAX_URL_LENGTH || url.is_empty() {
        return false;
    }
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return false,
    };
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return false,
    }
    let hostname = parsed.host_str().unwrap_or("");
    for pattern in blocked_patterns() {
        if pattern.is_match(hostname) {
            return false;
        }
    }
    for pattern in dns_rebinding_domains() {
        if pattern.is_match(hostname) {
            return false;
        }
    }
    if re_digits_5().is_match(hostname) {
        return false;
    }
    if re_hex_host().is_match(hostname) {
        return false;
    }
    true
}

pub fn sanitize_link_url(url: &str) -> Option<String> {
    if is_valid_media_url(url) {
        Some(url.to_string())
    } else {
        None
    }
}

/// Returns the URL unchanged only when it is safe to use as an `href`/`src`:
/// http/https scheme, no loopback/private/link-local hosts, no rebinding domains.
/// Rejects `javascript:`, `data:`, `blob:` and every other scheme.
pub fn safe_href(url: &str) -> Option<String> {
    sanitize_link_url(url)
}

/// True when `host` is a raw IP literal pointing at loopback, private, link-local,
/// CGNAT, multicast, or unspecified space (IPv4 and IPv6). Used for post-DNS-resolve
/// checks to block SSRF into internal networks.
pub fn is_private_ip_str(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    let ip: std::net::IpAddr = match host.parse() {
        Ok(ip) => ip,
        Err(_) => return false,
    };
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 0
                || o[0] == 10
                || o[0] == 127
                || o[0] == 169 && o[1] == 254
                || o[0] == 172 && (16..=31).contains(&o[1])
                || o[0] == 192 && o[1] == 168
                || o[0] == 100 && (64..=127).contains(&o[1])
                || v4.is_multicast()
        }
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped IPv6 (::ffff:127.0.0.1) must be judged as the
            // embedded IPv4 address; otherwise loopback/private guards
            // are bypassed with a V6-form literal.
            if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                let o = mapped_v4.octets();
                return o[0] == 0
                    || o[0] == 10
                    || o[0] == 127
                    || o[0] == 169 && o[1] == 254
                    || o[0] == 172 && (16..=31).contains(&o[1])
                    || o[0] == 192 && o[1] == 168
                    || o[0] == 100 && (64..=127).contains(&o[1])
                    || mapped_v4.is_multicast();
            }
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || v6.octets()[0] & 0xfe == 0xfc // fc00::/7 unique-local
                || v6.octets()[0] == 0xfe && v6.octets()[1] & 0xc0 == 0x80 // fe80::/10 link-local
        }
    }
}

/// Validates a relay URL a *user* typed into settings: ws/wss schemes, no
/// embedded credentials, no loopback/private/link-local/unspecified hosts, no
/// DNS-rebinding domains, hostname must be a real domain (punycode flagged).
///
/// Returns `(valid, is_punycode)`.
pub fn is_valid_relay_url(url: &str) -> (bool, bool) {
    if url.is_empty() || url.len() > MAX_URL_LENGTH {
        return (false, false);
    }
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return (false, false),
    };
    match parsed.scheme() {
        "ws" | "wss" => {}
        _ => return (false, false),
    }
    // Embedded credentials (`wss://user:pass@relay`) must not be accepted:
    // they would be stored and forwarded to the relay unauthenticated.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return (false, false);
    }
    let hostname = match parsed.host_str() {
        Some(h) if !h.is_empty() => h,
        _ => return (false, false),
    };
    if !hostname.contains('.') {
        return (false, false);
    }
    // Raw IP literals are rejected outright (no way to resolve a hostname,
    // so there is no legitimate private-host relay use case here).
    if re_ipv4_host().is_match(hostname)
        || hostname.contains(':')
        || re_hex_host().is_match(hostname)
        || re_digits_only().is_match(hostname)
    {
        return (false, false);
    }
    for pattern in blocked_patterns() {
        if pattern.is_match(hostname) {
            return (false, false);
        }
    }
    for pattern in dns_rebinding_domains() {
        if pattern.is_match(hostname) {
            return (false, false);
        }
    }
    let punycode = hostname.starts_with("xn--") || hostname.contains(".xn--");
    (true, punycode)
}

/// Validates a relay URL that came from *events* (NIP-65 lists, group relay
/// lists, NIP-05 metadata) — attacker-influenceable data. Such URLs must be
/// `wss://` only; cleartext `ws://` is never accepted from event data.
pub fn is_valid_event_relay_url(url: &str) -> bool {
    if !url.starts_with("wss://") {
        return false;
    }
    is_valid_relay_url(url).0
}
