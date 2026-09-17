//! Integration tests for soshal-search-core.

use soshal_search_core::event_map::{
    event_to_search_result, event_to_search_result_json, EventToSearchResultInput,
};
use soshal_search_core::fts5::{
    format_fts5_query, format_fts5_query_json, optimize_fts5_index_query, sanitize_fts5_term,
};
use soshal_search_core::row_map::{map_search_row, map_search_row_json, SearchRowInput};
use soshal_search_core::vector_search::{cosine_similarity, rank_vector_documents, VectorDocument};
use soshal_search_core::{MAX_CONTENT_LEN, MAX_FTS5_TERMS};

#[test]
fn sanitize_fts5_term_strips_punctuation() {
    assert_eq!(sanitize_fts5_term("hello").as_deref(), Some("hello"));
    assert_eq!(
        sanitize_fts5_term(" rust-lang! ").as_deref(),
        Some("rustlang")
    );
    assert_eq!(sanitize_fts5_term("café").as_deref(), Some("café"));
    assert!(sanitize_fts5_term("!@#$%").is_none());
    assert!(sanitize_fts5_term("").is_none());
}

#[test]
fn sanitize_fts5_term_rejects_oversize() {
    let long = "a".repeat(65);
    assert!(sanitize_fts5_term(&long).is_none());
    let ok = "a".repeat(64);
    assert!(sanitize_fts5_term(&ok).is_some());
}

#[test]
fn format_fts5_query_builds_prefix_query() {
    assert_eq!(
        format_fts5_query("hello world"),
        "\"hello\"* AND \"world\"*"
    );
    assert_eq!(
        format_fts5_query("  hello   world  "),
        "\"hello\"* AND \"world\"*"
    );
    assert_eq!(
        format_fts5_query("subject:release c"),
        "subject:\"release\"* AND \"c\"*"
    );
    assert_eq!(
        format_fts5_query("https://example.com"),
        "\"httpsexamplecom\"*"
    );
}

#[test]
fn format_fts5_query_empty_or_oversize_is_empty() {
    assert_eq!(format_fts5_query(""), "");
    assert_eq!(format_fts5_query("   "), "");
    assert_eq!(format_fts5_query("!!!"), "");
    assert_eq!(format_fts5_query(&"a".repeat(4097)), "");
    assert_eq!(format_fts5_query("content:world:"), "content:\"world\"*");
}

#[test]
fn format_fts5_query_deduplicates_and_caps_terms() {
    assert_eq!(format_fts5_query("a a b"), "\"a\"* AND \"b\"*");
    let many = (1..=40)
        .map(|i| format!("w{}", i))
        .collect::<Vec<_>>()
        .join(" ");
    let out = format_fts5_query(&many);
    assert_eq!(out.matches("AND").count(), MAX_FTS5_TERMS - 1);
}

#[test]
fn format_fts5_query_json_validates_input() {
    assert_eq!(
        format_fts5_query_json(r#"{"query":"rust nostr"}"#),
        "\"rust\"* AND \"nostr\"*"
    );
    assert_eq!(format_fts5_query_json("garbage"), "");
    assert_eq!(
        format_fts5_query_json(&format!(r#"{{"query":"{}"}}"#, "a".repeat(4097))),
        ""
    );
}

#[test]
fn optimize_fts5_index_query_returns_sql() {
    assert_eq!(
        optimize_fts5_index_query(),
        "INSERT INTO posts_fts(posts_fts) VALUES('optimize');"
    );
}

#[test]
fn map_search_row_user_parses_metadata() {
    let row = SearchRowInput {
        id: "u1".into(),
        row_type: "user".into(),
        title: "Alice".into(),
        content: r#"{"picture":"https://x/a.png","about":"builder"}"#.into(),
        pubkey: "pk1".into(),
        created_at: 99.0,
    };
    let out = map_search_row(&row);
    assert_eq!(out.result_type, "user");
    assert_eq!(out.title, "Alice");
    assert_eq!(out.subtitle, "builder");
    assert_eq!(out.image_url.as_deref(), Some("https://x/a.png"));
    assert_eq!(out.created_at, 99.0);
}

#[test]
fn map_search_row_user_without_picture() {
    let row = SearchRowInput {
        id: "u2".into(),
        row_type: "user".into(),
        title: "Bob".into(),
        content: r#"{"name":"Bob"}"#.into(),
        pubkey: "pk2".into(),
        created_at: 1.0,
    };
    let out = map_search_row(&row);
    assert!(out.image_url.is_none());
    assert_eq!(out.subtitle, "");
}

#[test]
fn map_search_row_post_uses_content_as_subtitle() {
    let row = SearchRowInput {
        id: "p1".into(),
        row_type: "post".into(),
        title: "Post title".into(),
        content: "post body".into(),
        pubkey: "pk3".into(),
        created_at: 2.0,
    };
    let out = map_search_row(&row);
    assert_eq!(out.subtitle, "post body");
    assert!(out.image_url.is_none());
}

#[test]
fn map_search_row_json_roundtrip_camel_case_out() {
    let input = serde_json::json!({
        "id": "p1",
        "type": "post",
        "title": "T",
        "content": "body",
        "pubkey": "pk",
        "created_at": 5.0
    });
    let out = map_search_row_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["type"], "post");
    assert_eq!(v["id"], "p1");
    assert_eq!(v["createdAt"], 5.0);
}

#[test]
fn map_search_row_json_rejects_invalid_or_oversize() {
    assert_eq!(map_search_row_json("garbage"), "null");
    let big = "x".repeat(MAX_CONTENT_LEN + 1);
    let input = serde_json::json!({"id": "p1", "type": "post", "title": "T", "content": big, "pubkey": "pk", "created_at": 1.0});
    assert_eq!(map_search_row_json(&input.to_string()), "null");
}

#[test]
fn event_to_search_result_maps_user_kind() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(
            0,
            r#"{"display_name":"Alice","about":"hello","picture":"https://x/a"}"#,
            vec![],
        ),
        kind: 0,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.result_type, "user");
    assert_eq!(out.title, "Alice");
    assert_eq!(out.subtitle, "hello");
    assert_eq!(out.image_url.as_deref(), Some("https://x/a"));
}

