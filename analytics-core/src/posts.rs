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
    if json_input.len() > 16 * 1024 * 1024 {
        return Err("json input exceeds 16MB cap".into());
    }
    let input: crate::AnalyticsInput =
        serde_json::from_str(json_input).map_err(|e| e.to_string())?;
    let out = compute_analytics_posts(&input.posts, &input.self_pubkey);
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

pub fn compute_analytics_posts(posts: &[PostInput], self_pubkey: &str) -> AnalyticsOutput {
    let mut total_posts = 0;
    let mut total_reactions = 0u64;
    for p in posts {
        if p.pubkey.eq_ignore_ascii_case(self_pubkey) {
            total_posts += 1;
            if let Some(ref stats) = p.local_stats {
                if let Some(likes) = stats.likes_count {
                    total_reactions = total_reactions.saturating_add(likes);
                }
            }
        }
    }
    AnalyticsOutput {
        total_posts,
        total_reactions,
    }
}
