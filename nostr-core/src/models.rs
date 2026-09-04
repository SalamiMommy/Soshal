use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::RwLock;

/// Cache key = (event id, signature bytes) so a replayed event id with a
/// forged signature can never be served a cached verification result.
struct VerifiedCache {
    set: HashSet<([u8; 32], [u8; 64])>,
    queue: VecDeque<([u8; 32], [u8; 64])>,
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

/// Verifies a Nostr event's Schnorr signature, using an in-memory bounded LRU cache.
pub fn verify_event(e: &nostr::event::Event) -> bool {
    let key = (*e.id.as_bytes(), *e.sig.as_bytes());
    let guard = VERIFIED_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(cache) = guard.as_ref() {
        if cache.set.contains(&key) {
            return true;
        }
    }
    drop(guard);
    if e.verify().is_ok() {
        let mut guard = VERIFIED_CACHE.write().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(|| VerifiedCache {
            set: HashSet::with_capacity(1024),
            queue: VecDeque::with_capacity(1024),
        });
        let max_cap = max_cache_capacity();
        if cache.set.insert(key) {
            if cache.set.len() > max_cap {
                if let Some(oldest) = cache.queue.pop_front() {
                    cache.set.remove(&oldest);
                }
            }
            cache.queue.push_back(key);
        }
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
