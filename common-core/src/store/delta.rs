//! Targeted reactive delta updates emitted to Dart services over FFI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntityDelta {
    UserUpdated {
        pubkey: String,
        name: Option<String>,
        avatar_url: Option<String>,
        nip05: Option<String>,
    },
    PostReactionAdded {
        post_id: String,
        like_count: u32,
        repost_count: u32,
        zap_amount_sats: u64,
        user_liked: bool,
    },
    PostBookmarkToggled {
        post_id: String,
        bookmarked: bool,
    },
    PostDeleted {
        post_id: String,
    },
    StoreCleared,
}
