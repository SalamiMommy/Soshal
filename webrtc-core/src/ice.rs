use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Deserialize;
use soshal_common_core::json_util::{json_in, json_out};

fn has_private_ip(s: &str) -> bool {
    let mut word = String::new();
    for c in s.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' {
            word.push(c);
        } else {
            if !word.is_empty() {
                if word.contains(':') {
                    if is_private_ipv6(&word) {
                        return true;
                    }
                } else if is_private_ip(&word) {
                    return true;
                }
                word.clear();
            }
        }
    }
    if !word.is_empty() {
        if word.contains(':') {
            if is_private_ipv6(&word) {
                return true;
            }
        } else if is_private_ip(&word) {
            return true;
        }
    }
    false
}

pub fn is_private_ip(ip: &str) -> bool {
    soshal_common_core::url::is_private_ip_str(ip)
}

fn ipv4_is_private(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    let (a, b) = (octets[0], octets[1]);
    if a == 0 {
        return true;
    }
    if a == 10 {
        return true;
    }
    if a == 100 && (64..=127).contains(&b) {
        return true;
    }
    if a == 127 {
        return true;
    }
    if a == 169 && b == 254 {
        return true;
    }
    if a == 172 && (16..=31).contains(&b) {
        return true;
    }
    if a == 192 && b == 0 {
        return true;
    }
    if a == 192 && b == 2 {
        return true;
    }
    if a == 192 && b == 168 {
        return true;
    }
    if a == 198 && (18..=19).contains(&b) {
        return true;
    }
    if a == 198 && b == 51 && octets[2] == 100 {
        return true;
    }
    if a == 203 && b == 0 && octets[2] == 113 {
        return true;
    }
    if (224..=239).contains(&a) {
        return true;
    }
    if a >= 240 {
        return true;
    }
    false
}

pub fn is_private_ipv6(ip: &str) -> bool {
    if let Ok(addr) = ip.parse::<Ipv6Addr>() {
        return ipv6_is_private(addr);
    }
    false
}

