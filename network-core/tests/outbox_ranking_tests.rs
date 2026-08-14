//! Outbox relay ranking tests

use soshal_network_core::outbox_ranking::{rank_outbox_relays, RelayRank};

#[test]
fn rank_outbox_relays_basic() {
    let input = vec![
        (
            "alice".to_string(),
            vec!["wss://relay1.com".into(), "wss://relay2.com".into()],
        ),
        ("bob".to_string(), vec!["wss://relay1.com".into()]),
    ];
    let ranked = rank_outbox_relays(&input, 10);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].relay_url, "wss://relay1.com");
    assert_eq!(ranked[0].author_count, 2);
    assert_eq!(ranked[0].covered_pubkeys.len(), 2);
    assert!(ranked[0].covered_pubkeys.contains(&"alice".to_string()));
    assert!(ranked[0].covered_pubkeys.contains(&"bob".to_string()));
}

#[test]
fn rank_outbox_relays_empty_and_limit() {
    assert_eq!(rank_outbox_relays(&[], 5).len(), 0);
    let input = vec![("alice".to_string(), vec!["wss://relay1.com".into()])];
    assert_eq!(rank_outbox_relays(&input, 0).len(), 0);
}

#[test]
fn rank_outbox_relays_normalizes_urls() {
    let input = vec![
        ("alice".to_string(), vec!["WSS://RELAY1.COM".into()]),
        ("bob".to_string(), vec!["wss://relay1.com".into()]),
    ];
    let ranked = rank_outbox_relays(&input, 10);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].relay_url, "wss://relay1.com");
    assert_eq!(ranked[0].author_count, 2);
}

#[test]
fn rank_outbox_relays_trims_whitespace() {
    let input = vec![
        ("alice".to_string(), vec![" wss://relay1.com ".into()]),
        ("bob".to_string(), vec!["wss://relay1.com".into()]),
    ];
    let ranked = rank_outbox_relays(&input, 10);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].author_count, 2);
}

#[test]
fn rank_outbox_relays_respects_limit() {
    let input = vec![
        ("alice".to_string(), vec!["wss://relay1.com".into()]),
        ("bob".to_string(), vec!["wss://relay2.com".into()]),
        ("charlie".to_string(), vec!["wss://relay3.com".into()]),
    ];
    let ranked = rank_outbox_relays(&input, 2);
    assert_eq!(ranked.len(), 2);
}

#[test]
fn rank_outbox_relays_sorts_by_author_count() {
    let input = vec![
        ("alice".to_string(), vec!["wss://relay1.com".into()]),
        (
            "bob".to_string(),
            vec!["wss://relay1.com".into(), "wss://relay2.com".into()],
        ),
        (
            "charlie".to_string(),
            vec![
                "wss://relay1.com".into(),
                "wss://relay2.com".into(),
                "wss://relay3.com".into(),
            ],
        ),
    ];
    let ranked = rank_outbox_relays(&input, 10);
    assert_eq!(ranked.len(), 3);
    assert_eq!(ranked[0].relay_url, "wss://relay1.com");
    assert_eq!(ranked[0].author_count, 3);
}

#[test]
fn rank_outbox_relays_handles_empty_relay_lists() {
    let input = vec![
        ("alice".to_string(), vec![]),
        ("bob".to_string(), vec!["wss://relay1.com".into()]),
    ];
    let ranked = rank_outbox_relays(&input, 10);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].relay_url, "wss://relay1.com");
    assert_eq!(ranked[0].author_count, 1);
}

#[test]
fn relay_rank_clone() {
    let rank = RelayRank {
        relay_url: "wss://relay.com".to_string(),
        author_count: 5,
        covered_pubkeys: vec!["alice".to_string(), "bob".to_string()],
    };
    let cloned = rank.clone();
    assert_eq!(rank.relay_url, cloned.relay_url);
    assert_eq!(rank.author_count, cloned.author_count);
    assert_eq!(rank.covered_pubkeys, cloned.covered_pubkeys);
}
