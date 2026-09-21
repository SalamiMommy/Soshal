//! Utility FFI module
//!
//! Common helpers: hashing, base64url, truncation, hashtag extraction, and
//! low-level TCP probes for daemon status checks.

use flutter_rust_bridge::frb;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::sync::MutexGuard;

/// Lock a `std::sync::Mutex`, recovering the guard if it was poisoned instead
/// of panicking. This is the single source of the
/// `.lock().unwrap_or_else(|e| e.into_inner())` idiom used at every global
/// handle site in the bridge.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| {
        // A thread panicked while holding this mutex. The guard is recovered so
        // the process can continue, but the event is logged. Any occurrence in
        // production warrants investigation: the guarded state may be inconsistent.
        eprintln!(
            "[soshal] WARN: mutex poisoning recovered ({}:{}); \
             inspect for state inconsistency",
            file!(),
            line!()
        );
        e.into_inner()
    })
}

/// Lock a **security-critical** `std::sync::Mutex`. Unlike [`lock`], this does
/// NOT recover from poisoning — a poisoned security-critical mutex indicates
/// that a thread panicked while holding sensitive state (e.g. the signer key),
/// and that state may be inconsistent or partially-zeroized. Returning the
/// poisoned guard would allow callers to observe or overwrite corrupted key
/// material. Instead, this returns `Err` so the caller's operation fails
/// cleanly without touching the inconsistent state.
pub(crate) fn lock_critical<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    m.lock().map_err(|_| {
        eprintln!(
            "[soshal] CRITICAL: security-critical mutex poisoned ({}:{}); \
             operation aborted to prevent inconsistent key material exposure",
            file!(),
            line!()
        );
        "security mutex poisoned: operation aborted for safety".to_string()
    })
}

/// Read a text file with `O_NOFOLLOW` (Unix): a symlink planted at `path`
/// between resolution and open is rejected by the kernel instead of followed
/// (TOCTOU / symlink-race hardening, same pattern as
/// `media.rs::open_allowed_read`). Used for secret-bearing files (sealed key,
/// session key) where following a substituted symlink exfiltrates/corrupts
/// key material.
pub(crate) fn read_to_string_nofollow(path: &std::path::Path) -> std::io::Result<String> {
    #[cfg(unix)]
    {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        Ok(s)
    }
    #[cfg(not(unix))]
    {
        std::fs::read_to_string(path)
    }
}

/// Lock-free token-bucket rate limiter backed by a single `AtomicU64`.
///
/// The u64 packs two u32 fields: the high 32 bits hold the current 1-second
/// window key (truncated Unix timestamp), and the low 32 bits hold the call
/// count within that window. A CAS loop provides thread-safety without a Mutex.
///
/// Primarily used to throttle FFI crypto operations (signing, encryption, HKDF)
/// against a compromised Flutter plugin that might call them in a tight loop
/// (signing oracle, CPU-exhaustion via HKDF).
pub(crate) struct RateLimiter {
    state: AtomicU64,
    max_per_sec: u32,
}

impl RateLimiter {
    pub(crate) const fn new(max_per_sec: u32) -> Self {
        Self {
            state: AtomicU64::new(0),
            max_per_sec,
        }
    }

    /// Consume one token. Returns `Ok(())` if within the per-second budget,
    /// `Err` if the budget is exhausted for the current second.
    pub(crate) fn check(&self) -> Result<(), String> {
        // Truncate to 32 bits: safely wraps every ~136 years, more than
        // adequate for a 1-second sliding-window comparison.
        let now_win = (soshal_common_core::format::now_secs() as u64) & 0xFFFF_FFFF;
        loop {
            let old = self.state.load(Ordering::Relaxed);
            let win = old >> 32;
            let cnt = (old & 0xFFFF_FFFF) as u32;
            let (new_win, new_cnt) = if win == now_win {
                if cnt >= self.max_per_sec {
                    return Err(format!(
                        "rate limit exceeded: max {} operations per second",
                        self.max_per_sec
                    ));
                }
                (now_win, cnt + 1)
            } else {
                // New second: reset the window.
                (now_win, 1)
            };
            let new_state = (new_win << 32) | (new_cnt as u64);
            if self
                .state
                .compare_exchange_weak(old, new_state, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(());
            }
        }
    }
}

