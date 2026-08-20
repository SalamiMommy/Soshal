//! Integration tests for soshal-sync-core: verified-event ingest into the
//! local cache, per-kind watermarks, revert strategies, and PlumTree gossip
//! bridging.

use nostr::event::{Event, Kind};
use nostr::key::Keys;
use sha2::{Digest, Sha256};
use soshal_db_core::query::query_first;
use soshal_db_core::repos::bookmark::BookmarkRepo;
use soshal_db_core::repos::post::PostRepo;
use soshal_db_core::repos::reaction::ReactionRepo;
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::user::UserRepo;
use soshal_db_core::repos::zap::ZapRepo;
use soshal_sync_core::epoch_gc::EpochGarbageCollector;
use soshal_sync_core::gossip::GossipSyncBridge;
use soshal_sync_core::ingest::{handle, handle_batch, set_watermark, watermark, watermark_key};
use soshal_sync_core::outbox::{
    enqueue_outbox_item, fetch_pending_outbox_items, fetch_pending_outbox_items_filtered,
    mark_outbox_item_completed, summarize_outbox,
};
use soshal_sync_core::revert::{revert, KIND_LIKE, KIND_POST, KIND_PROFILE};
use soshal_sync_core::tx::{tx_begin, tx_link, tx_mark_applied, tx_statuses, STATUS_APPLIED};
use soshal_sync_core::zk_rollup::{CommitmentRollup, RollupEngine};
use soshal_sync_core::{SyncUpdate, WM_DM, WM_FEED, WM_META};
use std::collections::HashMap;
use tokio::sync::mpsc;

fn gossip_msg(event: &Event) -> soshal_network_core::plumtree::PlumTreeMessage {
    soshal_network_core::plumtree::PlumTreeMessage::Gossip {
        message_id: event.id.to_hex(),
        payload_json: serde_json::to_string(event).unwrap().into(),
        round: 0,
    }
}

fn channel() -> (mpsc::Sender<SyncUpdate>, mpsc::Receiver<SyncUpdate>) {
    mpsc::channel(16)
}

#[test]
fn ingest_text_note_caches_post_and_emits_feed() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "hello mesh", vec![]);
    let (tx, mut rx) = channel();
    handle(&db, "", &event, &tx).unwrap();

    let row = PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(row.content, "hello mesh");
    assert_eq!(row.pubkey, event.pubkey.to_hex());
    assert_eq!(row.sync_status, "synced");

    match rx.try_recv().unwrap() {
        SyncUpdate::Feed { id, content, .. } => {
            assert_eq!(id, event.id.to_hex());
            assert_eq!(content, "hello mesh");
        }
        other => panic!("unexpected update: {other:?}"),
    }
}

#[test]
fn ingest_freenet_tag_sets_native_fields() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::TextNote,
        "mesh note",
        vec![vec!["freenet".to_string(), "key123".to_string()]],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();

    let row = PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(row.freenet_key.as_deref(), Some("key123"));
    assert!(row.is_freenet_native);
}

#[test]
fn ingest_oversized_content_skipped() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::TextNote,
        &"x".repeat(70 * 1024),
        vec![],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    assert!(PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .is_none());
}

#[test]
fn ingest_contact_list_sets_user_contacts() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let a = Keys::generate();
    let b = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::ContactList,
        "",
        vec![
            vec!["p".to_string(), a.public_key().to_hex()],
            vec!["p".to_string(), b.public_key().to_hex()],
        ],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    let user = UserRepo::new(&db)
        .get_by_pubkey(&event.pubkey.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(
        user.contact_pubkeys,
        format!("{},{}", a.public_key().to_hex(), b.public_key().to_hex())
    );
}

#[test]
fn ingest_zap_receipt_creates_zap_row() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let recipient = Keys::generate();
    let my_pubkey = recipient.public_key().to_hex();
    // NIP-57: a receipt without a locally-ingested zap-request binding
    // (matching description hash + amount) is forged-or-unverifiable and
    // must NOT be counted toward zap totals.
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::ZapReceipt,
        "lnbc210u",
        vec![
            vec!["p".to_string(), my_pubkey.clone()],
            vec!["e".to_string(), "target-id".to_string()],
        ],
    );
    let (tx, _rx) = channel();
    handle(&db, &my_pubkey, &event, &tx).unwrap();
    assert_eq!(ZapRepo::new(&db).sum_by_event("target-id").unwrap(), 0);
}

#[test]
fn ingest_bookmarks_creates_bookmark_row() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::Bookmarks,
        "saved stuff",
        vec![vec!["e".to_string(), "post-1".to_string()]],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    let row = BookmarkRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(row.event_id, "post-1");
}

