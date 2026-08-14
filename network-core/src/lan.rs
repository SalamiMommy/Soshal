//! LAN peer discovery helpers: private-IP checks, beacon MAC/verify, and
//! bearer-token derivation. Keychain access stays app-side; these functions
//! take the derived key material as input.

use soshal_crypto_core::hash::hmac_sha256;

/// True for RFC-1918, loopback, and link-local addresses. LAN sync/beacon
/// traffic is only ever accepted from private hosts.
pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || o[0] == 127
                || (o[0] == 169 && o[1] == 254)
        }
        std::net::IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfc00 || s[0] == 0xfe80 || v6.is_loopback()
        }
    }
}

/// Beacon body: `MAGIC:pubkey:port`.
pub fn beacon_body(magic: &str, pubkey: &str, port: u16) -> String {
    format!("{magic}:{pubkey}:{port}")
}

/// MACs a beacon body so receivers can verify the sender holds the
/// identity's keychain-derived secret before trusting the claimed pubkey.
pub fn beacon_mac(key: &[u8; 32], body: &str) -> String {
    hex::encode(hmac_sha256(key, body.as_bytes()))
}

/// Verifies a received beacon against the derived key. Returns the claimed
/// pubkey + port on success. Rejects malformed bodies, unknown magics, and
/// bad MACs.
pub fn parse_beacon(
    key: &[u8; 32],
    magic: &str,
    text: &str,
    default_port: u16,
) -> Option<(String, u16)> {
    if !text.starts_with(magic) {
        return None;
    }
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() < 4 {
        return None;
    }
    let body = format!("{}:{}:{}", parts[0], parts[1], parts[2]);
    let mac = beacon_mac(key, &body);
    if parts[3] != mac {
        return None;
    }
    let peer_pk = parts[1].to_string();
    if peer_pk.len() != 64 || !peer_pk.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let port = parts
        .get(2)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(default_port);
    Some((peer_pk, port))
}

/// Deterministic per-identity LAN sync bearer token: first 16 bytes of the
/// at-rest key. Two devices of the same identity derive the same token.
pub fn sync_token(at_rest_key: &[u8; 32]) -> String {
    hex::encode(&at_rest_key[..16])
}