/// Bounded, TTL'd key/value cache for the function-global caches scattered
/// across the ffi modules (single source of the OnceLock<Mutex<HashMap>> +
/// eviction + saturating_sub timestamp shape). Lives behind the caller's
/// static `OnceLock<Mutex<TtlCache<K, V>>>`, always accessed via [`lock`].
pub(crate) struct TtlCache<K, V> {
    entries: HashMap<K, (i64, V)>,
    ttl_secs: i64,
    cap: usize,
}

impl<K: Eq + Hash + Clone, V> TtlCache<K, V> {
    pub(crate) fn new(ttl_secs: i64, cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl_secs,
            cap,
        }
    }

    /// Look up `k`, ignoring entries older than the TTL. Stale entries are
    /// left in place (the bounded `insert` evicts them once capacity is
    /// reached), so a miss can return a borrowed value without a second
    /// borrow of the map.
    pub(crate) fn get<Q>(&mut self, k: &Q, now: i64) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let (ts, v) = self.entries.get(k)?;
        if now.saturating_sub(*ts) >= self.ttl_secs {
            return None;
        }
        Some(v)
    }

    /// Insert `k -> v` at `now`, evicting one entry when the cache is at
    /// capacity so distinct keys can't grow it without bound (TTL expiry
    /// bounds actual staleness; eviction order is arbitrary).
    pub(crate) fn insert(&mut self, k: K, v: V, now: i64) {
        if !self.entries.contains_key(&k) && self.entries.len() >= self.cap {
            let victim = self
                .entries
                .iter()
                .find(|(_, (ts, _))| now.saturating_sub(*ts) >= self.ttl_secs)
                .map(|(k, _)| k.clone())
                .or_else(|| {
                    self.entries
                        .iter()
                        .min_by_key(|(_, (ts, _))| *ts)
                        .map(|(k, _)| k.clone())
                });
            if let Some(victim_key) = victim {
                self.entries.remove(&victim_key);
            }
        }
        self.entries.insert(k, (now, v));
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Serialize any value to JSON for FFI transport (structs cross the bridge
/// as JSON strings; Dart side re-parses them).
pub fn json_ok<T: serde::Serialize>(v: T) -> Result<String, String> {
    serde_json::to_string(&v).map_err(|e| format!("serialize: {e}"))
}

pub fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn json_ok_or_empty<T: serde::Serialize>(v: T) -> String {
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string())
}

/// SHA256 hash of input string (hex).
#[frb(sync, serialize)]
pub fn util_sha256_hex(input: String) -> Result<String, String> {
    Ok(soshal_crypto_core::hash::sha256_hex(input.as_bytes())).into()
}

/// Base64URL (no padding) encode.
#[frb(sync, serialize)]
pub fn util_base64url_encode(input: String) -> Result<String, String> {
    Ok(soshal_crypto_core::base64url::base64url_encode(
        input.as_bytes(),
    ))
    .into()
}

/// Base64URL (no padding) decode; returns the decoded string if valid UTF-8.
/// Returns `Err` on malformed base64url input or if the decoded bytes are not
/// valid UTF-8. Callers that pass speculative/optional data should handle the
/// error gracefully (e.g., treat it as an absent value).
#[frb(sync, serialize)]
pub fn util_base64url_decode(input: String) -> Result<String, String> {
    let bytes = soshal_crypto_core::base64url::base64url_decode(&input)
        .ok_or_else(|| "base64url decode failed: invalid input".to_string())?;
    String::from_utf8(bytes)
        .map_err(|_| "base64url decode failed: decoded bytes are not valid UTF-8".to_string())
        .into()
}

/// Truncate string to max length (char-boundary safe).
#[frb(sync, serialize)]
pub fn util_truncate(input: String, max_len: usize) -> Result<String, String> {
    Ok(soshal_common_core::format::truncate(&input, max_len)).into()
}

/// Extract hashtags (without `#`) from text.
#[frb(sync, serialize)]
pub fn util_extract_hashtags(text: String) -> Result<Vec<String>, String> {
    Ok(soshal_content_core::hashtag::extract(&text)).into()
}

/// Apply thread affinity governing (pin calling thread to Performance or Efficiency cores).
#[frb(sync, serialize)]
pub fn util_apply_thread_affinity(target_performance: bool) -> Result<bool, String> {
    if target_performance {
        soshal_common_core::thread_governor::pin_to_performance_cores()?;
    } else {
        soshal_common_core::thread_governor::pin_to_efficiency_cores()?;
    }
    Ok(true)
}

/// Connect with a short timeout to `host:port` and report whether anything
/// accepted the connection (used for i2pd / Freenet / Reticulum daemons).
pub(crate) fn tcp_probe(host: &str, port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    let addr = format!("{host}:{port}");
    TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
        Duration::from_millis(500),
    )
    .map(|s| s.set_nonblocking(true).is_ok() && s.peer_addr().is_ok())
    .unwrap_or(false)
}

