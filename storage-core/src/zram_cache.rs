//! ZRAM-style Compressed In-Memory Storage Tier.
//! Keeps hot feed data and active peer indexes in compressed memory buffers using `zstd`
//! to reduce NAND flash page wear and keep queries sub-millisecond.

use soshal_content_core::compress;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Compressed entry stored in ZRAM memory pool.
#[derive(Debug, Clone)]
pub struct ZramEntry {
    pub key: String,
    pub compressed_bytes: Vec<u8>,
    pub uncompressed_size: usize,
    pub last_accessed_secs: i64,
}

/// ZRAM Cache Manager.
#[derive(Clone, Default)]
pub struct ZramCacheManager {
    pub pool: Arc<RwLock<HashMap<String, ZramEntry>>>,
}

impl ZramCacheManager {
    pub fn new() -> Self {
        Self {
            pool: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Stores a JSON object into compressed ZRAM pool.
    pub async fn put(&self, key: &str, payload_json: &str) {
        let compressed_bytes = compress::compress(payload_json.as_bytes()).unwrap_or_default();
        let now = soshal_common_core::format::now_secs();
        let entry = ZramEntry {
            key: key.to_string(),
            compressed_bytes,
            uncompressed_size: payload_json.len(),
            last_accessed_secs: now,
        };
        let mut pool = self.pool.write().await;
        pool.insert(key.to_string(), entry);
    }

    /// Retrieves and decompresses a JSON object from ZRAM pool.
    pub async fn get(&self, key: &str) -> Option<String> {
        let pool = self.pool.read().await;
        if let Some(entry) = pool.get(key) {
            let decompressed = compress::decompress(&entry.compressed_bytes).ok()?;
            if decompressed.is_empty() {
                None
            } else {
                String::from_utf8(decompressed).ok()
            }
        } else {
            None
        }
    }

    /// Evicts entries older than `ttl_secs`.
    pub async fn evict_stale(&self, ttl_secs: i64) -> usize {
        let now = soshal_common_core::format::now_secs();
        let mut pool = self.pool.write().await;
        let initial_len = pool.len();
        pool.retain(|_, entry| (now - entry.last_accessed_secs) < ttl_secs);
        initial_len - pool.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_zram_cache_compression_and_retrieval() {
        let cache = ZramCacheManager::new();
        let sample = "{\"post_id\":\"123\",\"content\":\"Hello world P2P mesh offgrid test\"}";

        cache.put("key1", sample).await;
        let retrieved = cache.get("key1").await;

        assert_eq!(retrieved.unwrap(), sample);
    }

    #[tokio::test]
    async fn test_evict_stale_fresh_survives() {
        let cache = ZramCacheManager::new();
        cache.put("key1", "{\"a\":1}").await;
        assert_eq!(cache.evict_stale(3600).await, 0);
        assert!(cache.get("key1").await.is_some());
    }

    #[tokio::test]
    async fn test_evict_stale_removes_old_entries() {
        let cache = ZramCacheManager::new();
        cache.put("key1", "{\"a\":1}").await;
        let now = soshal_common_core::format::now_secs();
        {
            let mut pool = cache.pool.write().await;
            if let Some(entry) = pool.get_mut("key1") {
                entry.last_accessed_secs = now - 1000;
            }
        }
        assert_eq!(cache.evict_stale(100).await, 1);
        assert!(cache.get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_evict_stale_empty_and_mixed() {
        let cache = ZramCacheManager::new();
        assert_eq!(cache.evict_stale(100).await, 0);

        cache.put("fresh", "{\"a\":1}").await;
        cache.put("stale", "{\"b\":2}").await;
        let now = soshal_common_core::format::now_secs();
        {
            let mut pool = cache.pool.write().await;
            if let Some(entry) = pool.get_mut("stale") {
                entry.last_accessed_secs = now - 1000;
            }
        }
        assert_eq!(cache.evict_stale(100).await, 1);
        assert!(cache.get("fresh").await.is_some());
        assert!(cache.get("stale").await.is_none());
    }
}
