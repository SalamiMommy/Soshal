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

/// Beacon body: `MAGIC:pubkey:port:unix_secs:nonce`. The timestamp is MAC'd
/// and freshness-checked on receive so a captured handshake line cannot be
/// replayed forever; the random nonce additionally invalidates byte-for-byte
/// replays of a captured beacon (see [`beacon_seq`]).
pub fn beacon_body(magic: &str, pubkey: &str, port: u16, ts_secs: u64, nonce_hex: &str) -> String {
    format!("{magic}:{pubkey}:{port}:{ts_secs}:{nonce_hex}")
}

/// Fresh 16-byte random nonce for a beacon (hex-encoded by the caller).
/// Unpredictable per send, so replayed beacon lines carry a duplicate nonce
/// and are rejected by [`BeaconSeq`].
pub fn fresh_nonce() -> [u8; 16] {
    let mut n = [0u8; 16];
    let _ = getrandom::fill(&mut n);
    n
}

/// MACs a beacon body so receivers can verify the sender holds the
/// identity's keychain-derived secret before trusting the claimed pubkey.
pub fn beacon_mac(key: &[u8; 32], body: &str) -> String {
    hex::encode(hmac_sha256(key, body.as_bytes()))
}

/// Maximum accepted age skew (seconds) between a beacon's timestamp and the
/// receiver's clock, both past and future directions.
/// 30 seconds is sufficient for normal clock drift on a LAN while keeping the
/// replay window tight enough to prevent captured-beacon reuse.
pub const BEACON_MAX_SKEW_SECS: u64 = 30;

/// Verifies a received beacon against the derived key. Returns the claimed
/// pubkey, port, and nonce on success. Rejects malformed bodies, unknown
/// magics, bad MACs (constant-time), and stale/future timestamps outside the
/// skew window.
pub fn parse_beacon(
    key: &[u8; 32],
    magic: &str,
    text: &str,
    default_port: u16,
    now_secs: u64,
) -> Option<(String, u16, [u8; 16])> {
    if !text.starts_with(magic) {
        return None;
    }
    // Split the 5 colon-delimited fields (magic, pubkey, port, ts, nonce)
    // with slice math; the remainder is the MAC hex field. No Vec<&str> alloc
    // nor format! body rebuild per beacon.
    let mut fields: [&str; 5] = [""; 5];
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
    if !soshal_common_core::util::constant_time_eq(&got_bytes, &expected_bytes) {
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
    let nonce_bytes = hex::decode(fields[4]).ok()?;
    let nonce: [u8; 16] = nonce_bytes.try_into().ok()?;
    let port = fields[2].parse::<u16>().ok().unwrap_or(default_port);
    Some((peer_pk.to_string(), port, nonce))
}

/// Per-identity replay guard for beacon nonces: the last N nonces seen from
/// each peer pubkey are remembered, so a captured beacon line replayed
/// byte-for-byte (same MAC, same timestamp, same nonce) is rejected even
/// inside the timestamp skew window. Legitimate parallel handshakes always
/// carry fresh nonces and are unaffected.
#[derive(Default)]
pub struct BeaconSeq {
    seen: std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<[u8; 16]>>>,
    /// Tombstone of nonces belonging to evicted peers: an evicted peer's
    /// captured beacons stay rejected even after its entry was dropped,
    /// closing the replay window opened by eviction.
    evicted: std::sync::Mutex<std::collections::VecDeque<[u8; 16]>>,
}

impl BeaconSeq {
    const MAX_NONCES_PER_PEER: usize = 64;
    const MAX_TRACKED_PEERS: usize = 1024;
    /// Hostile peer churn flushes evicted nonces into the tombstone bucket;
    /// a small FIFO lets an attacker rotate real peers' tombstones out and
    /// reopen their replay window. 16k entries (~256 KiB) forces ~16k
    /// churned peers (each must present a valid-shaped nonce to become
    /// tracked first) before the oldest tombstone is evicted.
    const MAX_EVICTED_TOMBSTONES: usize = 16_384;

    pub fn new() -> Self {
        Self::default()
    }

    /// Records `nonce` for `peer_pk` and reports whether it was new. Returns
    /// `false` for duplicates (replay) and drops the oldest remembered nonce
    /// once the per-peer cap is reached. Evicted peers' nonces are moved to
    /// the tombstone bucket instead of being forgotten.
    pub fn check_and_record(&self, peer_pk: &str, nonce: &[u8; 16]) -> bool {
        let mut g = match self.seen.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if !g.contains_key(peer_pk) && g.len() >= Self::MAX_TRACKED_PEERS {
            // Evict one arbitrary existing peer; move its nonces into the
            // tombstone bucket so replays stay rejected.
            if let Some(k) = g.keys().next().cloned() {
                if let Some(queue) = g.remove(&k) {
                    let mut ev = match self.evicted.lock() {
                        Ok(e) => e,
                        Err(e) => e.into_inner(),
                    };
                    for n in queue {
                        if !ev.contains(&n) {
                            ev.push_back(n);
                        }
                    }
                    while ev.len() > Self::MAX_EVICTED_TOMBSTONES {
                        ev.pop_front();
                    }
                }
            }
        }
        {
            let ev = match self.evicted.lock() {
                Ok(e) => e,
                Err(e) => e.into_inner(),
            };
            if ev.contains(nonce) {
                return false;
            }
            let _unused = ev;
        }
        let queue = g
            .entry(peer_pk.to_string())
            .or_insert_with(std::collections::VecDeque::new);
        if queue.contains(nonce) {
            return false;
        }
        if queue.len() >= Self::MAX_NONCES_PER_PEER {
            queue.pop_front();
        }
        queue.push_back(*nonce);
        true
    }
}

/// Process-global beacon replay guard, shared by the TCP LAN transport and
/// both QUIC accept paths so a beacon captured from one server cannot be
/// replayed against another listener of the same identity.
pub fn beacon_seq() -> &'static BeaconSeq {
    static SEQ: std::sync::OnceLock<BeaconSeq> = std::sync::OnceLock::new();
    SEQ.get_or_init(BeaconSeq::new)
}

