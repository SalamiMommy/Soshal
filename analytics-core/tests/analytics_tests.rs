use soshal_analytics_core::engagement::{compute_engagement_stats, compute_engagement_stats_json};
use soshal_analytics_core::format_count::{format_count, format_count_json};
use soshal_analytics_core::posts::{compute_analytics_posts, compute_analytics_posts_json};
use soshal_analytics_core::{EngagementPostInput, LocalStats, NostrEvent, PostInput};

fn make_post(
    id: &str,
    pubkey: &str,
    content: &str,
    created_at: u64,
    likes: u64,
    reposts: u64,
) -> EngagementPostInput {
    EngagementPostInput {
        id: id.into(),
        pubkey: pubkey.into(),
        nostr_event: NostrEvent {
            id: String::new(),
            content: content.into(),
            tags: vec![],
            created_at: 0.0,
            pubkey: String::new(),
            kind: 0,
        },
        created_at,
        local_stats: Some(LocalStats {
            likes_count: Some(likes),
            reposts_count: Some(reposts),
        }),
    }
}

#[test]
fn compute_engagement_stats_limits_to_50() {
    let posts: Vec<EngagementPostInput> = (0..60)
        .map(|i| make_post(&format!("id_{}", i), "alice", "hello", i as u64, 1, 2))
        .collect();
    let out = compute_engagement_stats(&posts, "alice");
    assert_eq!(out.len(), 50);
    assert_eq!(out[0].post_id, "id_59");
    assert_eq!(out[49].post_id, "id_10");
}

#[test]
fn compute_engagement_stats_truncates_content() {
    let posts = vec![make_post("id_1", "alice", &"x".repeat(200), 1000, 5, 3)];
    let out = compute_engagement_stats(&posts, "alice");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].content.len(), 100);
    assert_eq!(out[0].reaction_count, 5);
    assert_eq!(out[0].repost_count, 3);
}

#[test]
fn test_compute_engagement_stats_json() {
    let json_str = r#"{
        "posts": [
            {
                "id": "post_1",
                "pubkey": "alice",
                "nostrEvent": {"id": "e1", "content": "Test content", "tags": [], "createdAt": 0, "pubkey": "alice", "kind": 1},
                "createdAt": 100,
                "localStats": {"likesCount": 10, "repostsCount": 2}
            }
        ],
        "selfPubkey": "alice"
    }"#;
    let res = compute_engagement_stats_json(json_str).unwrap();
    assert!(res.contains("post_1"));
    assert!(res.contains("reactionCount\":10"));
}

#[test]
fn format_count_millions() {
    assert_eq!(format_count(1_500_000.0), "1.5M");
    assert_eq!(format_count(2_000_000.0), "2.0M");
}

#[test]
fn format_count_thousands() {
    assert_eq!(format_count(1_500.0), "1.5K");
    assert_eq!(format_count(10_000.0), "10.0K");
}

#[test]
fn format_count_small() {
    assert_eq!(format_count(999.0), "999");
    assert_eq!(format_count(0.0), "0");
}

#[test]
fn format_count_negative_and_nan() {
    // Negative values clamp to 0 ("no data"), NaN also maps to "0".
    assert_eq!(format_count(-1.0), "0");
    assert_eq!(format_count(-999.0), "0");
    assert_eq!(format_count(f64::NAN), "0");
    assert_eq!(format_count(f64::NEG_INFINITY), "0");
}

#[test]
fn format_count_positive_infinity() {
    // +Inf is non-finite — clamp to 0 like NaN/negative.
    assert_eq!(format_count(f64::INFINITY), "0");
}

#[test]
fn test_format_count_json() {
    let json_in = r#"{"n": 2500.0}"#;
    let res = format_count_json(json_in).unwrap();
    assert_eq!(res, "2.5K");
}

#[test]
fn compute_analytics_posts_filters_by_pubkey() {
    let posts = vec![
        PostInput {
            pubkey: "alice".into(),
            local_stats: Some(LocalStats {
                likes_count: Some(5),
                reposts_count: Some(2),
            }),
        },
        PostInput {
            pubkey: "bob".into(),
            local_stats: Some(LocalStats {
                likes_count: Some(3),
                reposts_count: Some(1),
            }),
        },
        PostInput {
            pubkey: "alice".into(),
            local_stats: None,
        },
    ];
    let out = compute_analytics_posts(&posts, "alice");
    assert_eq!(out.total_posts, 2);
    assert_eq!(out.total_reactions, 5);
}

#[test]
fn compute_analytics_posts_no_matching_pubkey() {
    let posts = vec![PostInput {
        pubkey: "bob".into(),
        local_stats: Some(LocalStats {
            likes_count: Some(3),
            reposts_count: Some(1),
        }),
    }];
    let out = compute_analytics_posts(&posts, "alice");
    assert_eq!(out.total_posts, 0);
    assert_eq!(out.total_reactions, 0);
}

#[test]
fn test_compute_analytics_posts_json() {
    let json_in = r#"{
        "posts": [
            {"pubkey": "alice", "localStats": {"likesCount": 8, "repostsCount": 1}}
        ],
        "selfPubkey": "alice"
    }"#;
    let res = compute_analytics_posts_json(json_in).unwrap();
    assert!(res.contains("totalPosts\":1"));
    assert!(res.contains("totalReactions\":8"));
}

#[test]
fn test_analytics_json_error_handling() {
    assert!(format_count_json("invalid json").is_err());
    assert!(compute_engagement_stats_json("invalid json").is_err());
    assert!(compute_analytics_posts_json("invalid json").is_err());

    let huge = "x".repeat(16 * 1024 * 1024 + 1);
    assert!(compute_engagement_stats_json(&huge).is_err());
    assert!(compute_analytics_posts_json(&huge).is_err());
}

#[test]
fn test_analytics_case_insensitivity_and_saturation() {
    let posts = vec![
        PostInput {
            pubkey: "ALICE".into(),
            local_stats: Some(LocalStats {
                likes_count: Some(u64::MAX - 10),
                reposts_count: None,
            }),
        },
        PostInput {
            pubkey: "alice".into(),
            local_stats: Some(LocalStats {
                likes_count: Some(50),
                reposts_count: None,
            }),
        },
    ];
    let out = compute_analytics_posts(&posts, "Alice");
    assert_eq!(out.total_posts, 2);
    assert_eq!(out.total_reactions, u64::MAX);

    let eng_posts = vec![make_post("p1", "ALICE", "msg", 100, 5, 2)];
    let eng_out = compute_engagement_stats(&eng_posts, "alice");
    assert_eq!(eng_out.len(), 1);
    assert_eq!(eng_out[0].post_id, "p1");
}
