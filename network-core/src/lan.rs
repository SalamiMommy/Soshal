//! LAN peer discovery helpers: private-IP checks, beacon MAC/verify, and
//! bearer-token derivation. Keychain access stays app-side; these functions
//! take the derived key material as input.

use soshal_crypto_core::hash::hmac_sha256;

/// True for RFC-1918, loopback, link-local, and wildcard addresses. Delegates
/// to `common-core::url::is_private_ip_str` (the SSRF-grade canonical check:
/// also covers CGNAT, multicast, IPv4-mapped IPv6, and v6 link-local/ULA).
/// LAN sync/beacon traffic is only ever accepted from private hosts.
pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    soshal_common_core::url::is_private_ip_str(&ip.to_string())
}

/// Beacon body: `MAGIC:pubkey:port:unix_secs`. The timestamp is MAC'd and
/// freshness-checked on receive so a captured handshake line cannot be
/// replayed forever.
pub fn beacon_body(magic: &str, pubkey: &str, port: u16, ts_secs: u64) -> String {
    format!("{magic}:{pubkey}:{port}:{ts_secs}")
}

/// MACs a beacon body so receivers can verify the sender holds the
/// identity's keychain-derived secret before trusting the claimed pubkey.
pub fn beacon_mac(key: &[u8; 32], body: &str) -> String {
    hex::encode(hmac_sha256(key, body.as_bytes()))
}

/// Constant-time byte compare. Used for MAC verification so a timing
/// side-channel cannot leak the expected MAC byte by byte.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc: u8 = 0;
    for (x, y) in a.iter().zip(b) {
        acc |= x ^ y;
    }
    acc == 0
}

/// Maximum accepted age skew (seconds) between a beacon's timestamp and the
/// receiver's clock, both past and future directions.
pub const BEACON_MAX_SKEW_SECS: u64 = 120;

/// Verifies a received beacon against the derived key. Returns the claimed
/// pubkey + port on success. Rejects malformed bodies, unknown magics, bad
/// MACs (constant-time), and stale/future timestamps outside the skew window.
pub fn parse_beacon(
    key: &[u8; 32],
    magic: &str,
    text: &str,
    default_port: u16,
    now_secs: u64,
) -> Option<(String, u16)> {
    if !text.starts_with(magic) {
        return None;
    }
    // Split the 4 colon-delimited fields (magic, pubkey, port, ts) with
    // slice math; the remainder is the MAC hex field. No Vec<&str> alloc nor
    // format! body rebuild per beacon.
    let mut fields: [&str; 4] = [""; 4];
    let mut rest = text;
    for field in fields.iter_mut() {
        match rest.split_once(':') {
            Some((head, tail)) => {
                *field = head;
                rest = tail;
            }
            None => return None,
        }
    }
    let body_len = text.len().saturating_sub(rest.len() + 1);
    let body = &text[..body_len];
    let expected = beacon_mac(key, body);
    let expected_bytes = hex::decode(expected).ok()?;
    let got_bytes = hex::decode(rest).ok()?;
    if !ct_eq(&got_bytes, &expected_bytes) {
        return None;
    }
    let peer_pk = fields[1];
    if peer_pk.len() != 64 || !peer_pk.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let ts: u64 = fields[3].parse().ok()?;
    let skew = now_secs.abs_diff(ts);
    if skew > BEACON_MAX_SKEW_SECS {
        return None;
    }
    let port = fields[2].parse::<u16>().ok().unwrap_or(default_port);
    Some((peer_pk.to_string(), port))
}

/// Deterministic per-identity LAN sync bearer token: first 16 bytes of the
/// at-rest key. Two devices of the same identity derive the same token.
pub fn sync_token(at_rest_key: &[u8; 32]) -> String {
    hex::encode(&at_rest_key[..16])
}
