//! Engagement metrics compute kernel.

use crate::{EngagementPostInput, EngagementStatOutput};

pub fn compute_engagement_stats_json(json_input: &str) -> Result<String, String> {
    let input: crate::EngagementInput =
        serde_json::from_str(json_input).map_err(|e| e.to_string())?;
    let out = compute_engagement_stats(&input.posts, &input.self_pubkey);
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

pub fn compute_engagement_stats(
    posts: &[EngagementPostInput],
    self_pubkey: &str,
) -> Vec<EngagementStatOutput> {
    let mut matching: Vec<&EngagementPostInput> =
        posts.iter().filter(|p| p.pubkey == self_pubkey).collect();

    matching.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    matching.truncate(50);

    let mut out = Vec::with_capacity(matching.len());
    for p in matching {
        let text = &p.nostr_event.content;
        let content = if text.len() <= 100 {
            text.to_string()
        } else if let Some((idx, _)) = text.char_indices().nth(100) {
            text[..idx].to_string()
        } else {
            text.to_string()
        };
        out.push(EngagementStatOutput {
            post_id: p.id.clone(),
            content,
            created_at: p.created_at,
            reaction_count: p
                .local_stats
                .as_ref()
                .and_then(|s| s.likes_count)
                .unwrap_or(0),
            repost_count: p
                .local_stats
                .as_ref()
                .and_then(|s| s.reposts_count)
                .unwrap_or(0),
        });
    }
    out
}
