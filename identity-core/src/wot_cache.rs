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
        let guard = self.entries.lock().ok()?;
        guard.0.get(pubkey).copied()
    }

    pub fn insert(&self, pubkey: String, score: TrustScore) {
        if let Ok(mut guard) = self.entries.lock() {
            if let std::collections::hash_map::Entry::Occupied(mut e) =
                guard.0.entry(pubkey.clone())
            {
                e.insert(score);
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
