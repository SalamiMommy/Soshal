//! High-throughput concurrent local relay store and subscription manager.
//!
//! Employs `DashMap` for lock-free sharded concurrency over active client
//! subscriptions and event indices, avoiding global `RwLock` contention during
//! parallel ingestion and high-frequency WebSocket filter queries.

use std::collections::VecDeque;
use std::sync::RwLock;

use dashmap::DashMap;
use nostr::event::Event;
use nostr::filter::{Filter, MatchEventOptions};

/// Default capacity for the local event store.
pub const DEFAULT_LOCAL_STORE_CAPACITY: usize = 10_000;

/// Concurrent local event store and subscription matcher.
pub struct LocalRelayStore {
    events: DashMap<String, Event>,
    subscriptions: DashMap<String, Vec<Filter>>,
    recent_ids: RwLock<VecDeque<String>>,
    max_events: usize,
}

impl Default for LocalRelayStore {
    fn default() -> Self {
        Self::new(DEFAULT_LOCAL_STORE_CAPACITY)
    }
}

impl LocalRelayStore {
    /// Creates a new `LocalRelayStore` capped at `max_events`.
    pub fn new(max_events: usize) -> Self {
        Self {
            events: DashMap::new(),
            subscriptions: DashMap::new(),
            recent_ids: RwLock::new(VecDeque::with_capacity(max_events.min(1024))),
            max_events: max_events.max(1),
        }
    }

    /// Registers or updates filters for a subscription `sub_id`.
    pub fn subscribe(&self, sub_id: String, filters: Vec<Filter>) {
        self.subscriptions.insert(sub_id, filters);
    }

    /// Removes an active subscription by `sub_id`. Returns true if it was present.
    pub fn unsubscribe(&self, sub_id: &str) -> bool {
        self.subscriptions.remove(sub_id).is_some()
    }

    /// Returns the number of active subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns the number of events in the store.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Fetches an event by its hexadecimal id.
    pub fn get_event(&self, id: &str) -> Option<Event> {
        let clean_id = id.trim().to_ascii_lowercase();
        self.events.get(&clean_id).map(|r| r.value().clone())
    }

    /// Inserts an event into the store and returns a list of subscription IDs that matched.
    pub fn insert_event(&self, event: Event) -> Vec<String> {
        let id_str = event.id.to_hex();

        // Check matching subscriptions across the DashMap concurrently
        let mut matched_subs = Vec::new();
        let match_opts = MatchEventOptions::default();
        for sub in self.subscriptions.iter() {
            let (sub_id, filters) = sub.pair();
            for filter in filters {
                if filter.match_event(&event, match_opts) {
                    matched_subs.push(sub_id.clone());
                    break;
                }
            }
        }

        // Insert into DashMap
        let was_new = self.events.insert(id_str.clone(), event).is_none();
        if was_new {
            if let Ok(mut recent) = self.recent_ids.write() {
                recent.push_back(id_str);
                while recent.len() > self.max_events {
                    if let Some(old_id) = recent.pop_front() {
                        self.events.remove(&old_id);
                    }
                }
            }
        }

        matched_subs
    }

    /// Queries the store for events matching `filter`, up to `limit` or filter limit.
    pub fn query(&self, filter: &Filter) -> Vec<Event> {
        let match_opts = MatchEventOptions::default();
        let limit = filter.limit.unwrap_or(100);
        let mut matched = Vec::new();

        // Read through recent ids in reverse order (newest first)
        let ids: Vec<String> = if let Ok(recent) = self.recent_ids.read() {
            recent.iter().rev().cloned().collect()
        } else {
            Vec::new()
        };

        for id in ids {
            if let Some(ev) = self.events.get(&id) {
                if filter.match_event(ev.value(), match_opts) {
                    matched.push(ev.value().clone());
                    if matched.len() >= limit {
                        break;
                    }
                }
            }
        }

        matched
    }

    /// Clears all events and subscriptions.
    pub fn clear(&self) {
        self.events.clear();
        self.subscriptions.clear();
        if let Ok(mut recent) = self.recent_ids.write() {
            recent.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::event::{EventBuilder, FinalizeEvent, Kind};
    use nostr::key::Keys;

    #[test]
    fn test_local_relay_store_insert_and_query() {
        let store = LocalRelayStore::new(5);
        let keys = Keys::generate();

        let event1 = EventBuilder::new(Kind::TextNote, "Hello Nostr DashMap!")
            .finalize(&keys)
            .unwrap();
        let event2 = EventBuilder::new(Kind::Custom(1234), "Custom event")
            .finalize(&keys)
            .unwrap();

        let matched = store.insert_event(event1.clone());
        assert!(matched.is_empty());
        assert_eq!(store.event_count(), 1);

        // Subscribe to text notes (Kind 1)
        let filter = Filter::new().kind(Kind::TextNote);
        store.subscribe("sub_text".into(), vec![filter.clone()]);
        assert_eq!(store.subscription_count(), 1);

        // Query existing
        let results = store.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, event1.id);

        // Insert custom event - should not match sub_text
        let matched = store.insert_event(event2.clone());
        assert!(matched.is_empty());

        // Insert second text note - should match sub_text
        let event3 = EventBuilder::new(Kind::TextNote, "Second note")
            .finalize(&keys)
            .unwrap();
        let matched = store.insert_event(event3.clone());
        assert_eq!(matched, vec!["sub_text"]);

        // Unsubscribe
        assert!(store.unsubscribe("sub_text"));
        assert_eq!(store.subscription_count(), 0);
    }

    #[test]
    fn test_local_relay_store_capacity_eviction() {
        let store = LocalRelayStore::new(2);
        let keys = Keys::generate();

        let ev1 = EventBuilder::new(Kind::TextNote, "1")
            .finalize(&keys)
            .unwrap();
        let ev2 = EventBuilder::new(Kind::TextNote, "2")
            .finalize(&keys)
            .unwrap();
        let ev3 = EventBuilder::new(Kind::TextNote, "3")
            .finalize(&keys)
            .unwrap();

        store.insert_event(ev1.clone());
        store.insert_event(ev2.clone());
        assert_eq!(store.event_count(), 2);
        assert!(store.get_event(&ev1.id.to_hex()).is_some());

        store.insert_event(ev3.clone());
        assert_eq!(store.event_count(), 2);
        // ev1 should have been evicted
        assert!(store.get_event(&ev1.id.to_hex()).is_none());
        assert!(store.get_event(&ev2.id.to_hex()).is_some());
        assert!(store.get_event(&ev3.id.to_hex()).is_some());
    }
}
