//! Post statistics compute kernel.

use crate::PostInput;
use serde::Serialize;

#[derive(Serialize)]
pub struct AnalyticsOutput {
    #[serde(rename = "totalPosts")]
    pub total_posts: usize,
    #[serde(rename = "totalReactions")]
    pub total_reactions: u64,
}

pub fn compute_analytics_posts_json(json_input: &str) -> Result<String, String> {
    let input: crate::AnalyticsInput =
        serde_json::from_str(json_input).map_err(|e| e.to_string())?;
    let out = compute_analytics_posts(&input.posts, &input.self_pubkey);
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

pub fn compute_analytics_posts(posts: &[PostInput], self_pubkey: &str) -> AnalyticsOutput {
    let total_posts = posts.iter().filter(|p| p.pubkey == self_pubkey).count();
    let total_reactions = posts
        .iter()
        .filter(|p| p.pubkey == self_pubkey)
        .filter_map(|p| p.local_stats.as_ref())
        .filter_map(|s| s.likes_count)
        .sum();
    AnalyticsOutput {
        total_posts,
        total_reactions,
    }
}
