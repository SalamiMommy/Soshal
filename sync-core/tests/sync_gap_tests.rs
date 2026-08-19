//! Gap-fill coverage for sync-core: outbox compress/fail/batch paths and
//! Prolly Tree + Prolly Sync session flows.

use soshal_sync_core::outbox::{
    compress_payload, decompress_payload, enqueue_outbox_item, fetch_pending_outbox_items,
    mark_outbox_item_failed, mark_outbox_items_completed, summarize_outbox,
};
use soshal_sync_core::prolly_sync::{ProllySyncMessage, ProllySyncSession};
use soshal_sync_core::prolly_tree::{ProllyTree, GEAR_MASK};

#[test]
fn outbox_payload_compress_roundtrips() {
    let small = "{}";
    assert_eq!(compress_payload(small), "{}");
    let big = format!("{{\"data\":\"{}\"}}", "x".repeat(600));
    let compressed = compress_payload(&big);
    assert!(
        compressed.starts_with("__zstd_b64__:"),
        "big payloads compress"
    );
    assert_eq!(decompress_payload(&compressed), big);
    assert_eq!(
        decompress_payload("__zstd_b64__:deadbeef"),
        "__zstd_b64__:deadbeef"
    );
    assert_eq!(decompress_payload("plain"), "plain");
    // Legacy hex form still decodes.
    let short_magic = format!("__zstd__:{}", hex::encode(b"Zst"));
    assert_eq!(decompress_payload(&short_magic), short_magic);
}

#[test]
fn outbox_failure_backoff_and_exhaustion() {
    let db = soshal_test_util::test_db();
    enqueue_outbox_item(&db, "f1", "post", "{}", None, 0).unwrap();
    mark_outbox_item_failed(&db, "f1", 1, 102).unwrap();
    let pending = fetch_pending_outbox_items(&db, 100, 10).unwrap();
    assert_eq!(pending.len(), 0, "next_retry_at moved into the future");
    let retried = fetch_pending_outbox_items(&db, 100 + 3, 10).unwrap();
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].retry_count, 1);
    assert_eq!(retried[0].next_retry_at, 102, "2s backoff");
    for i in 1..10 {
        mark_outbox_item_failed(&db, "f1", i, 1000).unwrap();
    }
    mark_outbox_item_failed(&db, "f1", 10, 1000).unwrap();
    let summary = summarize_outbox(&db).unwrap();
    assert_eq!(summary.pending_count, 0);
    assert_eq!(summary.failed_count, 1);
    let cap = fetch_pending_outbox_items(&db, 1000 + 3600, 10).unwrap();
    assert!(cap.is_empty(), "retries exhausted -> failed status");
}

#[test]
fn outbox_batch_complete_and_limit() {
    let db = soshal_test_util::test_db();
    for i in 0..4 {
        enqueue_outbox_item(&db, &format!("b{i}"), "post", "{}", None, i).unwrap();
    }
    let paged = fetch_pending_outbox_items(&db, 100, 2).unwrap();
    assert_eq!(paged.len(), 2);
    assert_eq!(paged[0].id, "b0");
    mark_outbox_items_completed(&db, &["b0".into(), "b1".into()]).unwrap();
    mark_outbox_items_completed(&db, &[]).unwrap();
    let remaining = fetch_pending_outbox_items(&db, 100, 10).unwrap();
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].id, "b2");
    let summary = summarize_outbox(&db).unwrap();
    assert_eq!(summary.total_count, 4);
    assert_eq!(summary.pending_count, 2);
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn prolly_tree_empty_build_and_hash() {
    let tree = ProllyTree::build(&[]);
    assert_eq!(tree.nodes.len(), 1);
    assert_eq!(tree.root_hash, tree.nodes[0].node_hash);
    let again = ProllyTree::build(&[]);
    assert_eq!(tree.root_hash, again.root_hash, "deterministic");
}

#[test]
fn prolly_tree_build_and_hashes() {
    let tree = ProllyTree::build(&pairs(&[
        ("a", "1"),
        ("b", "2"),
        ("c", "3"),
        ("d", "4"),
        ("e", "5"),
        ("f", "6"),
        ("g", "7"),
        ("h", "8"),
    ]));
    assert!(!tree.root_hash.is_empty());
    assert!(!tree.nodes.is_empty());
    assert_eq!(
        tree.root_hash,
        ProllyTree::build(&pairs(&[
            ("a", "1"),
            ("b", "2"),
            ("c", "3"),
            ("d", "4"),
            ("e", "5"),
            ("f", "6"),
            ("g", "7"),
            ("h", "8")
        ]))
        .root_hash,
        "same data -> same root"
    );
    let different = ProllyTree::build(&pairs(&[("a", "1"), ("b", "9")]));
    assert_ne!(
        tree.root_hash, different.root_hash,
        "data change -> root change"
    );
    let all_keys: Vec<String> = tree.nodes.iter().flat_map(|n| n.keys.clone()).collect();
    assert!(all_keys.contains(&"a".to_string()));
}

