const HOUR_SEC: f64 = 3600.0;

#[derive(Debug, Clone, Copy)]
pub struct PostStats {
    pub created_at_secs: f64,
    pub likes_count: u32,
    pub replies_count: u32,
    pub zaps_count: u32,
    pub reposts_count: u32,
    pub wot_distance: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct AlgoWeights {
    pub velocity: f64,
    pub wot: f64,
    pub recency: f64,
    pub hashtag: f64,
}

impl Default for AlgoWeights {
    fn default() -> Self {
        Self {
            velocity: 1.0,
            wot: 1.0,
            recency: 1.0,
            hashtag: 1.0,
        }
    }
}

use rayon::prelude::*;
use std::collections::HashSet;

pub fn score_post_with_set(
    stats: &PostStats,
    user_hashtags_set: &HashSet<String>,
    post_hashtags: &[String],
    weights: &AlgoWeights,
    now_secs: f64,
) -> f64 {
    let hours_ago = (now_secs - stats.created_at_secs).max(0.1) / HOUR_SEC;
    let engagement =
        stats.likes_count + stats.reposts_count + stats.zaps_count + stats.replies_count;
    let velocity = engagement as f64 / hours_ago;
    let wot_boost = (2.0 - stats.wot_distance.min(2) as f64).max(0.5);
    let recency_factor = 1.0 / (hours_ago + 1.0).log2();

    let hashtag_score = if !post_hashtags.is_empty() && !user_hashtags_set.is_empty() {
        let folded_tags: Vec<String> = post_hashtags
            .iter()
            .map(|t| {
                if t.is_ascii() {
                    t.to_ascii_lowercase()
                } else {
                    t.to_lowercase()
                }
            })
            .collect();
        let match_count = folded_tags
            .iter()
            .filter(|t| user_hashtags_set.contains(t.as_str()))
            .count();
        match_count as f64 / folded_tags.len() as f64
    } else {
        0.0
    };

    velocity * weights.velocity
        + wot_boost * weights.wot
        + recency_factor * weights.recency
        + hashtag_score * weights.hashtag
}

pub fn score_post(
    stats: &PostStats,
    user_hashtags: &[String],
    post_hashtags: &[String],
    weights: &AlgoWeights,
    now_secs: f64,
) -> f64 {
    let set: HashSet<String> = user_hashtags.iter().map(|s| s.to_lowercase()).collect();
    score_post_with_set(stats, &set, post_hashtags, weights, now_secs)
}

pub fn rank_posts(
    posts_stats: &[PostStats],
    post_hashtags: &[Vec<String>],
    user_hashtags: &[String],
    weights: &AlgoWeights,
    limit: usize,
    now_secs: f64,
) -> Vec<usize> {
    if posts_stats.is_empty() || limit == 0 {
        return vec![];
    }
    let user_hashtags_set: HashSet<String> = if user_hashtags.is_empty() {
        HashSet::new()
    } else {
        user_hashtags.iter().map(|s| s.to_lowercase()).collect()
    };
    let mut scored: Vec<(usize, f64)> = if posts_stats.len() < 256 {
        posts_stats
            .iter()
            .enumerate()
            .map(|(i, stats)| {
                let tags = post_hashtags.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
                let score = score_post_with_set(stats, &user_hashtags_set, tags, weights, now_secs);
                (i, score)
            })
            .collect()
    } else {
        posts_stats
            .par_iter()
            .enumerate()
            .map(|(i, stats)| {
                let tags = post_hashtags.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
                let score = score_post_with_set(stats, &user_hashtags_set, tags, weights, now_secs);
                (i, score)
            })
            .collect()
    };
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit, |a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }
    scored.into_iter().map(|(i, _)| i).collect()
}
