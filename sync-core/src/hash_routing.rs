//! High-throughput XXHash non-cryptographic hashing, partition routing, and consistent
//! hashing for sync workers and event deduplication.

use std::collections::BTreeMap;
use std::hash::Hasher;
use twox_hash::XxHash64;

/// Calculate a 64-bit XXHash of a byte slice running at memory bus speeds (10-15 GB/s).
pub fn xxhash64_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = XxHash64::default();
    hasher.write(bytes);
    hasher.finish()
}

/// Calculate a 64-bit XXHash of a string slice.
pub fn xxhash64_str(s: &str) -> u64 {
    xxhash64_bytes(s.as_bytes())
}

/// Route keys and events into `N` discrete partition queues or worker threads.
#[derive(Debug, Clone)]
pub struct PartitionRouter {
    num_partitions: usize,
}

impl PartitionRouter {
    /// Create a new partition router with a fixed number of partitions (minimum 1).
    pub fn new(num_partitions: usize) -> Self {
        Self {
            num_partitions: num_partitions.max(1),
        }
    }

    /// Number of configured partitions.
    pub fn num_partitions(&self) -> usize {
        self.num_partitions
    }

    /// Map a byte key to its assigned partition index in `0..num_partitions`.
    pub fn route_bytes(&self, bytes: &[u8]) -> usize {
        (xxhash64_bytes(bytes) as usize) % self.num_partitions
    }

    /// Map a string key (e.g. event ID or pubkey) to its assigned partition index.
    pub fn route_key(&self, key: &str) -> usize {
        self.route_bytes(key.as_bytes())
    }

    /// Partition a batch of items into separate buckets according to an extracted key.
    pub fn partition_items<T, F>(&self, items: Vec<T>, key_fn: F) -> Vec<Vec<T>>
    where
        F: Fn(&T) -> &str,
    {
        let mut buckets = Vec::with_capacity(self.num_partitions);
        for _ in 0..self.num_partitions {
            buckets.push(Vec::new());
        }

        for item in items {
            let key = key_fn(&item);
            let idx = self.route_key(key);
            buckets[idx].push(item);
        }

        buckets
    }
}

/// Consistent hashing ring backed by XXHash for dynamically distributing relays and peers.
#[derive(Debug, Clone, Default)]
pub struct ConsistentHashRing {
    ring: BTreeMap<u64, String>,
}

impl ConsistentHashRing {
    /// Create a new empty consistent hash ring.
    pub fn new() -> Self {
        Self {
            ring: BTreeMap::new(),
        }
    }

    /// Add a node to the ring with `vnodes` virtual node points (recommended: 20-50).
    pub fn add_node(&mut self, node: &str, vnodes: usize) {
        let count = vnodes.max(1);
        for i in 0..count {
            let vnode_key = format!("{node}#vn{i}");
            let hash = xxhash64_str(&vnode_key);
            self.ring.insert(hash, node.to_string());
        }
    }

    /// Remove a node and all of its virtual points from the ring.
    pub fn remove_node(&mut self, node: &str) {
        self.ring.retain(|_, v| v != node);
    }

    /// Locate the responsible node for a given key. Returns None if ring is empty.
    pub fn get_node(&self, key: &str) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }

        let key_hash = xxhash64_str(key);

        // Find the first vnode with hash >= key_hash (clockwise search)
        if let Some((_, node)) = self.ring.range(key_hash..).next() {
            return Some(node.as_str());
        }

        // If wrapped around the end of the ring, choose the first element
        self.ring.values().next().map(|s| s.as_str())
    }

    /// Whether the ring contains any nodes.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Total number of virtual nodes on the ring.
    pub fn len(&self) -> usize {
        self.ring.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xxhash64_deterministic() {
        let hash1 = xxhash64_str("nostr_event_id_123");
        let hash2 = xxhash64_str("nostr_event_id_123");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, 0);

        let hash_diff = xxhash64_str("nostr_event_id_124");
        assert_ne!(hash1, hash_diff);
    }

    #[test]
    fn test_partition_router() {
        let router = PartitionRouter::new(4);
        assert_eq!(router.num_partitions(), 4);

        let items = vec![
            ("event1", "payload1"),
            ("event2", "payload2"),
            ("event3", "payload3"),
            ("event4", "payload4"),
            ("event5", "payload5"),
        ];

        let buckets = router.partition_items(items, |(id, _)| *id);
        assert_eq!(buckets.len(), 4);

        let total_items: usize = buckets.iter().map(|b| b.len()).sum();
        assert_eq!(total_items, 5);
    }

    #[test]
    fn test_consistent_hash_ring() {
        let mut ring = ConsistentHashRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.get_node("event_key"), None);

        ring.add_node("relay1.damus.io", 20);
        ring.add_node("relay2.nos.lol", 20);
        ring.add_node("relay3.primal.net", 20);

        assert_eq!(ring.len(), 60);

        let target_node = ring.get_node("user_pubkey_001").expect("node exists");
        assert!(
            target_node == "relay1.damus.io"
                || target_node == "relay2.nos.lol"
                || target_node == "relay3.primal.net"
        );

        // Same key must always map to same node
        assert_eq!(ring.get_node("user_pubkey_001"), Some(target_node));

        ring.remove_node("relay1.damus.io");
        assert_eq!(ring.len(), 40);
        assert_ne!(ring.get_node("user_pubkey_001"), Some("relay1.damus.io"));
    }
}