/// Deterministic per-identity LAN sync bearer token derived via HKDF-SHA256
/// from the at-rest key.  Deriving via HKDF (rather than truncating the raw
/// key) ensures the token does not expose any bytes of the source key.
pub fn sync_token(at_rest_key: &[u8; 32]) -> String {
    let okm =
        soshal_crypto_core::hash::hkdf_sha256(at_rest_key, b"soshal-lan-sync", b"bearer-token", 32)
            .expect("HKDF with fixed-length output cannot fail");
    hex::encode(&okm[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_seq_rejects_replayed_nonce() {
        let seq = BeaconSeq::new();
        let pk = "ab".repeat(32);
        let n1 = fresh_nonce();
        let n2 = fresh_nonce();
        assert!(seq.check_and_record(&pk, &n1));
        assert!(seq.check_and_record(&pk, &n2));
        // Byte-for-byte replay of either earlier beacon is rejected.
        assert!(!seq.check_and_record(&pk, &n1));
        assert!(!seq.check_and_record(&pk, &n2));
        // Fresh nonces keep flowing (parallel legit handshakes).
        assert!(seq.check_and_record(&pk, &fresh_nonce()));
        // A different peer is independent.
        let other = "cd".repeat(32);
        assert!(seq.check_and_record(&other, &n1));
    }

    #[test]
    fn beacon_seq_caps_per_peer() {
        let seq = BeaconSeq::new();
        let pk = "ab".repeat(32);
        let mut nonces = Vec::new();
        for _ in 0..BeaconSeq::MAX_NONCES_PER_PEER + 10 {
            let n = fresh_nonce();
            assert!(seq.check_and_record(&pk, &n));
            nonces.push(n);
        }
        // The oldest nonces were evicted, so they are accepted again
        // (FIFO cap keeps memory bounded, not a security regression: evicted
        // nonces are older than any in-window beacon the peer would send).
        for n in nonces.iter().take(5) {
            assert!(seq.check_and_record(&pk, n));
        }
    }

    #[test]
    fn beacon_seq_tombstones_evicted_peer_nonces() {
        let seq = BeaconSeq::new();
        let evictor = "ab".repeat(32);
        let n_old = fresh_nonce();
        assert!(seq.check_and_record(&evictor, &n_old));
        assert!(seq.check_and_record(&evictor, &fresh_nonce()));
        // Fill the map past MAX_TRACKED_PEERS: the first-inserted peer's
        // entries are evicted into the tombstone bucket.
        let mut pks = Vec::new();
        for i in 0..BeaconSeq::MAX_TRACKED_PEERS + 2 {
            let pk = format!("{:02x}", i).repeat(32);
            pks.push(pk);
            assert!(seq.check_and_record(&pks[i], &fresh_nonce()));
        }
        // Evicted nonce now rejected via tombstone, not just replay map.
        assert!(!seq.check_and_record(&evictor, &n_old));
    }

    #[test]
    fn beacon_seq_tombstones_survive_hostile_churn_past_old_cap() {
        // WP11: the old 1024-entry tombstone FIFO let ~1k churned peers
        // rotate out a real peer's tombstones and reopen its replay window.
        // Fill the tombstone bucket beyond the OLD cap and confirm the
        // evicted nonce is still rejected.
        let seq = BeaconSeq::new();
        let victim = "ab".repeat(32);
        let n_old = fresh_nonce();
        assert!(seq.check_and_record(&victim, &n_old));
        assert!(seq.check_and_record(&victim, &fresh_nonce()));
        // Bigger-than-old-cap churn: each new tracked peer evicts the oldest
        // tracked peer (adding its nonces to tombstones), so the bucket
        // rotates past an attacker-sized window. Total tombstone pushes stay
        // under MAX_EVICTED_TOMBSTONES so the victim's entries are never the
        // ones dropped (they were pushed earliest).
        let churn = BeaconSeq::MAX_TRACKED_PEERS + 12_000;
        for i in 0..churn {
            // Distinct single-nonce peers; once tracked they accumulate a
            // second nonce so eviction always carries entries into tombstones.
            let pk = format!("{:08x}", i).repeat(4);
            assert!(seq.check_and_record(&pk, &fresh_nonce()));
        }
        assert!(
            !seq.check_and_record(&victim, &n_old),
            "tombstone must survive hostile churn well past the old 1024 cap"
        );
        // Bounded memory holds regardless of churn.
        {
            let ev = seq.evicted.lock().unwrap();
            assert!(ev.len() <= BeaconSeq::MAX_EVICTED_TOMBSTONES);
        }
    }
}
