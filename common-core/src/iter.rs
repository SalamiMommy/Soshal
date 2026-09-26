//! High-performance iterator adaptors and batching helpers built on `itertools`.

use itertools::Itertools;
use std::hash::Hash;

/// Chunk a collection into fixed-size batches of at most `batch_size`.
///
/// If `batch_size == 0`, defaults to batch size 1 to prevent infinite loops.
pub fn batch_items<I, T>(iter: I, batch_size: usize) -> Vec<Vec<T>>
where
    I: IntoIterator<Item = T>,
{
    let size = batch_size.max(1);
    iter.into_iter()
        .chunks(size)
        .into_iter()
        .map(|chunk| chunk.collect())
        .collect()
}

/// Deduplicate elements of an iterator preserving the first-seen occurrence order.
pub fn unique_by_key<I, T, K, F>(iter: I, key_fn: F) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    K: Eq + Hash,
    F: FnMut(&T) -> K,
{
    iter.into_iter().unique_by(key_fn).collect()
}

/// Interleave elements from two iterators alternating one-by-one.
///
/// Useful for interleaving organic feed items with promoted or recommendation items.
pub fn interleave_items<I1, I2, T>(iter1: I1, iter2: I2) -> Vec<T>
where
    I1: IntoIterator<Item = T>,
    I2: IntoIterator<Item = T>,
{
    iter1.into_iter().interleave(iter2).collect()
}

/// Group items by consecutive identical keys.
pub fn group_consecutive_by<I, T, K, F>(iter: I, key_fn: F) -> Vec<(K, Vec<T>)>
where
    I: IntoIterator<Item = T>,
    K: PartialEq,
    F: Fn(&T) -> K,
{
    iter.into_iter()
        .chunk_by(key_fn)
        .into_iter()
        .map(|(key, group)| (key, group.collect()))
        .collect()
}

/// Generate all pairs (Cartesian product) from two slices.
pub fn cartesian_pairs<T: Clone, U: Clone>(a: &[T], b: &[U]) -> Vec<(T, U)> {
    a.iter()
        .cartesian_product(b.iter())
        .map(|(x, y)| (x.clone(), y.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_items() {
        let items = vec![1, 2, 3, 4, 5, 6, 7];
        let batches = batch_items(items, 3);
        assert_eq!(batches, vec![vec![1, 2, 3], vec![4, 5, 6], vec![7]]);

        let empty: Vec<i32> = vec![];
        assert_eq!(batch_items(empty, 3), Vec::<Vec<i32>>::new());

        let single = batch_items(vec![1, 2], 0);
        assert_eq!(single, vec![vec![1], vec![2]]);
    }

    #[test]
    fn test_unique_by_key() {
        #[derive(Debug, PartialEq, Clone)]
        struct Post {
            id: u32,
            author: &'static str,
        }

        let posts = vec![
            Post {
                id: 1,
                author: "alice",
            },
            Post {
                id: 2,
                author: "bob",
            },
            Post {
                id: 3,
                author: "alice",
            },
            Post {
                id: 4,
                author: "charlie",
            },
            Post {
                id: 5,
                author: "bob",
            },
        ];

        let deduped = unique_by_key(posts, |p| p.author);
        assert_eq!(deduped.len(), 3);
        assert_eq!(deduped[0].id, 1);
        assert_eq!(deduped[1].id, 2);
        assert_eq!(deduped[2].id, 4);
    }

    #[test]
    fn test_interleave_items() {
        let feed = vec!["post1", "post2", "post3"];
        let ads = vec!["ad1", "ad2"];
        let mixed = interleave_items(feed, ads);
        assert_eq!(mixed, vec!["post1", "ad1", "post2", "ad2", "post3"]);
    }

    #[test]
    fn test_group_consecutive_by() {
        let items = vec!["apple", "apricot", "banana", "blueberry", "cherry"];
        let groups = group_consecutive_by(items, |s| s.chars().next().unwrap());

        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, 'a');
        assert_eq!(groups[0].1, vec!["apple", "apricot"]);
        assert_eq!(groups[1].0, 'b');
        assert_eq!(groups[1].1, vec!["banana", "blueberry"]);
        assert_eq!(groups[2].0, 'c');
        assert_eq!(groups[2].1, vec!["cherry"]);
    }

    #[test]
    fn test_cartesian_pairs() {
        let peers = vec!["peer1", "peer2"];
        let relays = vec![8080, 8081];
        let pairs = cartesian_pairs(&peers, &relays);
        assert_eq!(
            pairs,
            vec![
                ("peer1", 8080),
                ("peer1", 8081),
                ("peer2", 8080),
                ("peer2", 8081),
            ]
        );
    }
}