#[test]
fn ingest_dm_addressed_to_me_emits_update() {
    let db = soshal_test_util::test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &peer,
        Kind::EncryptedDirectMessage,
        "encrypted-blob",
        vec![vec!["p".to_string(), me.public_key().to_hex()]],
    );
    let (tx, mut rx) = channel();
    handle(&db, &me.public_key().to_hex(), &event, &tx).unwrap();

    match rx.try_recv().unwrap() {
        SyncUpdate::Dm {
            sender, content, ..
        } => {
            assert_eq!(sender, peer.public_key().to_hex());
            assert_eq!(content, "encrypted-blob");
        }
        other => panic!("unexpected update: {other:?}"),
    }
    // DMs are never persisted raw by ingest — the bridge decrypts first.
    assert!(PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .is_none());
}

#[test]
fn ingest_dm_not_addressed_to_me_skipped() {
    let db = soshal_test_util::test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let other = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &peer,
        Kind::EncryptedDirectMessage,
        "encrypted-blob",
        vec![vec!["p".to_string(), other.public_key().to_hex()]],
    );
    let (tx, mut rx) = channel();
    handle(&db, &me.public_key().to_hex(), &event, &tx).unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn ingest_dm_authored_by_me_emits_update() {
    let db = soshal_test_util::test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &me,
        Kind::EncryptedDirectMessage,
        "outgoing-blob",
        vec![vec!["p".to_string(), peer.public_key().to_hex()]],
    );
    let (tx, mut rx) = channel();
    handle(&db, &me.public_key().to_hex(), &event, &tx).unwrap();
    assert!(matches!(rx.try_recv().unwrap(), SyncUpdate::Dm { .. }));
}

#[test]
fn ingest_custom_kind_cached_as_post() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::Custom(20082),
        r#"{"t":"group_dist","groupId":"group-42"}"#,
        vec![],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    let row = PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(row.content, r#"{"t":"group_dist","groupId":"group-42"}"#);
}

#[test]
fn ingest_reaction_caches_with_e_tag() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let target = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::Reaction,
        "+",
        vec![vec!["e".to_string(), target.public_key().to_hex()]],
    );
    let (tx, mut rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    let reactions = ReactionRepo::new(&db)
        .get_by_event(&target.public_key().to_hex())
        .unwrap();
    assert_eq!(reactions.len(), 1);
    assert!(matches!(
        rx.try_recv().unwrap(),
        SyncUpdate::Reaction { .. }
    ));
}

#[test]
fn ingest_reaction_without_e_tag_skipped() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(&keys, Kind::Reaction, "+", vec![]);
    let (tx, mut rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn ingest_metadata_upserts_user() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::Metadata,
        r#"{"name":"alice","about":"builder","lud16":"alice@example.com"}"#,
        vec![],
    );
    let (tx, mut rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    let user = UserRepo::new(&db)
        .get_by_pubkey(&event.pubkey.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(user.name.as_deref(), Some("alice"));
    assert_eq!(user.lud16.as_deref(), Some("alice@example.com"));
    assert!(user.npub.starts_with("npub1"));
    assert!(matches!(rx.try_recv().unwrap(), SyncUpdate::Profile { .. }));
}

#[test]
fn ingest_rejects_unverified_event() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let mut event = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "hello", vec![]);
    event.content = "tampered".to_string();
    let (tx, _rx) = channel();
    assert!(handle(&db, "", &event, &tx).is_err());
    assert!(PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .is_none());
}

#[test]
fn ingest_batch_applies_all_events() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let e1 = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "one", vec![]);
    let e2 = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "two", vec![]);
    soshal_test_util::seed_user(&db, &e1.pubkey.to_hex());
    let (tx, _rx) = channel();
    handle_batch(&db, "", &[e1.clone(), e2.clone()], &tx).unwrap();
    assert!(PostRepo::new(&db)
        .get_by_id(&e1.id.to_hex())
        .unwrap()
        .is_some());
    assert!(PostRepo::new(&db)
        .get_by_id(&e2.id.to_hex())
        .unwrap()
        .is_some());
}

#[test]
fn watermark_roundtrip_and_kind_mapping() {
    assert_eq!(watermark_key(Kind::TextNote), Some(WM_FEED));
    assert_eq!(watermark_key(Kind::Metadata), Some(WM_META));
    assert_eq!(watermark_key(Kind::EncryptedDirectMessage), Some(WM_DM));
    assert_eq!(watermark_key(Kind::Reaction), Some(WM_FEED));

    let db = soshal_test_util::test_db();
    assert_eq!(watermark(&db, WM_FEED), 0);
    set_watermark(&db, WM_FEED, 1_700_000_123);
    assert_eq!(watermark(&db, WM_FEED), 1_700_000_123);
    assert_eq!(
        SettingsRepo::new(&db).get(WM_FEED).unwrap().unwrap(),
        "1700000123"
    );
}

