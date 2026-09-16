use soshal_feed_core::publish::validate_note_content;
use soshal_feed_core::query::{
    aggregate_reaction_map, aggregate_reply_map, event_with_tags_from_event, split_feed_buffer,
    timeline_entry_from_event,
};
use soshal_feed_core::ranking::{rank_posts, score_post, AlgoWeights, PostStats};
use soshal_feed_core::reaction::{
    aggregate_message_reactions, AggregateReactionsInput, RawMessageReaction,
};
#[test]
fn test_validate_content() {
    assert!(validate_note_content("hello").is_ok());
    assert!(validate_note_content("").is_err());
    assert!(validate_note_content(&"a".repeat(64001)).is_err());
}

#[test]
fn timeline_entry_shape() {
    let out = timeline_entry_from_event(&soshal_test_util::nostr_event("hi", vec![]));
    assert_eq!(out["id"], "id1");
    assert_eq!(out["created_at"], 100);
    assert!(out.get("tags").is_none());
}

#[test]
fn with_tags_includes_tags() {
    let out = event_with_tags_from_event(&soshal_test_util::nostr_event(
        "hi",
        vec![vec!["t".into(), "x".into()]],
    ));
    assert_eq!(out["tags"][0], serde_json::json!(["t", "x"]));
}

#[test]
fn buffer_split_incremental() {
    let mut buf: std::collections::VecDeque<_> = vec![
        serde_json::json!({"id": "a"}),
        serde_json::json!({"id": "b"}),
        serde_json::json!({"id": "c"}),
    ]
    .into();
    let (pending, total) = split_feed_buffer(&mut buf, "b", 10);
    assert_eq!(total, 3);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["id"], "c");
    assert_eq!(buf.len(), 2);
}

#[test]
fn buffer_split_empty_since() {
    let mut buf: std::collections::VecDeque<_> = vec![serde_json::json!({"id": "a"})].into();
    let (pending, total) = split_feed_buffer(&mut buf, "", 10);
    assert_eq!(pending.len(), 1);
    assert_eq!(total, 1);
}

#[test]
fn buffer_trims_excess() {
    let mut buf: std::collections::VecDeque<serde_json::Value> = (0..10)
        .map(|i| serde_json::json!({"id": i.to_string()}))
        .collect();
    let (pending, _) = split_feed_buffer(&mut buf, "5", 3);
    assert_eq!(pending.len(), 4);
    assert_eq!(buf.len(), 3);
}

#[test]
fn reply_map_counts_once_per_event() {
    let events = vec![
        soshal_test_util::nostr_event("r1", vec![vec!["e".into(), "t1".into()]]),
        soshal_test_util::nostr_event("r2", vec![vec!["e".into(), "t1".into()]]),
        soshal_test_util::nostr_event(
            "r3",
            vec![vec!["e".into(), "t1".into()], vec!["e".into(), "t2".into()]],
        ),
        soshal_test_util::nostr_event("root", vec![]),
        soshal_test_util::nostr_event("x", vec![vec!["p".into(), "t1".into()]]),
    ];
    let map = aggregate_reply_map(&events);
    assert_eq!(map.get("t1"), Some(&3));
    assert_eq!(map.get("t2"), None);
    assert_eq!(map.len(), 1);
}

#[test]
fn reply_map_skips_non_kind1() {
    let events = vec![
        soshal_test_util::nostr_event("reaction", vec![vec!["e".into(), "t1".into()]]),
        soshal_test_util::nostr_event("reply", vec![vec!["e".into(), "t1".into()]]),
    ];
    let mut events = events;
    events[0].kind = 7;
    let map = aggregate_reply_map(&events);
    assert_eq!(map.get("t1"), Some(&1));
}

#[test]
fn reaction_map_aggregates() {
    let events = vec![
        soshal_test_util::nostr_event("👍", vec![vec!["e".into(), "t1".into()]]),
        soshal_test_util::nostr_event("👍", vec![vec!["e".into(), "t1".into()]]),
        soshal_test_util::nostr_event("", vec![vec!["e".into(), "t2".into()]]),
        soshal_test_util::nostr_event("x", vec![vec!["p".into(), "t1".into()]]),
    ];
    let map = aggregate_reaction_map(&events);
    assert_eq!(map["t1"]["count"], 2);
    assert_eq!(map["t1"]["emojis"]["👍"], 2);
    assert_eq!(map["t2"]["count"], 1);
    assert_eq!(map["t2"]["emojis"]["+"], 1);
    assert!(map.get("n/a").is_none());
}

#[test]
fn test_score_new_post() {
    let s = PostStats {
        created_at_secs: 1000.0,
        likes_count: 10,
        replies_count: 5,
        zaps_count: 2,
        reposts_count: 3,
        wot_distance: 0,
    };
    let score = score_post(&s, &[], &[], &AlgoWeights::default(), 1100.0);
    assert!(score > 0.0);
}

#[test]
fn test_hashtag_boost() {
    let s = PostStats {
        created_at_secs: 1000.0,
        likes_count: 0,
        replies_count: 0,
        zaps_count: 0,
        reposts_count: 0,
        wot_distance: 2,
    };
    let user_tags = vec!["rust".to_string()];
    let post_tags = vec!["rust".to_string(), "other".to_string()];
    let score = score_post(&s, &user_tags, &post_tags, &AlgoWeights::default(), 1100.0);
    let no_match = score_post(&s, &[], &post_tags, &AlgoWeights::default(), 1100.0);
    assert!(score > no_match);
}

