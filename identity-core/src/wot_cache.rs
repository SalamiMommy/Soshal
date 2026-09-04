//! In-memory LRU cache for Web-of-Trust (WoT) distance and trust score lookups.

use crate::wot::TrustScore;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

pub struct WotCache {
    capacity: usize,
    entries: Mutex<(HashMap<String, TrustScore>, VecDeque<String>)>,
}

impl WotCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new((
                HashMap::with_capacity(capacity),
                VecDeque::with_capacity(capacity),
            )),
        }
    }

    pub fn get(&self, pubkey: &str) -> Option<TrustScore> {
        let mut guard = self.entries.lock().ok()?;
        if guard.0.contains_key(pubkey) {
            if let Some(pos) = guard.1.iter().position(|k| k == pubkey) {
                let k = guard.1.remove(pos).unwrap();
                guard.1.push_back(k);
            }
        }
        guard.0.get(pubkey).copied()
    }

    pub fn insert(&self, pubkey: String, score: TrustScore) {
        if let Ok(mut guard) = self.entries.lock() {
            if let std::collections::hash_map::Entry::Occupied(mut e) =
                guard.0.entry(pubkey.clone())
            {
                e.insert(score);
                if let Some(pos) = guard.1.iter().position(|k| k == &pubkey) {
                    let k = guard.1.remove(pos).unwrap();
                    guard.1.push_back(k);
                }
                return;
            }
            while guard.0.len() >= self.capacity && self.capacity > 0 {
                if let Some(oldest) = guard.1.pop_front() {
                    guard.0.remove(&oldest);
                } else {
                    break;
                }
            }
            guard.1.push_back(pubkey.clone());
            guard.0.insert(pubkey, score);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.0.clear();
            guard.1.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wot::TrustScore;

    fn score(val: f64) -> TrustScore {
        TrustScore {
            score: val,
            distance: 0,
            mutual_count: 0,
        }
    }

    #[test]
    fn lru_get_refreshes_recency() {
        let cache = WotCache::new(3);
        cache.insert("A".into(), score(1.0));
        cache.insert("B".into(), score(2.0));
        cache.insert("C".into(), score(3.0));

        // touch A so it becomes most-recent
        assert!(cache.get("A").is_some());

        // D should evict B (the true LRU), not A
        cache.insert("D".into(), score(4.0));
        assert!(cache.get("B").is_none());
        assert!(cache.get("A").is_some());
    }

    #[test]
    fn lru_reinsert_refreshes_recency() {
        let cache = WotCache::new(3);
        cache.insert("A".into(), score(1.0));
        cache.insert("B".into(), score(2.0));
        cache.insert("C".into(), score(3.0));

        // re-insert A (same key) refreshes its position
        cache.insert("A".into(), score(10.0));

        cache.insert("D".into(), score(4.0));
        assert!(cache.get("B").is_none());
        assert!(cache.get("A").is_some());
    }
}
