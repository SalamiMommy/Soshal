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
        let key = pubkey.to_ascii_lowercase();
        if guard.0.contains_key(&key) {
            if let Some(pos) = guard.1.iter().position(|k| k == &key) {
                let k = guard.1.remove(pos).unwrap();
                guard.1.push_back(k);
            }
        }
        guard.0.get(&key).copied()
    }

    pub fn insert(&self, pubkey: String, score: TrustScore) {
        let pubkey = pubkey.to_ascii_lowercase();
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
            while self.capacity > 0 && guard.0.len() >= self.capacity {
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

    #[test]
    fn zero_capacity_cache_is_unbounded() {
        let cache = WotCache::new(0);
        cache.insert("A".into(), score(1.0));
        assert_eq!(cache.get("A").map(|s| s.score), Some(1.0));
    }

    #[test]
    fn cache_case_insensitivity() {
        let cache = WotCache::new(2);
        cache.insert("AbCd".into(), score(5.0));
        assert_eq!(cache.get("abcd").map(|s| s.score), Some(5.0));
        assert_eq!(cache.get("ABCD").map(|s| s.score), Some(5.0));
    }
}
