//! High-performance compressed bitmap index for sync tracking and set operations.
//!
//! Uses Roaring Bitmaps (`roaring::RoaringTreemap` for 64-bit event IDs, sequence numbers,
//! and timestamps) to perform ultra-fast set membership, union, intersection, and difference
//! operations with minimal memory footprint and zero platform dependencies.

use roaring::RoaringTreemap;

/// High-performance compressed bitmap index for 64-bit identifiers.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SyncBitmapIndex {
    treemap: RoaringTreemap,
}

impl std::fmt::Debug for SyncBitmapIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncBitmapIndex")
            .field("len", &self.treemap.len())
            .field("min", &self.treemap.min())
            .field("max", &self.treemap.max())
            .finish()
    }
}

impl FromIterator<u64> for SyncBitmapIndex {
    fn from_iter<I: IntoIterator<Item = u64>>(iter: I) -> Self {
        Self {
            treemap: iter.into_iter().collect(),
        }
    }
}

impl SyncBitmapIndex {
    /// Create an empty bitmap index.
    pub fn new() -> Self {
        Self {
            treemap: RoaringTreemap::new(),
        }
    }

    /// Insert an integer into the set. Returns `true` if the value was newly inserted.
    pub fn insert(&mut self, value: u64) -> bool {
        self.treemap.insert(value)
    }

    /// Remove an integer from the set. Returns `true` if the value was present.
    pub fn remove(&mut self, value: u64) -> bool {
        self.treemap.remove(value)
    }

    /// Returns `true` if the set contains the given integer.
    pub fn contains(&self, value: u64) -> bool {
        self.treemap.contains(value)
    }

    /// Number of distinct integers in the set.
    pub fn len(&self) -> u64 {
        self.treemap.len()
    }

    /// Returns `true` if the set contains no elements.
    pub fn is_empty(&self) -> bool {
        self.treemap.is_empty()
    }

    /// Clear all elements from the set.
    pub fn clear(&mut self) {
        self.treemap.clear();
    }

    /// Return the minimum integer in the set, if any.
    pub fn min(&self) -> Option<u64> {
        self.treemap.min()
    }

    /// Return the maximum integer in the set, if any.
    pub fn max(&self) -> Option<u64> {
        self.treemap.max()
    }

    /// Compute the union of two sets (\(A \cup B\)).
    pub fn union(&self, other: &Self) -> Self {
        Self {
            treemap: &self.treemap | &other.treemap,
        }
    }

    /// Compute the intersection of two sets (\(A \cap B\)).
    pub fn intersection(&self, other: &Self) -> Self {
        Self {
            treemap: &self.treemap & &other.treemap,
        }
    }

    /// Compute the difference of two sets (\(A \setminus B\)).
    pub fn difference(&self, other: &Self) -> Self {
        Self {
            treemap: &self.treemap - &other.treemap,
        }
    }

    /// Compute the symmetric difference of two sets (\(A \Delta B\)).
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        Self {
            treemap: &self.treemap ^ &other.treemap,
        }
    }

    /// Returns `true` if this set is a subset of `other`.
    pub fn is_subset(&self, other: &Self) -> bool {
        self.treemap.is_subset(&other.treemap)
    }

    /// Returns `true` if this set shares no elements with `other`.
    pub fn is_disjoint(&self, other: &Self) -> bool {
        self.treemap.is_disjoint(&other.treemap)
    }

    /// Return all values in sorted order.
    pub fn to_vec(&self) -> Vec<u64> {
        self.treemap.iter().collect()
    }

    /// Serialize the roaring bitmap into compact binary format.
    pub fn serialize(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.treemap.serialize_into(&mut buf)?;
        Ok(buf)
    }

    /// Deserialize a roaring bitmap from compact binary format.
    pub fn deserialize(bytes: &[u8]) -> std::io::Result<Self> {
        let treemap = RoaringTreemap::deserialize_from(bytes)?;
        Ok(Self { treemap })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_set_operations() {
        let mut index = SyncBitmapIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);

        assert!(index.insert(10));
        assert!(index.insert(20));
        assert!(index.insert(30));
        assert!(!index.insert(10)); // Duplicate

        assert_eq!(index.len(), 3);
        assert!(index.contains(10));
        assert!(index.contains(20));
        assert!(index.contains(30));
        assert!(!index.contains(40));

        assert_eq!(index.min(), Some(10));
        assert_eq!(index.max(), Some(30));

        assert!(index.remove(20));
        assert!(!index.contains(20));
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn union_intersection_difference() {
        let a = SyncBitmapIndex::from_iter(vec![1, 2, 3, 4, 5]);
        let b = SyncBitmapIndex::from_iter(vec![4, 5, 6, 7, 8]);

        let u = a.union(&b);
        assert_eq!(u.to_vec(), vec![1, 2, 3, 4, 5, 6, 7, 8]);

        let i = a.intersection(&b);
        assert_eq!(i.to_vec(), vec![4, 5]);

        let d = a.difference(&b);
        assert_eq!(d.to_vec(), vec![1, 2, 3]);

        let sym = a.symmetric_difference(&b);
        assert_eq!(sym.to_vec(), vec![1, 2, 3, 6, 7, 8]);

        let sub = SyncBitmapIndex::from_iter(vec![2, 3]);
        assert!(sub.is_subset(&a));
        assert!(!a.is_subset(&sub));

        let disj = SyncBitmapIndex::from_iter(vec![100, 200]);
        assert!(a.is_disjoint(&disj));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut original = SyncBitmapIndex::new();
        for v in [1, 100, 10_000, 1_000_000, 10_000_000_000_u64] {
            original.insert(v);
        }

        let bytes = original.serialize().expect("serialize should succeed");
        assert!(!bytes.is_empty());

        let restored = SyncBitmapIndex::deserialize(&bytes).expect("deserialize should succeed");
        assert_eq!(original, restored);
        assert_eq!(restored.to_vec(), original.to_vec());
    }

    #[test]
    fn sparse_and_dense_scale() {
        let mut index = SyncBitmapIndex::new();
        // Dense range
        for i in 1000..2000 {
            index.insert(i);
        }
        // Sparse IDs
        index.insert(1 << 40);
        index.insert(1 << 50);

        assert_eq!(index.len(), 1002);
        assert_eq!(index.min(), Some(1000));
        assert_eq!(index.max(), Some(1 << 50));

        let bytes = index.serialize().unwrap();
        let restored = SyncBitmapIndex::deserialize(&bytes).unwrap();
        assert_eq!(restored.len(), 1002);
    }
}
