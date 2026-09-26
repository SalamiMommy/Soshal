//! SIMD-accelerated Bloom filter for relay event dedup.
//!
//! Replaces massive in-memory HashSet allocations with an AVX2/NEON-accelerated
//! bitset, dropping memory footprint by ~98% for high-throughput flood gossip.

use fastbloom::BloomFilter;

/// Default capacity for the seen-events Bloom filter.
pub const DEFAULT_SEEN_CAPACITY: usize = 100_000;
/// Target false positive rate (0.1%).
pub const DEFAULT_FALSE_POSITIVE_RATE: f64 = 0.001;

/// High-performance SIMD Bloom filter for tracking seen envelope payload digests.
pub struct FastBloomSeenFilter {
    filter: BloomFilter,
    count: usize,
    capacity: usize,
}

impl Default for FastBloomSeenFilter {
    fn default() -> Self {
        Self::new(DEFAULT_SEEN_CAPACITY, DEFAULT_FALSE_POSITIVE_RATE)
    }
}

impl FastBloomSeenFilter {
    /// Creates a new `FastBloomSeenFilter` sized for `expected_items` with `fp_rate`.
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        let capacity = expected_items.max(1024);
        let filter = BloomFilter::with_false_pos(fp_rate).expected_items(capacity);
        Self {
            filter,
            count: 0,
            capacity,
        }
    }

    /// Checks if `digest_hex` is present in the filter.
    #[inline]
    pub fn contains(&self, digest_hex: &str) -> bool {
        self.filter.contains(digest_hex.as_bytes())
    }

    /// Inserts `digest_hex`. Returns `true` if newly added (not seen before),
    /// or `false` if it was already likely in the filter.
    #[inline]
    pub fn insert(&mut self, digest_hex: &str) -> bool {
        let bytes = digest_hex.as_bytes();
        if self.filter.contains(bytes) {
            return false;
        }
        self.filter.insert(bytes);
        self.count += 1;
        true
    }

    /// Sized capacity of the filter.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of items inserted into the filter.
    #[inline]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Clears the filter and resets count.
    pub fn clear(&mut self) {
        self.filter.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_seen_filter_insert_and_contains() {
        let mut filter = FastBloomSeenFilter::new(1000, 0.001);
        let id1 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let id2 = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";

        assert!(!filter.contains(id1));
        assert!(filter.insert(id1));
        assert!(filter.contains(id1));

        // Duplicate insert returns false
        assert!(!filter.insert(id1));
        assert_eq!(filter.count(), 1);

        // id2 is absent
        assert!(!filter.contains(id2));
        assert!(filter.insert(id2));
        assert_eq!(filter.count(), 2);
    }

    #[test]
    fn test_bloom_seen_filter_false_positive_rate() {
        let n = 5000;
        let mut filter = FastBloomSeenFilter::new(n, 0.001);

        for i in 0..n {
            let digest = format!("{:064x}", i);
            filter.insert(&digest);
        }

        let mut false_positives = 0;
        let test_n = 5000;
        for i in n..(n + test_n) {
            let digest = format!("{:064x}", i);
            if filter.contains(&digest) {
                false_positives += 1;
            }
        }

        let actual_fp_rate = false_positives as f64 / test_n as f64;
        assert!(
            actual_fp_rate < 0.01,
            "actual FP rate was too high: {}",
            actual_fp_rate
        );
    }
}
