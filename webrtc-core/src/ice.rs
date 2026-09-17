use serde::Deserialize;
use soshal_common_core::json_util::{json_in, json_out};
use soshal_common_core::url::{is_private_ip_str, is_private_ipv6_str};

fn check_word_private_ip(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    if let Ok(sa) = word.parse::<std::net::SocketAddr>() {
        let ip_str = sa.ip().to_string();
        if is_private_ip_str(&ip_str) || is_private_ipv6_str(&ip_str) {
            return true;
        }
    }
    if let Some(start) = word.find('[') {
        if let Some(rest) = word.get(start + 1..) {
            if let Some((host, _rest)) = rest.split_once(']') {
                if is_private_ipv6_str(host) {
                    return true;
                }
            }
        }
    }
    let clean = word.trim_matches(|c| c == '[' || c == ']');
    if clean.is_empty() {
        return false;
    }
    if is_private_ip_str(clean) || is_private_ipv6_str(clean) {
        return true;
    }
    if let Some((host, _port)) = clean.split_once(':') {
        if is_private_ip_str(host) || is_private_ipv6_str(host) {
            return true;
        }
    }
    false
}

pub const MAX_CANDIDATE_LEN: usize = 2048;
pub const MAX_REDACT_LEN: usize = 1024 * 1024;

fn has_private_ip(s: &str) -> bool {
    let mut word = String::new();
    for c in s.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' || c == '[' || c == ']' {
            if word.len() < 128 {
                word.push(c);
            }
        } else {
            if !word.is_empty() {
                if check_word_private_ip(&word) {
                    return true;
                }
                word.clear();
            }
        }
    }
    if !word.is_empty() && check_word_private_ip(&word) {
        return true;
    }
    false
}

pub fn is_safe_candidate(candidate: &str, force_relay: bool) -> bool {
    if candidate.is_empty() || candidate.len() > MAX_CANDIDATE_LEN {
        return false;
    }
    if candidate.contains("typ host") {
        return false;
    }
    if force_relay && (candidate.contains("typ srflx") || candidate.contains("typ prflx")) {
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
    if s.len() > MAX_REDACT_LEN {
        return String::new();
    }
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    for c in s.chars() {
        if c.is_ascii_hexdigit() || c == '.' || c == ':' || c == '[' || c == ']' {
            if word.len() < 128 {
                word.push(c);
            }
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
        if check_word_private_ip(word) {
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