#[test]
fn test_rank_posts() {
    let stats = vec![
        PostStats {
            created_at_secs: 100.0,
            likes_count: 100,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 0,
        },
        PostStats {
            created_at_secs: 100.0,
            likes_count: 1,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 3,
        },
    ];
    let ranked = rank_posts(&stats, &[], &[], &AlgoWeights::default(), 2, 200.0);
    assert_eq!(ranked[0], 0);
}

#[test]
fn aggregates_reactions_by_emoji() {
    let input = AggregateReactionsInput {
        reactions: vec![
            RawMessageReaction {
                emoji: "👍".into(),
                reactor_pubkey: "alice".into(),
            },
            RawMessageReaction {
                emoji: "👍".into(),
                reactor_pubkey: "bob".into(),
            },
            RawMessageReaction {
                emoji: "❤️".into(),
                reactor_pubkey: "alice".into(),
            },
        ],
        self_pubkey: "alice".into(),
    };
    let result = aggregate_message_reactions(input);
    assert_eq!(result.len(), 2);
    let thumbs = result.iter().find(|r| r.emoji == "👍").unwrap();
    assert_eq!(thumbs.count, 2);
    assert!(thumbs.has_reacted);
    let heart = result.iter().find(|r| r.emoji == "❤️").unwrap();
    assert_eq!(heart.count, 1);
    assert!(heart.has_reacted);
}

#[test]
fn empty_reactions() {
    let input = AggregateReactionsInput {
        reactions: vec![],
        self_pubkey: "me".into(),
    };
    let result = aggregate_message_reactions(input);
    assert!(result.is_empty());
}

#[test]
fn oversized_emoji_skipped() {
    let input = AggregateReactionsInput {
        reactions: vec![
            RawMessageReaction {
                emoji: "x".repeat(65),
                reactor_pubkey: "a".into(),
            },
            RawMessageReaction {
                emoji: "".into(),
                reactor_pubkey: "a".into(),
            },
        ],
        self_pubkey: "a".into(),
    };
    let result = aggregate_message_reactions(input);
    assert!(result.is_empty());
}

#[test]
fn duplicate_reactions_from_same_pubkey_deduplicated() {
    let input = AggregateReactionsInput {
        reactions: vec![
            RawMessageReaction {
                emoji: "🔥".into(),
                reactor_pubkey: "spammer".into(),
            },
            RawMessageReaction {
                emoji: "🔥".into(),
                reactor_pubkey: "spammer".into(),
            },
            RawMessageReaction {
                emoji: "🔥".into(),
                reactor_pubkey: "legit".into(),
            },
        ],
        self_pubkey: "spammer".into(),
    };
    let result = aggregate_message_reactions(input);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].emoji, "🔥");
    assert_eq!(result[0].count, 2);
    assert!(result[0].has_reacted);
}

#[test]
fn test_rank_posts_limits_and_empty_stats() {
    assert_eq!(
        rank_posts(&[], &[], &[], &AlgoWeights::default(), 10, 100.0).len(),
        0
    );
    let stats = vec![
        PostStats {
            created_at_secs: 100.0,
            likes_count: 10,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 0,
        },
        PostStats {
            created_at_secs: 100.0,
            likes_count: 5,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 0,
        },
    ];
    let ranked = rank_posts(&stats, &[], &[], &AlgoWeights::default(), 0, 200.0);
    assert_eq!(ranked.len(), 0);

    let ranked_limited = rank_posts(&stats, &[], &[], &AlgoWeights::default(), 1, 200.0);
    assert_eq!(ranked_limited.len(), 1);
    assert_eq!(ranked_limited[0], 0);
}

#[test]
fn test_hashtag_case_insensitivity() {
    let s = PostStats {
        created_at_secs: 1000.0,
        likes_count: 0,
        replies_count: 0,
        zaps_count: 0,
        reposts_count: 0,
        wot_distance: 1,
    };
    let user_tags = vec!["RUST".to_string()];
    let post_tags = vec!["rust".to_string()];
    let score = score_post(&s, &user_tags, &post_tags, &AlgoWeights::default(), 1100.0);
    let no_match = score_post(&s, &[], &post_tags, &AlgoWeights::default(), 1100.0);
    assert!(score > no_match);
}

#[test]
fn media_tag_extraction() {
    use soshal_feed_core::query::media_json_from_tags;
    let good = media_json_from_tags(
        r#"[["t","x"],["media","video","blob://abab","abababababababababababababababababababababababababababababababab","123"]]"#,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&good).unwrap();
    assert_eq!(v["type"], "video");
    assert_eq!(v["blob_hash"], "ab".repeat(32));
    assert_eq!(v["size"], 123);
    assert!(media_json_from_tags(r#"[]"#).is_none());
    assert!(media_json_from_tags(r#"[["media","video","x","short","1"]]"#).is_none());
    assert!(media_json_from_tags(r#"[["media","pdf","x","y","1"]]"#).is_none());
    assert!(media_json_from_tags("not json").is_none());
}
