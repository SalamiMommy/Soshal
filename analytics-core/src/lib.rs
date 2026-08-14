//! Analytics compute kernels: post statistics, engagement metrics, and
//! human-readable count formatting.

pub mod engagement;
pub mod format_count;
pub mod posts;
pub mod slm;

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PostInput {
    pub pubkey: String,
    #[serde(rename = "localStats")]
    pub local_stats: Option<LocalStats>,
}

#[derive(Deserialize)]
pub struct LocalStats {
    #[serde(rename = "likesCount")]
    pub likes_count: Option<u64>,
    #[serde(rename = "repostsCount")]
    pub reposts_count: Option<u64>,
}

#[derive(Deserialize)]
pub struct EngagementPostInput {
    pub id: String,
    pub pubkey: String,
    #[serde(rename = "nostrEvent")]
    pub nostr_event: NostrEvent,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(rename = "localStats")]
    pub local_stats: Option<LocalStats>,
}

pub use soshal_nostr_core::NostrEvent;

#[derive(Deserialize)]
pub struct AnalyticsInput {
    pub posts: Vec<PostInput>,
    #[serde(rename = "selfPubkey")]
    pub self_pubkey: String,
}

#[derive(Deserialize)]
pub struct EngagementInput {
    pub posts: Vec<EngagementPostInput>,
    #[serde(rename = "selfPubkey")]
    pub self_pubkey: String,
}

#[derive(Deserialize)]
pub struct FormatCountInput {
    pub n: f64,
}

#[derive(Serialize)]
pub struct EngagementStatOutput {
    #[serde(rename = "postId")]
    pub post_id: String,
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(rename = "reactionCount")]
    pub reaction_count: u64,
    #[serde(rename = "repostCount")]
    pub repost_count: u64,
}
