use soshal_feed_core::ranking::{
    rank_posts, score_post, score_post_with_set, AlgoWeights, PostStats,
};
use std::collections::HashSet;

fn stats(created_secs: f64) -> PostStats {
    PostStats {
        created_at_secs: created_secs,
        likes_count: 10,
        replies_count: 2,
        zaps_count: 1,
        reposts_count: 3,
        wot_distance: 1,
    }
}

fn weights() -> AlgoWeights {
    AlgoWeights {
        velocity: 1.0,
        wot: 1.0,
        recency: 1.0,
        hashtag: 1.0,
    }
}

#[test]
fn score_post_with_set_engagement_dominates() {
    let active = stats(1_700_000_000.0);
    let stale = PostStats {
        created_at_secs: 1_700_000_000.0 - 48.0 * 3600.0,
        ..active
    };
    let set = HashSet::new();
    let now = 1_700_000_100.0;
    let fresh = score_post_with_set(&active, &set, &[], &weights(), now);
    let old = score_post_with_set(&stale, &set, &[], &weights(), now);
    assert!(fresh > old, "fresher post scores higher: {fresh} vs {old}");
}

#[test]
fn score_post_with_set_hashtag_match_boosts() {
    let s = stats(1_700_000_000.0);
    let mut set = HashSet::new();
    set.insert("nostr".to_string());
    set.insert("rust".to_string());
    let now = 1_700_000_100.0;
    let matched = score_post_with_set(&s, &set, &["NOSTR".into(), "music".into()], &weights(), now);
    let unmatched =
        score_post_with_set(&s, &set, &["food".into(), "music".into()], &weights(), now);
    assert!(
        matched > unmatched,
        "hashtag overlap boosts: {matched} vs {unmatched}"
    );
    let none = score_post_with_set(&s, &set, &[], &weights(), now);
    let empty_set = score_post_with_set(&s, &HashSet::new(), &["nostr".into()], &weights(), now);
    assert_eq!(
        none, empty_set,
        "empty either side neutralizes hashtag score"
    );
}

#[test]
fn score_post_with_set_wot_boost_and_zero_edge() {
    let s = stats(1_700_000_000.0);
    let now = 1_700_000_100.0;
    let set = HashSet::new();
    let close = score_post_with_set(&s, &set, &[], &weights(), now);
    let far = score_post_with_set(
        &PostStats {
            wot_distance: 2,
            ..s
        },
        &set,
        &[],
        &weights(),
        now,
    );
    assert!(close > far, "distance-1 post outranks distance-2");
    let fresh_future = score_post_with_set(
        &PostStats {
            created_at_secs: now + 100.0,
            ..s
        },
        &set,
        &[],
        &weights(),
        now,
    );
    assert!(fresh_future.is_finite());
}

#[test]
fn score_post_with_set_edges() {
    let s = stats(1_700_000_000.0);
    let now = 1_700_000_100.0;
    let set = HashSet::new();
    // Zero-engagement post still scores (velocity 0, recency/wot live).
    let zero = PostStats {
        likes_count: 0,
        replies_count: 0,
        zaps_count: 0,
        reposts_count: 0,
        ..s
    };
    let z = score_post_with_set(&zero, &set, &[], &weights(), now);
    assert!(z.is_finite() && z > 0.0, "zero engagement: {z}");
    // Wot distance clamps at 2 → floor boost.
    let far = score_post_with_set(
        &PostStats {
            wot_distance: 9,
            ..s
        },
        &set,
        &[],
        &weights(),
        now,
    );
    assert_eq!(
        far,
        score_post_with_set(
            &PostStats {
                wot_distance: 2,
                ..s
            },
            &set,
            &[],
            &weights(),
            now
        ),
        "distance > 2 clamps"
    );
    // Very old post: hours_ago large, recency decays.
    let ancient = score_post_with_set(
        &PostStats {
            created_at_secs: now - 30.0 * 24.0 * 3600.0,
            ..s
        },
        &set,
        &[],
        &weights(),
        now,
    );
    assert!(ancient < z, "ancient scores below fresh zero-engagement");
    // Zeroed weights → score collapses to zero.
    let zeroed = AlgoWeights {
        velocity: 0.0,
        wot: 0.0,
        recency: 0.0,
        hashtag: 0.0,
    };
    assert_eq!(
        score_post_with_set(&s, &set, &[], &zeroed, now),
        0.0,
        "all-zero weights"
    );
}

#[test]
fn rank_posts_orders_and_truncates() {
    let now = 1_700_000_100.0;
    let s = stats(now - 100.0);
    let set = vec!["rust".to_string()];
    let stats_vec = vec![
        s, // index 0: fresh, high engagement
        PostStats {
            created_at_secs: now - 50.0 * 3600.0,
            ..s
        }, // index 1: stale
        PostStats {
            created_at_secs: now - 60.0,
            ..s
        }, // index 2: freshest
    ];
    let tags = vec![vec![], vec![], vec!["rust".to_string()]];
    let ranked = rank_posts(&stats_vec, &tags, &set, &weights(), 2, now);
    assert_eq!(ranked.len(), 2);
    // Freshest with hashtag match must lead; no index out of range.
    assert_eq!(ranked[0], 2, "ranked: {ranked:?}");
    // Missing tags slice entry is tolerated (None → empty tags).
    let ranked = rank_posts(&stats_vec, &[], &set, &weights(), 10, now);
    assert_eq!(ranked.len(), 3, "ranked: {ranked:?}");
    // Empty input or zero limit → empty result.
    assert!(rank_posts(&[], &[], &set, &weights(), 5, now).is_empty());
    assert!(rank_posts(&stats_vec, &tags, &set, &weights(), 0, now).is_empty());
}

#[test]
fn score_post_case_folding_matches() {
    let s = stats(1_700_000_000.0);
    let now = 1_700_000_100.0;
    let weights = weights();
    // ASCII user tag folds; non-ASCII tag uses its literal form.
    let ascii = score_post(
        &s,
        &["NOSTR".to_string()],
        &["nostr".to_string()],
        &weights,
        now,
    );
    let unicode = score_post(&s, &["🎉".to_string()], &["🎉".to_string()], &weights, now);
    let none = score_post(&s, &[], &["nostr".to_string()], &weights, now);
    assert!(ascii > none, "{ascii} vs {none}");
    assert!(unicode > none, "{unicode} vs {none}");
}
