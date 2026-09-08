use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::RwLock;

use sha2::{Digest, Sha256};

/// Cache key = SHA-256(event id || signature bytes) so a replayed event id with a
/// forged signature can never be served a cached verification result, while keeping
/// the key size at 32 bytes (66% memory reduction over (id, sig)).
///
/// Implements an O(1) 2-generation LRU set: on hit in `previous`, the key is
/// promoted to `current`. When `current` reaches half of max capacity, `previous`
/// is replaced by `current` and `current` resets.
struct VerifiedCache {
    current: HashSet<[u8; 32]>,
    previous: HashSet<[u8; 32]>,
}

impl VerifiedCache {
    fn with_capacity(cap: usize) -> Self {
        let half = (cap / 2).max(512);
        Self {
            current: HashSet::with_capacity(half),
            previous: HashSet::with_capacity(half),
        }
    }
}

fn compute_verify_cache_key(e: &nostr::event::Event) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(e.id.as_bytes());
    hasher.update(e.sig.as_bytes());
    hasher.finalize().into()
}

fn max_cache_capacity() -> usize {
    #[cfg(target_os = "android")]
    {
        10_000
    }
    #[cfg(not(target_os = "android"))]
    {
        50_000
    }
}

static VERIFIED_CACHE: RwLock<Option<VerifiedCache>> = RwLock::new(None);

/// Clears the global verified event signature cache to release memory.
pub fn clear_verified_cache() {
    let mut guard = VERIFIED_CACHE.write().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// Verifies a Nostr event's Schnorr signature, using an in-memory bounded generational LRU cache.
pub fn verify_event(e: &nostr::event::Event) -> bool {
    let key = compute_verify_cache_key(e);
    {
        let guard = VERIFIED_CACHE.read().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = guard.as_ref() {
            if cache.current.contains(&key) {
                return true;
            }
            if cache.previous.contains(&key) {
                // Promotion handled below under write lock
            } else {
                // Fast path: not in cache, drop read lock and verify
            }
        }
    }

    // Check if promotion from previous is needed
    {
        let mut guard = VERIFIED_CACHE.write().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = guard.as_mut() {
            if cache.current.contains(&key) {
                return true;
            }
            if cache.previous.contains(&key) {
                cache.current.insert(key);
                return true;
            }
        }
    }

    if e.verify().is_ok() {
        let mut guard = VERIFIED_CACHE.write().unwrap_or_else(|e| e.into_inner());
        let max_cap = max_cache_capacity();
        let cache = guard.get_or_insert_with(|| VerifiedCache::with_capacity(max_cap));
        let half_cap = (max_cap / 2).max(512);

        if cache.current.len() >= half_cap {
            cache.previous =
                std::mem::replace(&mut cache.current, HashSet::with_capacity(half_cap));
        }
        cache.current.insert(key);
        true
    } else {
        false
    }
}

/// Canonical minimal Nostr event used across all cores (search, groups,
/// notifications, media parsing). Missing fields default to empty values.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct NostrEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<Vec<String>>,
    #[serde(rename = "created_at", default)]
    pub created_at: f64,
    #[serde(default)]
    pub kind: u32,
}

impl NostrEvent {
    /// Returns the first tag value matching key `key` without dynamic allocation.
    pub fn find_tag(&self, key: &str) -> Option<&str> {
        self.tags.iter().find_map(|t| {
            if t.first().map(|s| s.as_str()) == Some(key) {
                t.get(1).map(|s| s.as_str())
            } else {
                None
            }
        })
    }
}

pub use soshal_content_core::tags::{find_tag_value, find_tag_values_map, parse_audience};

impl From<&nostr::event::Event> for NostrEvent {
    fn from(e: &nostr::event::Event) -> Self {
        let mut tags = Vec::with_capacity(e.tags.len());
        for t in e.tags.iter() {
            let slice = t.as_slice();
            let mut tag_vec = Vec::with_capacity(slice.len());
            for s in slice {
                tag_vec.push(s.clone());
            }
            tags.push(tag_vec);
        }
        NostrEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_string(),
            content: e.content.clone(),
            tags,
            created_at: e.created_at.as_secs() as f64,
            kind: e.kind.as_u16() as u32,
        }
    }
}

impl From<nostr::event::Event> for NostrEvent {
    fn from(e: nostr::event::Event) -> Self {
        let mut tags = Vec::with_capacity(e.tags.len());
        for t in e.tags.iter() {
            let slice = t.as_slice();
            let mut tag_vec = Vec::with_capacity(slice.len());
            for s in slice {
                tag_vec.push(s.clone());
            }
            tags.push(tag_vec);
        }
        NostrEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_string(),
            content: e.content,
            tags,
            created_at: e.created_at.as_secs() as f64,
            kind: e.kind.as_u16() as u32,
        }
    }
}