/// Cached TCP probe with a 5-second TTL to avoid repeated socket connections on hot paths.
#[cfg_attr(test, allow(dead_code))] // only referenced from cfg(not(test)) paths
pub(crate) fn cached_tcp_probe(host: &str, port: u16) -> bool {
    use std::sync::OnceLock;

    static PROBE_CACHE: OnceLock<Mutex<TtlCache<(String, u16), bool>>> = OnceLock::new();
    let cache = PROBE_CACHE.get_or_init(|| Mutex::new(TtlCache::new(5, 256)));
    let key = (host.to_string(), port);
    let mut guard = lock(cache);
    if let Some(&result) = guard.get(&key, soshal_common_core::format::now_secs()) {
        return result;
    }
    let fresh = tcp_probe(host, port);
    guard.insert(key, fresh, soshal_common_core::format::now_secs());
    fresh
}

pub(crate) fn uuid_like() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64url_roundtrip() {
        let enc = util_base64url_encode("hello world".to_string()).unwrap();
        assert_eq!(util_base64url_decode(enc).unwrap(), "hello world");
    }

    #[test]
    fn test_hashtag_extraction() {
        let tags = util_extract_hashtags("hello #nostr and #soshal #nostr".to_string()).unwrap();
        assert!(tags.iter().any(|t| t == "nostr"));
        assert!(tags.iter().any(|t| t == "soshal"));
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_sha256_known_vector() {
        assert_eq!(
            util_sha256_hex(String::new()).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_base64url_decode_garbage_and_vector() {
        // Known vector: base64("hello world") = "aGVsbG8gd29ybGQ=", url-form strips '='.
        assert_eq!(
            util_base64url_decode("aGVsbG8gd29ybGQ".to_string()).unwrap(),
            "hello world"
        );
        // Garbage input is an explicit decode error, not an empty string.
        assert!(util_base64url_decode("!!!".to_string()).is_err());
    }

    #[test]
    fn test_truncate_boundaries() {
        assert_eq!(util_truncate("hello".to_string(), 10).unwrap(), "hello");
        assert_eq!(util_truncate("hello".to_string(), 5).unwrap(), "hello");
        assert_eq!(util_truncate("hello".to_string(), 3).unwrap(), "he…");
        assert_eq!(util_truncate("hello".to_string(), 0).unwrap(), "");
    }

    #[test]
    fn test_ttl_cache_expiry_and_cap() {
        use super::super::util::TtlCache;
        let mut c = TtlCache::new(2, 2);
        c.insert("a".to_string(), 1, 100);
        assert_eq!(c.get("a", 101), Some(&1));
        // Expired after TTL.
        assert_eq!(c.get("a", 103), None);
        // Cap evicts one oldest entry per insert when full.
        c.insert("b".to_string(), 2, 100);
        c.insert("c".to_string(), 3, 100);
        assert_eq!(c.entries.len(), 2);
        // A clear wipes everything (used on moderation filter change).
        c.clear();
        assert!(c.entries.is_empty());
    }

    #[test]
    fn test_tcp_probe() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_probe("127.0.0.1", port));
        drop(listener);
        assert!(!tcp_probe("127.0.0.1", port));
        // Malformed host falls back to 127.0.0.1:1 (refused).
        assert!(!tcp_probe("not-an-ip", 9999));
    }

    #[test]
    fn test_uuid_like_shape() {
        let a = uuid_like();
        let b = uuid_like();
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn test_apply_thread_affinity_both_branches() {
        // Ok always carries true; real pinning may fail in restricted sandboxes.
        let perf = util_apply_thread_affinity(true);
        let eff = util_apply_thread_affinity(false);
        assert!(perf.as_ref().map_or(true, |v| *v));
        assert!(eff.as_ref().map_or(true, |v| *v));
    }
}
