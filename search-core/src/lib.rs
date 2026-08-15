// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod event_map;
pub mod fts5;
pub mod row_map;
pub mod vector_search;

pub use soshal_content_core::fts5::{MAX_FTS5_TERMS, MAX_FTS5_TERM_LEN};

/// Maximum number of tags an event stub may carry.
pub use soshal_common_core::consts::MAX_TAGS;

/// Maximum length of the user-content / post-content / row-content fields.
pub use soshal_common_core::consts::MAX_CONTENT_BYTES as MAX_CONTENT_LEN;
