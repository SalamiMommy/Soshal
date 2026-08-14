//! Cuckoo Filter Negative Caching for P2P media chunk presence.
//!
//! Sub-nanosecond O(1) in-memory filter checks reject non-existent mesh chunk queries
//! without hitting SQLite or waking up disk flash storage.

use cuckoo_filter::CuckooFilter;
use std::sync::RwLock;

pub struct ChunkCuckooFilter {
    filter: RwLock<CuckooFilter>,
}

impl ChunkCuckooFilter {
    /// Creates a new Cuckoo Filter initialized with max capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            filter: RwLock::new(CuckooFilter::new(capacity)),
        }
    }

    /// Checks if a chunk hash is potentially present in local storage.
    /// Returns `false` for Definite Negative (0% false negative rate).
    pub fn contains(&self, chunk_hash: &[u8; 32]) -> bool {
        if let Ok(guard) = self.filter.read() {
            guard.contains(chunk_hash.as_slice())
        } else {
            true // Fallback conservatively to true if lock is poisoned
        }
    }

    /// Inserts a chunk hash into the negative filter.
    pub fn insert(&self, chunk_hash: &[u8; 32]) -> Result<(), String> {
        if let Ok(mut guard) = self.filter.write() {
            if guard.insert(chunk_hash.as_slice()) {
                Ok(())
            } else {
                Err("Cuckoo filter capacity exhausted".to_string())
            }
        } else {
            Err("Cuckoo filter lock poisoned".to_string())
        }
    }

    /// Inserts a slice of chunk hashes in a single batch write lock acquisition.
    pub fn insert_batch(&self, hashes: &[[u8; 32]]) -> Result<usize, String> {
        if let Ok(mut guard) = self.filter.write() {
            let mut count = 0;
            for hash in hashes {
                if guard.insert(hash.as_slice()) {
                    count += 1;
                } else {
                    break;
                }
            }
            Ok(count)
        } else {
            Err("Cuckoo filter lock poisoned".to_string())
        }
    }

    /// Deletes a chunk hash from the negative filter.
    pub fn delete(&self, chunk_hash: &[u8; 32]) -> bool {
        if let Ok(mut guard) = self.filter.write() {
            guard.remove(chunk_hash.as_slice())
        } else {
            false
        }
    }
}

impl Default for ChunkCuckooFilter {
    fn default() -> Self {
        Self::new(100_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cuckoo_filter_insert_contains_delete() {
        let filter = ChunkCuckooFilter::new(1000);
        let hash_a = [1u8; 32];
        let hash_b = [2u8; 32];

        assert!(!filter.contains(&hash_a));

        filter.insert(&hash_a).unwrap();
        assert!(filter.contains(&hash_a));
        assert!(!filter.contains(&hash_b));

        filter.delete(&hash_a);
        assert!(!filter.contains(&hash_a));
    }

    #[test]
    fn test_insert_batch_many_and_empty() {
        let filter = ChunkCuckooFilter::new(1000);
        let hashes: Vec<[u8; 32]> = (0..10).map(|i| [i as u8; 32]).collect();
        assert_eq!(filter.insert_batch(&hashes).unwrap(), 10);
        for h in &hashes {
            assert!(filter.contains(h));
        }
        assert_eq!(filter.insert_batch(&[]).unwrap(), 0);
    }

    #[test]
    fn test_insert_batch_duplicate_overwrite() {
        let filter = ChunkCuckooFilter::new(1000);
        let hash = [7u8; 32];
        assert_eq!(filter.insert_batch(&[hash]).unwrap(), 1);
        assert_eq!(filter.insert_batch(&[hash]).unwrap(), 1);
        assert!(filter.contains(&hash));
    }

    #[test]
    fn test_insert_batch_under_capacity() {
        let filter = ChunkCuckooFilter::new(4);
        let hashes: Vec<[u8; 32]> = (0..3).map(|i| [i as u8; 32]).collect();
        assert_eq!(filter.insert_batch(&hashes).unwrap(), 3);
        for h in &hashes {
            assert!(filter.contains(h));
        }
    }
}
