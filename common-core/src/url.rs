use regex::{Regex, RegexSet};
use std::collections::HashSet;
use std::sync::OnceLock;
use url::Url;

use crate::regex_util::compile_re;

const MAX_URL_LENGTH: usize = 2048;
const MAX_EXTRACT_COUNT: usize = 1024;

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

fn host_security_regex_set() -> &'static RegexSet {
    static SET: OnceLock<RegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        RegexSet::new([
            r"(?i)localhost",
            r"127\.0\.0\.1",
            r"^127\.", // 127.1 / 127.0.1 loopback shorthand
            r"(?i)0x7f",
            r"^169\.254\.",
            r"^192\.168\.",
            r"^10\.",
            r"^172\.(1[6-9]|2[0-9]|3[0-1])\.",
            r"::1",
            r"(?i)fc00:",
            r"(?i)fe80:",
            r"^0\.0\.0\.0$",
            r"^0$",
            r"^0[0-7]+\.",
            r"(?i)^0x[0-9a-f]+\.",
            r"^\d{5,}",
            r"(?i)\.nip\.io$",
            r"(?i)\.xip\.io$",
            r"(?i)\.sslip\.io$",
            r"(?i)\.localtest\.me$",
            r"(?i)\.loca\.lt$",
            r"(?i)\.customer\.your-server\.de$",
            r"(?i)\.dns\.to$",
            r"(?i)\.traefik\.me$",
            // Bare wildcard-DNS domains (no leading dot) resolve to
            // loopback for every subdomain — block the apex too.
            r"(?i)^nip\.io$",
            r"(?i)^xip\.io$",
            r"(?i)^sslip\.io$",
            r"(?i)^localtest\.me$",
            r"(?i)^loca\.lt$",
        ])
        .expect("valid host security regex set")
    })
}

pub fn extract(text: &str) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    url_re()
        .find_iter(text)
        .map(|m| m.as_str())
        // Cap per-match length and total count so adversarial text cannot
        // force unbounded output; duplicates skipped in first-seen order.
        .filter(|m| m.len() <= MAX_URL_LENGTH && seen.insert(m))
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
    // Embedded credentials (`http://user:pass@host`) must not be accepted:
    // they would be stored and forwarded to the host unauthenticated.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    if is_private_ip_str(hostname) || is_private_ipv6_str(hostname) {
        return false;
    }
    if host_security_regex_set().is_match(hostname) {
        return false;
    }
    if re_digits_5().is_match(hostname) {
        return false;
    }
    // Single-integer IPv4 forms (decimal/octal, e.g. 2130706433, 017700000001,
    // short 1234) resolve as IPs on many stacks. Block all pure-digit hosts,
    // not just 5+ digits.
    if re_digits_only().is_match(hostname) {
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

/// True when `addr` is loopback, private, link-local, CGNAT, multicast,
/// unspecified, or reserved space. Canonical IPv4 predicate for the URL/SSRF
/// checks in this module.
fn is_private_ipv4(addr: std::net::Ipv4Addr) -> bool {
    let o = addr.octets();
    o[0] == 0
        || o[0] == 10
        || o[0] == 127
        || o[0] == 169 && o[1] == 254
        || o[0] == 172 && (16..=31).contains(&o[1])
        || o[0] == 192 && o[1] == 168
        || o[0] == 100 && (64..=127).contains(&o[1])
        || o[0] == 198 && (o[1] == 18 || o[1] == 19) // RFC 2544 benchmarking 198.18.0.0/15
        || o[0] >= 240 // reserved (240.0.0.0/4) and broadcast (255.255.255.255)
        || addr.is_multicast()
}

fn is_private_ipv6(addr: std::net::Ipv6Addr) -> bool {
    if addr.is_unspecified() || addr.is_loopback() || addr.is_multicast() {
        return true;
    }
    let segs = addr.segments();
    if (segs[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if (segs[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    if let Some(mapped) = addr.to_ipv4_mapped() {
        return is_private_ipv4(mapped);
    }
    // Teredo tunneling (2001::/32) relays over NAT64/other clients: poor
    // connectivity and trivially spoofed — never treat as a public address.
    if segs[0] == 0x2001 && segs[1] == 0 {
        return true;
    }
    if segs[0] == 0x2002 {
        let v4 = std::net::Ipv4Addr::new(
            ((segs[1] >> 8) & 0xff) as u8,
            (segs[1] & 0xff) as u8,
            ((segs[2] >> 8) & 0xff) as u8,
            (segs[2] & 0xff) as u8,
        );
        return is_private_ipv4(v4);
    }
    if segs[0] == 0 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0 && segs[4] == 0 && segs[5] == 0
    {
        let v4 = std::net::Ipv4Addr::new(
            ((segs[6] >> 8) & 0xff) as u8,
            (segs[6] & 0xff) as u8,
            ((segs[7] >> 8) & 0xff) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_private_ipv4(v4);
    }
    false
}

/// True when `host` parses as an IPv6 literal in loopback, private,
/// link-local, multicast, unspecified, Teredo (2001::/32), 6to4 (2002::/16),
/// or IPv4-mapped/compatible private space. Parses like
/// [`is_private_ip_str`] — never treats a non-IPv6 string as private.
pub fn is_private_ipv6_str(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.parse::<std::net::Ipv6Addr>()
        .map(is_private_ipv6)
        .unwrap_or(false)
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
        std::net::IpAddr::V4(v4) => is_private_ipv4(v4),
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped IPv6 (::ffff:127.0.0.1) must be judged as the
            // embedded IPv4 address; otherwise loopback/private guards
            // are bypassed with a V6-form literal.
            if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                return is_private_ipv4(mapped_v4);
            }
            is_private_ipv6(v6)
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
    if host_security_regex_set().is_match(hostname) {
        return (false, false);
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