#[test]
fn revert_post_tombstones_not_deletes() {
    let db = soshal_test_util::test_db();
    soshal_test_util::seed_user(&db, &"aa".repeat(32));
    PostRepo::new(&db)
        .upsert(&soshal_test_util::post_row("post-1"))
        .unwrap();
    let conn = db.conn().unwrap();
    revert(&conn, KIND_POST, r#"{"id":"post-1"}"#).unwrap();
    drop(conn);
    let row = PostRepo::new(&db).get_by_id("post-1").unwrap().unwrap();
    assert!(row.is_deleted);
}

#[test]
fn revert_like_removes_reaction() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(
        &keys,
        Kind::Reaction,
        "+",
        vec![vec!["e".to_string(), "target-id".to_string()]],
    );
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    assert_eq!(
        ReactionRepo::new(&db)
            .get_by_event("target-id")
            .unwrap()
            .len(),
        1
    );
    let conn = db.conn().unwrap();
    revert(
        &conn,
        KIND_LIKE,
        &format!(r#"{{"id":"{}"}}"#, event.id.to_hex()),
    )
    .unwrap();
    drop(conn);
    assert!(ReactionRepo::new(&db)
        .get_by_event("target-id")
        .unwrap()
        .is_empty());
}

#[test]
fn revert_profile_restores_prior_fields() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    UserRepo::new(&db)
        .upsert(&soshal_db_core::repos::user::UserRow {
            pubkey: pubkey.clone(),
            npub: String::new(),
            name: Some("new-name".to_string()),
            display_name: Some("new-display".to_string()),
            about: Some("new-about".to_string()),
            picture: Some("new-pic".to_string()),
            banner: None,
            nip05: None,
            lud16: None,
            created_at: 0,
            updated_at: 0,
            metadata_json: None,
            contact_pubkeys: String::new(),
            relay_list: String::new(),
        })
        .unwrap();
    let payload = format!(
        r#"{{"pubkey":"{pubkey}","prior":{{"name":"old-name","display_name":"old-display","about":"old-about","picture":"old-pic"}}}}"#
    );
    let conn = db.conn().unwrap();
    revert(&conn, KIND_PROFILE, &payload).unwrap();
    drop(conn);
    let user = UserRepo::new(&db).get_by_pubkey(&pubkey).unwrap().unwrap();
    assert_eq!(user.name.as_deref(), Some("old-name"));
    assert_eq!(user.display_name.as_deref(), Some("old-display"));
    assert_eq!(user.about.as_deref(), Some("old-about"));
    assert_eq!(user.picture.as_deref(), Some("old-pic"));
}

