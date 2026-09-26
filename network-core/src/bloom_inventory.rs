//! Bloom filter inventory summaries for P2P swarm and gossip sync.
//!
//! Enables nodes to exchange compact set membership proofs over low-bandwidth
//! mesh transports (Reticulum, BLE, QUIC) without sending full chunk hash arrays.

use fastbloom::BloomFilter;

/// Maximum number of hashes allowed in a single inventory filter exchange.
pub const MAX_INVENTORY_ITEMS: usize = 50_000;
/// Default target false positive rate for inventory synchronization (0.5%).
pub const DEFAULT_INVENTORY_FP_RATE: f64 = 0.005;

/// Compact inventory filter representing available blob chunks or event IDs.
#[derive(Clone)]
pub struct PeerInventoryBloomFilter {
    filter: BloomFilter,
    item_count: usize,
}

impl PeerInventoryBloomFilter {
    /// Builds a new inventory filter from a slice of hex hashes.
    pub fn from_hashes<T: AsRef<str>>(hashes: &[T], fp_rate: f64) -> Self {
        let count = hashes.len().min(MAX_INVENTORY_ITEMS);
        let capacity = count.max(64);
        let mut filter =
            BloomFilter::with_false_pos(fp_rate.clamp(0.0001, 0.1)).expected_items(capacity);

        for hash in hashes.iter().take(count) {
            filter.insert(hash.as_ref().as_bytes());
        }

        Self {
            filter,
            item_count: count,
        }
    }

    /// Checks if a given hash is likely held in the peer inventory.
    #[inline]
    pub fn contains_hash(&self, hash: &str) -> bool {
        self.filter.contains(hash.as_bytes())
    }

    /// Number of items represented in the inventory.
    #[inline]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Total number of bits in the underlying filter.
    #[inline]
    pub fn num_bits(&self) -> usize {
        self.filter.num_bits()
    }

    /// Filters a list of required target hashes, returning only those present in this inventory.
    pub fn filter_matching<'a, T: AsRef<str>>(&self, required_hashes: &'a [T]) -> Vec<&'a T> {
        required_hashes
            .iter()
            .filter(|h| self.contains_hash(h.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_inventory_membership() {
        let hashes = vec![
            "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
            "ef2d127de37b942baad06145e54b0c619a1f22327b2ebbcfbec78f5564afe39d",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ];

        let inventory = PeerInventoryBloomFilter::from_hashes(&hashes, DEFAULT_INVENTORY_FP_RATE);
        assert_eq!(inventory.item_count(), 3);
        assert!(inventory.num_bits() > 0);

        for h in &hashes {
            assert!(inventory.contains_hash(h));
        }

        let absent = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!inventory.contains_hash(absent));

        let query = vec![hashes[0], absent, hashes[1]];
        let matched = inventory.filter_matching(&query);
        assert_eq!(matched.len(), 2);
        assert_eq!(*matched[0], hashes[0]);
        assert_eq!(*matched[1], hashes[1]);
    }
}
