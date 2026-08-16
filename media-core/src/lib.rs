// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod blossom;
pub mod cas;
pub mod chunking;
pub mod decoder;
pub mod freenet_media;
pub mod identicon;
pub mod media;
pub mod prefetcher;
pub mod source;
pub mod thumbhash;

pub fn trim_media_caches(level: soshal_common_core::memory::MemoryPressureLevel) {
    if level != soshal_common_core::memory::MemoryPressureLevel::Normal {
        // Evict LRU media texture buffers and byte caches
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaFile {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub mime_type: String,
    pub created_at: i64,
}