#[test]
fn event_to_search_result_user_falls_back_to_pubkey_prefix() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(0, "not-json", vec![]),
        kind: 0,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.title, "pk1");
}

#[test]
fn event_to_search_result_maps_post_kind() {
    let content = "post content here";
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(1, content, vec![]),
        kind: 1,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.result_type, "post");
    assert_eq!(out.title, content);
    assert_eq!(out.subtitle, "pk1...");
}

#[test]
fn event_to_search_result_post_truncates_title() {
    let content = "x".repeat(100);
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(1, &content, vec![]),
        kind: 1,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.title.len(), 80);
}

#[test]
fn event_to_search_result_maps_event_kind_with_title_tag() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(
            31923,
            "some event content",
            vec![
                vec!["d".into(), "my-event".into()],
                vec!["title".into(), "Event Title".into()],
            ],
        ),
        kind: 31923,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.result_type, "event");
    assert_eq!(out.title, "Event Title");
}

#[test]
fn event_to_search_result_event_kind_falls_back_to_d_tag() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(
            31923,
            "content",
            vec![vec!["d".into(), "fallback-d".into()]],
        ),
        kind: 31923,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.title, "fallback-d");
}

#[test]
fn event_to_search_result_maps_listing_kind() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(
            30402,
            "listing body",
            vec![
                vec!["title".into(), "Sofa".into()],
                vec!["price".into(), "100".into()],
                vec!["image".into(), "https://x/sofa.png".into()],
            ],
        ),
        kind: 30402,
    };
    let out = event_to_search_result(&input).unwrap();
    assert_eq!(out.result_type, "listing");
    assert_eq!(out.title, "Sofa");
    assert_eq!(out.subtitle, "100");
    assert_eq!(out.image_url.as_deref(), Some("https://x/sofa.png"));
}

#[test]
fn event_to_search_result_unsupported_kind_returns_none() {
    let input = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(7, "content", vec![]),
        kind: 7,
    };
    assert!(event_to_search_result(&input).is_none());
}

#[test]
fn event_to_search_result_json_roundtrip() {
    let input = serde_json::json!({
        "event": {"id": "e1", "pubkey": "pk1", "content": "hi", "tags": [], "created_at": 1.0, "kind": 1},
        "kind": 1
    });
    let out = event_to_search_result_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["type"], "post");
    assert_eq!(v["id"], "e1");
    assert_eq!(v["createdAt"], 1.0);
}

#[test]
fn event_to_search_result_json_rejects_invalid_input() {
    assert_eq!(event_to_search_result_json("garbage"), "null");
    let unsupported = serde_json::json!({
        "event": {"id": "e1", "pubkey": "pk1", "content": "hi", "tags": [], "created_at": 1.0, "kind": 1},
        "kind": 999
    });
    assert_eq!(
        event_to_search_result_json(&unsupported.to_string()),
        "null"
    );
}

#[test]
fn event_to_search_result_display_name_and_listing_defaults() {
    // User metadata with displayName / name fallbacks
    let input1 = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(0, r#"{"displayName":"Bob"}"#, vec![]),
        kind: 0,
    };
    assert_eq!(event_to_search_result(&input1).unwrap().title, "Bob");

    let input2 = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(0, r#"{"name":"Charlie"}"#, vec![]),
        kind: 0,
    };
    assert_eq!(event_to_search_result(&input2).unwrap().title, "Charlie");

    // Listing default title
    let input3 = EventToSearchResultInput {
        event: soshal_test_util::nostr_event_kind(30402, "listing body", vec![]),
        kind: 30402,
    };
    assert_eq!(
        event_to_search_result(&input3).unwrap().title,
        "Marketplace Listing"
    );
}