fn ipv6_is_private(addr: Ipv6Addr) -> bool {
    if addr.is_unspecified() {
        return true;
    }
    if addr.is_loopback() {
        return true;
    }
    if addr.is_multicast() {
        return true;
    }
    let segs = addr.segments();
    if (segs[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if (segs[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    if let Some(mapped) = ipv4_mapped(&addr) {
        return ipv4_is_private(mapped);
    }
    // Teredo tunneling (2001::/32) relays over NAT64/other clients: poor
    // connectivity and trivially spoofed — never treat as a public address.
    if segs[0] == 0x2001 && segs[1] == 0 {
        return true;
    }
    if segs[0] == 0x2002 {
        let v4 = Ipv4Addr::new(
            ((segs[1] >> 8) & 0xff) as u8,
            (segs[1] & 0xff) as u8,
            ((segs[2] >> 8) & 0xff) as u8,
            (segs[2] & 0xff) as u8,
        );
        return ipv4_is_private(v4);
    }
    if segs[0] == 0 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0 && segs[4] == 0 && segs[5] == 0
    {
        let v4 = Ipv4Addr::new(
            ((segs[6] >> 8) & 0xff) as u8,
            (segs[6] & 0xff) as u8,
            ((segs[7] >> 8) & 0xff) as u8,
            (segs[7] & 0xff) as u8,
        );
        return ipv4_is_private(v4);
    }
    false
}

fn ipv4_mapped(addr: &Ipv6Addr) -> Option<Ipv4Addr> {
    match addr.octets() {
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d] => Some(Ipv4Addr::new(a, b, c, d)),
        _ => None,
    }
}

pub fn is_safe_candidate(candidate: &str, force_relay: bool) -> bool {
    if candidate.is_empty() {
        return false;
    }
    if candidate.contains("typ host") {
        return false;
    }
    if force_relay && candidate.contains("typ srflx") {
        return false;
    }
    if has_private_ip(candidate) {
        return false;
    }
    true
}

/// Replaces private IP literals in SDP/ICE payloads with `0.0.0.0` so call
/// signaling published to relays never leaks LAN addresses (browsers do the
/// same for host candidates; peers fall back to STUN srflx/relay candidates).
/// Public addresses and non-IP tokens pass through untouched.
pub fn redact_private_ips(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    for c in s.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' {
            word.push(c);
        } else {
            push_redacted_word(&mut word, &mut out);
            out.push(c);
        }
    }
    push_redacted_word(&mut word, &mut out);
    out
}

fn push_redacted_word(word: &mut String, out: &mut String) {
    if !word.is_empty() {
        let is_ip = word.contains('.') || word.contains(':');
        if is_ip && (is_private_ip(word) || is_private_ipv6(word)) {
            out.push_str("0.0.0.0");
        } else {
            out.push_str(word);
        }
        word.clear();
    }
}

/// JSON-in/JSON-out wrapper for is_safe_candidate.
/// Input: `{"candidate": "...", "forceRelay": true/false}`
/// Output: `{"safe": true/false}`
pub fn is_safe_candidate_json(input: &str) -> String {
    #[derive(Deserialize)]
    struct Input {
        candidate: String,
        #[serde(rename = "forceRelay")]
        force_relay: bool,
    }
    let Some(input) = json_in::<Option<Input>>(input, None) else {
        return json_out(&serde_json::json!({"safe": false}), r#"{"safe": false}"#);
    };
    let safe = is_safe_candidate(&input.candidate, input.force_relay);
    json_out(&serde_json::json!({"safe": safe}), r#"{"safe": false}"#)
}

/// Builds the `RTCPeerConnection` configuration for the given privacy level.
/// `friends`/`private` force relay-only ICE so host and server-reflexive
/// candidates are never offered to peers (private IPs never leak); `public`
/// allows all candidate types. An empty `stun_url` yields no ICE servers
/// (I2P mode: no STUN traffic at all). The `forceRelay` flag mirrors
/// `iceTransportPolicy` for the UI's candidate filtering.
pub fn ice_config(privacy_level: &str, stun_url: &str) -> serde_json::Value {
    let force_relay = !privacy_level.is_empty() && privacy_level != "public";
    let servers: Vec<serde_json::Value> = if stun_url.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({ "urls": stun_url })]
    };
    serde_json::json!({
        "iceServers": servers,
        "iceTransportPolicy": if force_relay { "relay" } else { "all" },
        "forceRelay": force_relay,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_level_allows_all_candidates() {
        let cfg = ice_config("public", "stun:stun.l.google.com:19302");
        assert_eq!(cfg["iceTransportPolicy"], "all");
        assert_eq!(cfg["forceRelay"], false);
        assert_eq!(cfg["iceServers"][0]["urls"], "stun:stun.l.google.com:19302");
    }

    #[test]
    fn friends_level_forces_relay() {
        let cfg = ice_config("friends", "stun:stun.l.google.com:19302");
        assert_eq!(cfg["iceTransportPolicy"], "relay");
        assert_eq!(cfg["forceRelay"], true);
    }

    #[test]
    fn private_level_forces_relay() {
        let cfg = ice_config("private", "stun:stun.l.google.com:19302");
        assert_eq!(cfg["iceTransportPolicy"], "relay");
        assert_eq!(cfg["forceRelay"], true);
    }

    #[test]
    fn empty_stun_yields_no_servers() {
        let cfg = ice_config("public", "");
        assert_eq!(cfg["iceServers"].as_array().unwrap().len(), 0);
        assert_eq!(cfg["iceTransportPolicy"], "all");
    }

    #[test]
    fn empty_level_defaults_to_all() {
        let cfg = ice_config("", "stun:stun.l.google.com:19302");
        assert_eq!(cfg["iceTransportPolicy"], "all");
        assert_eq!(cfg["forceRelay"], false);
    }
}