#[test]
fn prolly_tree_gear_hash_and_boundary() {
    let mut prev = ProllyTree::gear_hash(b"a");
    for i in 0..50u8 {
        let h = ProllyTree::gear_hash(&[i]);
        assert_eq!(h & 0x0FFF, h & GEAR_MASK);
        let _ = prev;
        prev = h;
    }
    assert!(!ProllyTree::is_boundary(b"hello"));
    let node_hash = ProllyTree::compute_node_hash(1, &["k".into()], &["c".into()]);
    assert_eq!(node_hash.len(), 64);
    assert_eq!(
        node_hash,
        ProllyTree::compute_node_hash(1, &["k".into()], &["c".into()])
    );
}

#[test]
fn prolly_sync_session_match_and_branch_flows() {
    let local = ProllyTree::build(&pairs(&[("a", "1"), ("b", "2")]));
    let mut session = ProllySyncSession::new(local);
    assert_eq!(session.peer_root_hash, None);
    assert!(session.missing_keys.is_empty());

    let reply = session.handle_message(ProllySyncMessage::RootExchange {
        root_hash: session.local_tree.root_hash.clone(),
    });
    assert_eq!(reply, Some(ProllySyncMessage::Match));

    let reply = session
        .handle_message(ProllySyncMessage::RootExchange {
            root_hash: "different-root".into(),
        })
        .unwrap();
    assert!(matches!(
        reply,
        ProllySyncMessage::RequestBranch { level: 0, .. }
    ));

    let branch_node = session.local_tree.nodes[0].clone();
    let reply = session
        .handle_message(ProllySyncMessage::RequestBranch {
            level: 0,
            node_hash: branch_node.node_hash.clone(),
        })
        .unwrap();
    match reply {
        ProllySyncMessage::ResponseBranch {
            node_hash, keys, ..
        } => {
            assert_eq!(node_hash, branch_node.node_hash);
            assert!(!keys.is_empty());
        }
        other => panic!("expected ResponseBranch, got {other:?}"),
    }
}

#[test]
fn prolly_sync_session_requests_missing_deltas() {
    let local = ProllyTree::build(&pairs(&[("a", "1")]));
    let mut session = ProllySyncSession::new(local);
    let reply = session
        .handle_message(ProllySyncMessage::ResponseBranch {
            node_hash: "peer-node".into(),
            keys: vec!["a".into(), "zzz".into()],
            child_hashes: vec![],
        })
        .unwrap();
    match reply {
        ProllySyncMessage::RequestDeltas { missing_ids } => {
            assert_eq!(missing_ids, vec!["zzz".to_string()]);
        }
        other => panic!("expected RequestDeltas, got {other:?}"),
    }
    assert_eq!(session.missing_keys, vec!["zzz".to_string()]);

    let reply = session.handle_message(ProllySyncMessage::ResponseBranch {
        node_hash: "peer-node".into(),
        keys: vec!["a".into()],
        child_hashes: vec![],
    });
    assert_eq!(
        reply,
        Some(ProllySyncMessage::Match),
        "no missing keys -> match"
    );

    assert_eq!(
        session.handle_message(ProllySyncMessage::RequestDeltas {
            missing_ids: vec![]
        }),
        None
    );
    assert_eq!(
        session.handle_message(ProllySyncMessage::ResponseDeltas {
            payload_json: "{}".into()
        }),
        None
    );
    assert_eq!(
        session.handle_message(ProllySyncMessage::ResponsePatch {
            base_id: "a".into(),
            new_id: "b".into(),
            patch_b64: "".into(),
            full_b64: None
        }),
        None
    );
    assert_eq!(session.handle_message(ProllySyncMessage::Match), None);
    let unknown = session.handle_message(ProllySyncMessage::RequestBranch {
        level: 1,
        node_hash: "ghost".into(),
    });
    assert!(unknown.is_none(), "unknown branch hash -> no response");
}
