//! Bounded FIFO dedup collections shared across cores.
//!
//! Several cores hand-implement the same "bounded FIFO dedup set": a
//! `HashSet` plus a parallel `VecDeque` tracking insertion order, evicting
//! the oldest entry once the set exceeds a numeric cap. This module folds
//! that pattern into one generic, dependency-free type (`BoundedSet`) plus a
//! value-carrying variant (`BoundedMap`) so eviction order and caps stay the
//! same everywhere instead of being re-implemented per crate.
//!
//! Semantics mirror the canonical insert-then-trim snippet: on a genuinely
//! new key, the key is recorded in FIFO order and, once `len() > capacity`,
//! the oldest entry is evicted. Behaviorally identical to the classic
//! per-site loops (insert → `while len > cap { pop_front; remove }`), which
//! also matches sites that trim pre-insert at `len >= cap` for fresh keys
//! (the observable post-call state is unchanged).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::Hash;

/// Bounded FIFO dedup set of `T` keys. Inserts a key exactly once (like a
/// `HashSet`), tracking insertion order and evicting the oldest entry when
/// the set would otherwise exceed `capacity`.
pub struct BoundedSet<T> {
    set: HashSet<T>,
    order: VecDeque<T>,
    capacity: usize,
}

impl<T: Eq + Hash + Clone> BoundedSet<T> {
    /// Creates an empty set that keeps at most `capacity` entries, evicting
    /// the oldest (FIFO) once capacity is reached.
    pub fn new(capacity: usize) -> Self {
        Self {
            set: HashSet::with_capacity(capacity.min(4096)),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// Inserts `key`. Returns `true` if it was newly added, `false` if it
    /// was already present. On a fresh insert that pushes the set past
    /// `capacity`, the oldest entry is evicted.
    pub fn insert(&mut self, key: T) -> bool {
        if self.set.insert(key.clone()) {
            self.order.push_back(key);
            while self.order.len() > self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.set.remove(&oldest);
                }
            }
            true
        } else {
            false
        }
    }

    /// Returns `true` if `key` is present.
    pub fn contains(&self, key: &T) -> bool {
        self.set.contains(key)
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Returns `true` if the set holds no entries.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.set.clear();
        self.order.clear();
    }
}

impl<T: fmt::Debug> fmt::Debug for BoundedSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.order.iter()).finish()
    }
}

/// Bounded FIFO key→value map. `insert` records a value under `key`, tracking
/// insertion order once per key and evicting the oldest key (and its value)
/// when the map would otherwise exceed `capacity`.
pub struct BoundedMap<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> BoundedMap<K, V> {
    /// Creates an empty map that keeps at most `capacity` entries, evicting
    /// the oldest (FIFO) once capacity is reached.
    pub fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity.min(4096)),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// Inserts `value` under `key`, returning the previous value if `key`
    /// was already present. A brand-new key pushes the map past `capacity`
    /// and evicts the oldest entry. Re-inserting an existing key updates its
    /// value without changing its position in the eviction order.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let existed = self.map.contains_key(&key);
        if !existed {
            if self.order.len() >= self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.map.remove(&oldest);
                }
            }
            self.order.push_back(key.clone());
        }
        self.map.insert(key, value)
    }

    /// Returns the value stored under `key`, if any.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Returns `true` if `key` is present.
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Removes `key` and its value, returning the value if it was present.
    /// The key is dropped from the eviction order regardless of position.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let removed = self.map.remove(key);
        if removed.is_some() {
            self.order.retain(|k| k != key);
        }
        removed
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the map holds no entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for BoundedMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.map.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_set_evicts_oldest_fifo() {
        let mut s: BoundedSet<&str> = BoundedSet::new(3);
        assert!(s.insert("a"));
        assert!(s.insert("b"));
        assert!(s.insert("c"));
        assert!(s.insert("d"));
        assert_eq!(s.len(), 3);
        assert!(!s.contains(&"a"));
        assert!(s.contains(&"b"));
        assert!(s.contains(&"c"));
        assert!(s.contains(&"d"));
    }

    #[test]
    fn bounded_set_duplicate_is_noop() {
        let mut s: BoundedSet<&str> = BoundedSet::new(2);
        s.insert("a");
        assert!(!s.insert("a"));
        assert_eq!(s.len(), 1);
        s.insert("b");
        assert!(s.contains(&"a"));
        assert!(s.contains(&"b"));
    }

    #[test]
    fn bounded_set_clear() {
        let mut s: BoundedSet<&str> = BoundedSet::new(2);
        s.insert("a");
        s.insert("b");
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn bounded_map_evicts_oldest_fifo() {
        let mut m: BoundedMap<&str, i32> = BoundedMap::new(2);
        assert_eq!(m.insert("a", 1), None);
        assert_eq!(m.insert("b", 2), None);
        assert_eq!(m.insert("c", 3), None);
        assert!(!m.contains_key(&"a"));
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&"b"), Some(&2));
        assert_eq!(m.get(&"c"), Some(&3));
    }

    #[test]
    fn bounded_map_update_keeps_position() {
        let mut m: BoundedMap<&str, i32> = BoundedMap::new(2);
        m.insert("a", 1);
        m.insert("b", 2);
        assert_eq!(m.insert("a", 10), Some(1));
        // FIFO order unchanged by update: c pushes past cap and evicts the
        // oldest-inserted key, "a".
        assert_eq!(m.insert("c", 3), None);
        assert!(!m.contains_key(&"a"));
        assert!(m.contains_key(&"b"));
        assert!(m.contains_key(&"c"));
    }

    #[test]
    fn bounded_map_remove_mid_order() {
        let mut m: BoundedMap<&str, i32> = BoundedMap::new(4);
        m.insert("a", 1);
        m.insert("b", 2);
        m.insert("c", 3);
        m.insert("d", 4);
        assert_eq!(m.remove(&"b"), Some(2));
        assert_eq!(m.len(), 3);
        // Eviction order still FIFO after mid-order remove: filling past
        // capacity evicts "a" (oldest still present).
        m.insert("e", 5);
        m.insert("f", 6);
        assert!(!m.contains_key(&"a"));
        assert!(!m.contains_key(&"b"));
        assert!(m.contains_key(&"c"));
        assert!(m.contains_key(&"d"));
        assert!(m.contains_key(&"e"));
        assert!(m.contains_key(&"f"));
    }
}