#[test]
fn revert_unknown_kind_and_bad_payload_are_noops() {
    let db = soshal_test_util::test_db();
    let conn = db.conn().unwrap();
    revert(&conn, "unknown-kind", r#"{"id":"x"}"#).unwrap();
    assert!(revert(&conn, KIND_POST, "not-json").is_err());
    assert!(revert(&conn, KIND_POST, r#"{"id":42}"#).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn gossip_forwards_to_eager_peers_and_ingests() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "gossiped", vec![]);
    let msg = gossip_msg(&event);
    let bridge = GossipSyncBridge::new("self");
    bridge.node.write().await.add_peer("peer_a");
    bridge.node.write().await.add_peer("peer_b");
    let (tx, _rx) = channel();
    let outgoing = bridge.process_gossip(&db, "sender", msg, &tx).await;

    let forwarded: Vec<_> = outgoing
        .iter()
        .filter(|(_, m)| {
            matches!(
                m,
                soshal_network_core::plumtree::PlumTreeMessage::Gossip { .. }
            )
        })
        .map(|(p, _)| p.as_str())
        .collect();
    assert_eq!(forwarded.len(), 2);
    assert!(forwarded.contains(&"peer_a"));
    assert!(forwarded.contains(&"peer_b"));

    let row = PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .unwrap();
    assert_eq!(row.content, "gossiped");
}

#[tokio::test(flavor = "multi_thread")]
async fn gossip_duplicate_emits_prune() {
    let db = soshal_test_util::test_db();
    let keys = Keys::generate();
    soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
    let event = soshal_test_util::signed_event_tagged(&keys, Kind::TextNote, "dup", vec![]);
    let msg = gossip_msg(&event);
    let bridge = GossipSyncBridge::new("self");
    bridge.node.write().await.add_peer("peer_a");
    bridge.node.write().await.add_peer("peer_b");
    let (tx, _rx) = channel();
    let first = bridge.process_gossip(&db, "peer_a", msg.clone(), &tx).await;
    assert!(first.iter().any(|(p, m)| p == "peer_b"
        && matches!(
            m,
            soshal_network_core::plumtree::PlumTreeMessage::Gossip { .. }
        )));
    let second = bridge.process_gossip(&db, "peer_a", msg, &tx).await;
    assert!(second.iter().any(|(p, m)| p == "peer_a"
        && matches!(
            m,
            soshal_network_core::plumtree::PlumTreeMessage::Prune { .. }
        )));
}

#[test]
fn epoch_gc_prunes_old_tombstones_keeps_new_and_live() {
    let db = soshal_test_util::test_db();
    let pubkey = "aa".repeat(32);
    soshal_test_util::seed_user(&db, &pubkey);
    let repo = PostRepo::new(&db);
    let mut old_tomb = soshal_test_util::post_row_content("old_tomb", "gone");
    old_tomb.is_deleted = true;
    old_tomb.created_at = 86_000;
    let mut new_tomb = soshal_test_util::post_row_content("new_tomb", "kept");
    new_tomb.is_deleted = true;
    new_tomb.created_at = 87_000;
    let mut live = soshal_test_util::post_row_content("live_old", "live");
    live.created_at = 1_000;
    repo.upsert(&old_tomb).unwrap();
    repo.upsert(&new_tomb).unwrap();
    repo.upsert(&live).unwrap();

    let mut clocks = HashMap::new();
    clocks.insert("peer1".to_string(), 100_000);
    clocks.insert("peer2".to_string(), 90_000);
    let summary = EpochGarbageCollector::prune_tombstones_if_consensus_reached(
        &db.conn().unwrap(),
        "posts_feed",
        &clocks,
        3_600,
    )
    .unwrap();

    assert_eq!(summary.domain, "posts_feed");
    assert_eq!(summary.epoch_counter, 1);
    assert_eq!(summary.pruned_tombstones, 1);
    assert_eq!(summary.bytes_reclaimed, 512);
    assert!(repo.get_by_id("old_tomb").unwrap().is_none());
    assert!(repo.get_by_id("new_tomb").unwrap().unwrap().is_deleted);
    assert!(!repo.get_by_id("live_old").unwrap().unwrap().is_deleted);
}

#[test]
fn epoch_gc_increments_epoch_counter_per_run() {
    let db = soshal_test_util::test_db();
    let mut clocks = HashMap::new();
    clocks.insert("peer1".to_string(), 100_000);
    let conn = db.conn().unwrap();
    let first = EpochGarbageCollector::prune_tombstones_if_consensus_reached(
        &conn,
        "posts_feed",
        &clocks,
        3_600,
    )
    .unwrap();
    let second = EpochGarbageCollector::prune_tombstones_if_consensus_reached(
        &conn,
        "posts_feed",
        &clocks,
        3_600,
    )
    .unwrap();
    assert_eq!(first.epoch_counter, 1);
    assert_eq!(second.epoch_counter, 2);
}

#[test]
fn epoch_gc_is_noop_without_peer_clocks() {
    let db = soshal_test_util::test_db();
    let summary = EpochGarbageCollector::prune_tombstones_if_consensus_reached(
        &db.conn().unwrap(),
        "posts_feed",
        &HashMap::new(),
        3_600,
    )
    .unwrap();
    assert_eq!(summary.epoch_counter, 0);
    assert_eq!(summary.pruned_tombstones, 0);
    assert_eq!(summary.bytes_reclaimed, 0);
}

fn rollup(thread_id: &str, ops: u64, genesis: &str, final_state: &str) -> CommitmentRollup {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_bytes());
    hasher.update(genesis.as_bytes());
    hasher.update(final_state.as_bytes());
    hasher.update(ops.to_le_bytes());
    CommitmentRollup {
        thread_id: thread_id.to_string(),
        genesis_root: genesis.to_string(),
        final_state_root: final_state.to_string(),
        operation_count: ops,
        commitment_hex: hex::encode(hasher.finalize()),
    }
}

#[test]
fn zk_rollup_valid_commitment_verifies_and_tampered_rejected() {
    let genesis = "g".repeat(64);
    let final_state = "f".repeat(64);
    let engine = RollupEngine::new();

    let ok = engine.verify_rollup(&rollup("zr1", 42, &genesis, &final_state));
    assert!(ok.verified);
    assert_eq!(ok.verified_operations, 42);
    assert_eq!(ok.error_msg, None);

    let mut tampered_proof = rollup("zr2", 7, &genesis, &final_state);
    tampered_proof.commitment_hex = hex::encode([0u8; 32]);
    let bad = engine.verify_rollup(&tampered_proof);
    assert!(!bad.verified);
    assert_eq!(bad.verified_operations, 0);
    assert_eq!(bad.error_msg.as_deref(), Some("Rollup commitment mismatch"));

    let mut tampered_state = rollup("zr3", 7, &genesis, &final_state);
    tampered_state.final_state_root = "x".repeat(64);
    assert!(!engine.verify_rollup(&tampered_state).verified);
}

#[test]
fn zk_rollup_apply_writes_upserts_and_rejects_tampered() {
    let db = soshal_test_util::test_db();
    let conn = db.conn().unwrap();
    let genesis = "g".repeat(64);
    let final_state = "f".repeat(64);
    let engine = RollupEngine::new();

    assert!(engine
        .apply_rollup_to_db(&conn, &rollup("thread_1", 5, &genesis, &final_state))
        .unwrap());
    let (root, ops): (String, i64) = query_first(
        &conn,
        "SELECT final_state_root, operation_count FROM zk_state_rollups WHERE thread_id = ?1",
        libsql::params!["thread_1"],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(root, final_state);
    assert_eq!(ops, 5);

    assert!(engine
        .apply_rollup_to_db(&conn, &rollup("thread_1", 9, &genesis, &final_state))
        .unwrap());
    let ops2: i64 = query_first(
        &conn,
        "SELECT operation_count FROM zk_state_rollups WHERE thread_id = ?1",
        libsql::params!["thread_1"],
        |r| r.get::<i64>(0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(ops2, 9);

    let mut tampered = rollup("thread_2", 1, &genesis, &final_state);
    tampered.commitment_hex = hex::encode([0u8; 32]);
    let err = engine.apply_rollup_to_db(&conn, &tampered).unwrap_err();
    assert_eq!(err, "Rollup commitment mismatch");
}

#[test]
fn tx_record_status_update_and_list() {
    let db = soshal_test_util::test_db();
    tx_begin(&db, "n1", "post", r#"{"id":"n1"}"#, 1).unwrap();
    tx_begin(&db, "n2", "like", r#"{"id":"n2"}"#, 2).unwrap();
    tx_link(&db, "n1", "n2").unwrap();
    tx_mark_applied(&db, "n2").unwrap();

    let nodes = tx_statuses(&db).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].id, "n2");
    assert_eq!(nodes[0].status, STATUS_APPLIED);
    assert_eq!(nodes[1].id, "n1");
    assert_eq!(nodes[1].status, "pending");
}

#[test]
fn outbox_enqueue_pending_count_and_complete() {
    let db = soshal_test_util::test_db();
    enqueue_outbox_item(&db, "o1", "post", "{}", None, 100).unwrap();
    enqueue_outbox_item(
        &db,
        "o2",
        "image",
        r#"{"path":"x"}"#,
        Some("media/x.jpg"),
        200,
    )
    .unwrap();

    let summary = summarize_outbox(&db).unwrap();
    assert_eq!(summary.pending_count, 2);
    assert_eq!(summary.failed_count, 0);
    assert_eq!(summary.total_count, 2);

    // Media-backed items are excluded from the plain fetch (separate media
    // uploader path); include_media=true returns the full pending set.
    let pending = fetch_pending_outbox_items(&db, 200, 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "o1");
    assert_eq!(pending[0].media_path, None);
    let pending_all = fetch_pending_outbox_items_filtered(&db, 200, 10, true).unwrap();
    assert_eq!(pending_all.len(), 2);
    assert_eq!(pending_all[0].id, "o1");
    assert_eq!(pending_all[0].media_path, None);
    assert_eq!(pending_all[1].id, "o2");
    assert_eq!(pending_all[1].media_path.as_deref(), Some("media/x.jpg"));
    assert_eq!(pending_all[1].payload_json, r#"{"path":"x"}"#);

    mark_outbox_item_completed(&db, "o1").unwrap();
    let after = summarize_outbox(&db).unwrap();
    assert_eq!(after.pending_count, 1);
    assert_eq!(after.total_count, 2);

    // Plain fetch stays media-excluded (o2 has a media path).
    let remaining = fetch_pending_outbox_items(&db, 200, 10).unwrap();
    assert!(remaining.is_empty());
    let remaining_all = fetch_pending_outbox_items_filtered(&db, 200, 10, true).unwrap();
    assert_eq!(remaining_all.len(), 1);
    assert_eq!(remaining_all[0].id, "o2");
}
