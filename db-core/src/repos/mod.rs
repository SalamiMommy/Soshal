pub mod audit_log;
pub mod banned_member;
pub mod block;
pub mod bookmark;
pub mod conversation_mute;
pub mod dating_unmatch;
pub mod diagnostic_log;
pub mod ephemeral_media;
pub mod escrow;
pub mod friend_backup;
pub mod geohash_peer;
pub mod group;
pub mod group_invite;
pub mod group_join_request;
pub mod guestbook;
pub mod hashtag;
pub mod huddle_post;
pub mod ignored_notification;
pub mod limits;
pub mod link_preview;
pub mod marketplace_review;
pub mod media;
pub mod message;
pub mod musicloud;
pub mod notification;
pub mod poll;
pub mod post;
pub mod post_views;
pub mod profile_node;
pub mod reaction;
pub mod refetch_item;
pub mod relay;
pub mod reminder;
pub mod repost;
pub mod role;
pub mod room;
pub mod saved;
pub mod search_index;
pub mod settings;
pub mod spam_report;
pub mod story_reaction;
pub mod stream_chat;
pub mod thread;
pub mod user;
pub mod voice;
pub mod zap;

/// Maximum rows any paged query may return.
pub const MAX_PAGE_LIMIT: i64 = 2000;

/// Clamps a webview-supplied limit to `1..=MAX_PAGE_LIMIT`. A negative limit
/// would map to SQLite `LIMIT -1` (no bound at all) and return the whole table.
pub fn clamp_limit(limit: i64) -> i64 {
    limit.clamp(1, MAX_PAGE_LIMIT)
}

/// Clamps a webview-supplied page to sane bounds. Besides the unbounded-limit
/// case above, a negative offset would skip backwards into the result set.
pub fn clamp_page(limit: i64, offset: i64) -> (i64, i64) {
    (clamp_limit(limit), offset.max(0))
}
