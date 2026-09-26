//! In-memory Least Recently Used (LRU) bounded caches for feeds, post rankings, and author profiles.
//!
//! Provides deterministic memory bounding and O(1) cache lookups using `lru`.

use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;

/// A bounded in-memory LRU cache with hit and miss metrics.
#[derive(Debug)]
pub struct BoundedFeedCache<K: Hash + Eq, V> {
    cache: LruCache<K, V>,
    hits: u64,
    misses: u64,
}

impl<K: Hash + Eq, V> BoundedFeedCache<K, V> {
    /// Create a new LRU cache with a maximum capacity bound.
    pub fn new(capacity: usize) -> Result<Self, String> {
        let cap = NonZeroUsize::new(capacity)
            .ok_or_else(|| "cache capacity must be greater than zero".to_string())?;
        Ok(Self {
            cache: LruCache::new(cap),
            hits: 0,
            misses: 0,
        })
    }

    /// Insert a key-value pair. If capacity is exceeded, the least recently used item is evicted.
    /// Returns the prior value if the key already existed.
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        self.cache.put(key, value)
    }

    /// Retrieve a reference to a cached value, updating its recency.
    /// Increments hit/miss counters.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        match self.cache.get(key) {
            Some(val) => {
                self.hits += 1;
                Some(val)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Retrieve a reference to a cached value without updating its LRU recency or hit counters.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.cache.peek(key)
    }

    /// Check if a key is present in the cache.
    pub fn contains(&self, key: &K) -> bool {
        self.cache.contains(key)
    }

    /// Remove an entry by key, returning its value if it was present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.cache.pop(key)
    }

    /// Evict and return the least recently used `(key, value)` entry.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        self.cache.pop_lru()
    }

    /// Number of items currently stored in the cache.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is currently empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Maximum capacity of the cache before eviction occurs.
    pub fn capacity(&self) -> usize {
        self.cache.cap().get()
    }

    /// Clear all items from the cache and reset counters.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Total number of successful cache hits.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Total number of cache misses.
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Cache hit ratio (between 0.0 and 1.0, or 0.0 if no queries were made).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_put_get_and_metrics() {
        let mut cache = BoundedFeedCache::<String, i32>::new(2).unwrap();
        assert_eq!(cache.capacity(), 2);
        assert!(cache.is_empty());

        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.hits(), 0);

        cache.put("a".to_string(), 100);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&"a".to_string()), Some(&100));
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.hit_rate(), 0.5);
    }

    #[test]
    fn lru_eviction() {
        let mut cache = BoundedFeedCache::<&str, &str>::new(2).unwrap();
        cache.put("k1", "v1");
        cache.put("k2", "v2");

        // Access k1 so k2 becomes the least recently used
        assert_eq!(cache.get(&"k1"), Some(&"v1"));

        // Inserting k3 must evict k2
        cache.put("k3", "v3");

        assert_eq!(cache.len(), 2);
        assert!(cache.contains(&"k1"));
        assert!(cache.contains(&"k3"));
        assert!(!cache.contains(&"k2"));
    }

    #[test]
    fn peek_does_not_alter_order() {
        let mut cache = BoundedFeedCache::<&str, i32>::new(2).unwrap();
        cache.put("first", 1);
        cache.put("second", 2);

        // Peek "first": should not alter LRU order
        assert_eq!(cache.peek(&"first"), Some(&1));

        // Insert "third": should evict "first" since it was not accessed via get()
        cache.put("third", 3);

        assert!(!cache.contains(&"first"));
        assert!(cache.contains(&"second"));
        assert!(cache.contains(&"third"));
    }

    #[test]
    fn clear_resets_everything() {
        let mut cache = BoundedFeedCache::<i32, i32>::new(5).unwrap();
        cache.put(1, 10);
        cache.put(2, 20);
        let _ = cache.get(&1);
        let _ = cache.get(&999); // miss

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.hit_rate(), 0.0);
    }

    #[test]
    fn zero_capacity_rejected() {
        assert!(BoundedFeedCache::<i32, i32>::new(0).is_err());
    }
}
