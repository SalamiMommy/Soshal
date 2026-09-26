//! In-memory radix trie autocomplete for fast prefix matching, hashtags, and mentions.

use radix_trie::{Trie, TrieCommon};
use serde::{Deserialize, Serialize};

/// High-performance prefix autocomplete index backed by a Radix / Patricia Trie.
///
/// Lookups are $O(k)$ where $k$ is the length of the query prefix, independent
/// of the total number of entries stored.
#[derive(Debug, Clone)]
pub struct RadixAutocomplete<V> {
    trie: Trie<String, V>,
}

impl<V> Default for RadixAutocomplete<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> RadixAutocomplete<V> {
    /// Create a new empty autocomplete trie.
    pub fn new() -> Self {
        Self { trie: Trie::new() }
    }

    /// Insert or update a key-value entry. Returns previous value if existing.
    pub fn insert(&mut self, key: impl Into<String>, value: V) -> Option<V> {
        self.trie.insert(key.into(), value)
    }

    /// Get reference to the value associated with an exact key.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.trie.get(key)
    }

    /// Check if a key exists in the trie.
    pub fn contains_key(&self, key: &str) -> bool {
        self.trie.get(key).is_some()
    }

    /// Check if any key with the given prefix exists in the trie.
    pub fn contains_prefix(&self, prefix: &str) -> bool {
        self.trie.get_raw_descendant(prefix).is_some()
    }

    /// Remove a key from the trie and return its value if present.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.trie.remove(key)
    }

    /// Find all entries whose keys start with `prefix`, up to `limit` entries.
    pub fn find_prefix(&self, prefix: &str, limit: usize) -> Vec<(&str, &V)> {
        if limit == 0 {
            return Vec::new();
        }

        match self.trie.get_raw_descendant(prefix) {
            Some(subtrie) => subtrie
                .iter()
                .take(limit)
                .map(|(k, v)| (k.as_str(), v))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Returns the number of distinct keys in the trie.
    pub fn len(&self) -> usize {
        self.trie.len()
    }

    /// Returns true if the trie contains no entries.
    pub fn is_empty(&self) -> bool {
        self.trie.is_empty()
    }

    /// Clear all entries from the trie.
    pub fn clear(&mut self) {
        self.trie = Trie::new();
    }
}

/// Autocomplete match result item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutocompleteMatch {
    pub key: String,
    pub score: u32,
}

/// Suggestion index for hashtags with frequency weighting.
#[derive(Debug, Clone, Default)]
pub struct HashtagAutocomplete {
    index: RadixAutocomplete<u32>, // hashtag -> usage count
}

impl HashtagAutocomplete {
    pub fn new() -> Self {
        Self {
            index: RadixAutocomplete::new(),
        }
    }

    /// Register or increment a hashtag (case-normalized to lowercase, leading `#` stripped).
    pub fn record_hashtag(&mut self, tag: &str) {
        let normalized = tag.trim_start_matches('#').to_lowercase();
        if normalized.is_empty() {
            return;
        }
        let count = self.index.get(&normalized).copied().unwrap_or(0);
        self.index.insert(normalized, count.saturating_add(1));
    }

    /// Complete a hashtag prefix, sorted by popularity (usage count descending).
    pub fn complete(&self, prefix: &str, limit: usize) -> Vec<AutocompleteMatch> {
        let normalized = prefix.trim_start_matches('#').to_lowercase();
        let mut matches: Vec<AutocompleteMatch> = self
            .index
            .find_prefix(&normalized, limit * 2)
            .into_iter()
            .map(|(k, &count)| AutocompleteMatch {
                key: format!("#{k}"),
                score: count,
            })
            .collect();

        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.key.cmp(&b.key)));
        matches.truncate(limit);
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radix_autocomplete_crud() {
        let mut trie = RadixAutocomplete::new();
        assert!(trie.is_empty());
        assert_eq!(trie.len(), 0);

        trie.insert("nostr", 100);
        trie.insert("nostalgia", 50);
        trie.insert("network", 200);

        assert_eq!(trie.len(), 3);
        assert!(!trie.is_empty());
        assert_eq!(trie.get("nostr"), Some(&100));
        assert_eq!(trie.get("none"), None);

        assert!(trie.contains_prefix("nos"));
        assert!(trie.contains_prefix("net"));
        assert!(!trie.contains_prefix("xyz"));

        let prefix_matches = trie.find_prefix("nos", 10);
        assert_eq!(prefix_matches.len(), 2);

        assert_eq!(trie.remove("nostalgia"), Some(50));
        assert_eq!(trie.len(), 2);
        assert_eq!(trie.find_prefix("nos", 10).len(), 1);

        trie.clear();
        assert!(trie.is_empty());
    }

    #[test]
    fn test_hashtag_autocomplete_popularity() {
        let mut hashtags = HashtagAutocomplete::new();
        hashtags.record_hashtag("nostr");
        hashtags.record_hashtag("nostr");
        hashtags.record_hashtag("nostr");
        hashtags.record_hashtag("nostalgia");
        hashtags.record_hashtag("note");
        hashtags.record_hashtag("#news");

        let results = hashtags.complete("no", 5);
        assert_eq!(results.len(), 3);
        // #nostr has score 3, so it must be first
        assert_eq!(results[0].key, "#nostr");
        assert_eq!(results[0].score, 3);

        // #news match test
        let news = hashtags.complete("#ne", 5);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].key, "#news");
    }

    #[test]
    fn test_empty_query_and_limits() {
        let mut trie = RadixAutocomplete::new();
        trie.insert("a", 1);
        trie.insert("ab", 2);
        trie.insert("abc", 3);

        assert_eq!(trie.find_prefix("a", 0).len(), 0);
        assert_eq!(trie.find_prefix("a", 2).len(), 2);
    }
}