#[test]
fn event_to_search_result_json_limits() {
    let big_content = "a".repeat(MAX_CONTENT_LEN + 1);
    let input = serde_json::json!({
        "event": {"id": "e1", "pubkey": "pk1", "content": big_content, "tags": [], "created_at": 1.0, "kind": 1},
        "kind": 1
    });
    assert_eq!(event_to_search_result_json(&input.to_string()), "null");
}

#[test]
fn format_fts5_query_handles_unicode_and_term_variants() {
    assert_eq!(
        format_fts5_query("café au lait"),
        "\"café\"* AND \"au\"* AND \"lait\"*"
    );
    assert_eq!(
        format_fts5_query("\"quoted\" term"),
        "\"quoted\"* AND \"term\"*"
    );
    assert_eq!(
        format_fts5_query("rust-lang 2x!"),
        "\"rustlang\"* AND \"2x\"*"
    );
    assert_eq!(
        format_fts5_query("#nostr @alice"),
        "\"nostr\"* AND \"alice\"*"
    );
}

#[test]
fn format_fts5_query_no_match_terms_yield_empty() {
    assert_eq!(format_fts5_query("--- ..."), "");
    assert_eq!(format_fts5_query("'single'"), "\"single\"*");
    assert_eq!(format_fts5_query(",,,"), "");
}

#[test]
fn format_fts5_query_reserved_words_are_quoted() {
    // FTS5 reserved words (AND, OR, NOT, NEAR) must be quoted so they are
    // treated as literal phrase terms instead of parsed as operators.
    assert_eq!(format_fts5_query("AND"), "\"AND\"*");
    assert_eq!(format_fts5_query("OR NOT"), "\"OR\"* AND \"NOT\"*");
    assert_eq!(
        format_fts5_query("hello AND world"),
        "\"hello\"* AND \"AND\"* AND \"world\"*"
    );
    assert_eq!(format_fts5_query("NEAR test"), "\"NEAR\"* AND \"test\"*");
}

#[test]
fn sanitize_fts5_term_uses_char_count_for_length() {
    // 33 CJK chars = 99 bytes in UTF-8; should pass at char-count 33
    let cjk33 = "漢".repeat(33);
    assert!(sanitize_fts5_term(&cjk33).is_some());
    // 65 CJK chars = 195 bytes; should fail at char-count 65
    let cjk65 = "漢".repeat(65);
    assert!(sanitize_fts5_term(&cjk65).is_none());
    // 64 CJK chars = 192 bytes; should pass
    let cjk64 = "漢".repeat(64);
    assert!(sanitize_fts5_term(&cjk64).is_some());
}

#[test]
fn cosine_similarity_mismatched_or_empty_returns_zero() {
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    assert_eq!(cosine_similarity(&[], &[]), 0.0);
}

#[test]
fn rank_vector_documents_orders_by_similarity() {
    let docs = vec![
        VectorDocument {
            id: "far".into(),
            embedding: vec![0.0, 1.0],
            norm: 0.0,
        },
        VectorDocument {
            id: "near".into(),
            embedding: vec![0.9, 0.1],
            norm: 0.0,
        },
        VectorDocument {
            id: "mid".into(),
            embedding: vec![0.5, 0.5],
            norm: 0.0,
        },
    ];
    let ranked = rank_vector_documents(&[1.0, 0.0], &docs, 3);
    let ids: Vec<&str> = ranked.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["near", "mid", "far"]);
    assert!(ranked[0].1 > ranked[1].1 && ranked[1].1 > ranked[2].1);
}

#[test]
fn rank_vector_documents_respects_top_k() {
    let docs = vec![
        VectorDocument {
            id: "a".into(),
            embedding: vec![1.0, 0.0],
            norm: 0.0,
        },
        VectorDocument {
            id: "b".into(),
            embedding: vec![0.5, 0.5],
            norm: 0.0,
        },
        VectorDocument {
            id: "c".into(),
            embedding: vec![0.0, 1.0],
            norm: 0.0,
        },
    ];
    let ranked = rank_vector_documents(&[1.0, 0.0], &docs, 2);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].0, "a");
}

#[test]
fn rank_vector_documents_empty_index_returns_empty() {
    let ranked = rank_vector_documents(&[1.0, 0.0], &[], 5);
    assert!(ranked.is_empty());
}

#[test]
fn rank_vector_documents_dimension_mismatch_scores_zero() {
    let docs = vec![
        VectorDocument {
            id: "d1".into(),
            embedding: vec![1.0, 0.0],
            norm: 0.0,
        },
        VectorDocument {
            id: "d2".into(),
            embedding: vec![0.0, 1.0, 0.0],
            norm: 0.0,
        },
    ];
    let ranked = rank_vector_documents(&[1.0, 0.0], &docs, 5);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0], ("d1".to_string(), 1.0));
    assert_eq!(ranked[1], ("d2".to_string(), 0.0));
}
