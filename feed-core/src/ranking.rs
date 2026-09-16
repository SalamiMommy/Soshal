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
    let hours_ago = (now_secs - stats.created_at_secs).max(0.0) / HOUR_SEC;

    let velocity_term = if weights.velocity != 0.0 {
        let engagement = (stats.likes_count as u64)
            + (stats.reposts_count as u64)
            + (stats.zaps_count as u64)
            + (stats.replies_count as u64);
        let effective_hours = hours_ago + 1.0;
        (engagement as f64 / effective_hours) * weights.velocity
    } else {
        0.0
    };

    let wot_term = if weights.wot != 0.0 {
        let wot_boost = (2.0 - stats.wot_distance.min(2) as f64).max(0.5);
        wot_boost * weights.wot
    } else {
        0.0
    };

    let recency_term = if weights.recency != 0.0 {
        let recency_factor = 1.0 / (hours_ago + 2.0).log2();
        recency_factor * weights.recency
    } else {
        0.0
    };

    let hashtag_term =
        if weights.hashtag != 0.0 && !post_hashtags.is_empty() && !user_hashtags_set.is_empty() {
            // ASCII tags match via alloc-free case-insensitive scan of the
            // (lowercased) user set — no per-tag String allocation in the
            // scoring loop. Non-ASCII falls back to a lowercased lookup.
            let match_count = post_hashtags
                .iter()
                .filter(|t| {
                    if t.is_ascii() && t.len() <= 64 {
                        let mut buf = [0u8; 64];
                        let bytes = t.as_bytes();
                        for (i, &b) in bytes.iter().enumerate() {
                            buf[i] = b.to_ascii_lowercase();
                        }
                        if let Ok(s) = std::str::from_utf8(&buf[..bytes.len()]) {
                            user_hashtags_set.contains(s)
                        } else {
                            false
                        }
                    } else {
                        user_hashtags_set.contains(t.to_lowercase().as_str())
                    }
                })
                .count();
            (match_count as f64 / post_hashtags.len() as f64) * weights.hashtag
        } else {
            0.0
        };

    let score = velocity_term + wot_term + recency_term + hashtag_term;
    if score.is_finite() {
        score.max(0.0)
    } else {
        0.0
    }
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
        let mut set = HashSet::with_capacity(user_hashtags.len());
        for s in user_hashtags {
            set.insert(s.to_lowercase());
        }
        set
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
        scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_post_handles_nan_and_inf() {
        let stats = PostStats {
            created_at_secs: f64::NAN,
            likes_count: 10,
            replies_count: 5,
            zaps_count: 2,
            reposts_count: 1,
            wot_distance: 1,
        };
        let weights = AlgoWeights::default();
        let score = score_post(&stats, &[], &[], &weights, 1000.0);
        assert!(score.is_finite());
        assert!(score >= 0.0);
    }

    #[test]
    fn test_score_new_post_bounded() {
        let now = 1_000_000.0;
        let stats = PostStats {
            created_at_secs: now, // 0 seconds old
            likes_count: 1,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 0,
        };
        let weights = AlgoWeights::default();
        let score = score_post(&stats, &[], &[], &weights, now);
        // velocity: 1 / 1.0 = 1.0
        // wot: 2.0
        // recency: 1.0 / log2(2.0) = 1.0
        // total: 4.0
        assert!(
            (score - 4.0).abs() < 1e-6,
            "score should be exactly 4.0, got {score}"
        );
    }
}
