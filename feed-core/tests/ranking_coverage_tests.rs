use soshal_feed_core::ranking::{score_post_with_set, AlgoWeights, PostStats};
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
