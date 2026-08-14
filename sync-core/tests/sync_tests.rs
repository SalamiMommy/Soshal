//! Integration tests for soshal-sync-core: verified-event ingest into the
//! local cache, per-kind watermarks, revert strategies, and PlumTree gossip
//! bridging.

use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::Keys;
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::reaction::ReactionRepo;
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::user::UserRepo;
use soshal_db_core::Database;
use soshal_sync_core::gossip::GossipSyncBridge;
use soshal_sync_core::ingest::{handle, handle_batch, set_watermark, watermark, watermark_key};
use soshal_sync_core::revert::{revert, KIND_LIKE, KIND_POST, KIND_PROFILE};
use soshal_sync_core::{SyncUpdate, WM_FEED, WM_META};
use tokio::sync::mpsc;

fn gossip_msg(event: &Event) -> soshal_network_core::plumtree::PlumTreeMessage {
    soshal_network_core::plumtree::PlumTreeMessage::Gossip {
        message_id: event.id.to_hex(),
        payload_json: serde_json::to_string(event).unwrap(),
        round: 0,
    }
}

fn signed_event(keys: &Keys, kind: Kind, content: &str, tags: Vec<Vec<String>>) -> Event {
    let mut builder = EventBuilder::new(kind, content);
    for t in tags {
        builder = builder.tag(Tag::parse(t).unwrap());
    }
    builder.finalize(keys).unwrap()
}

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

fn channel() -> (mpsc::Sender<SyncUpdate>, mpsc::Receiver<SyncUpdate>) {
    mpsc::channel(16)
}

/// FK: `posts.pubkey` references `users(pubkey)` — ensure the author exists.
fn seed_user(db: &Database, pubkey: &str) {
    UserRepo::new(db).ensure_exists(pubkey).unwrap();
}

fn post_row(id: &str, content: &str) -> PostRow {
    PostRow {
        id: id.to_string(),
        pubkey: "aa".repeat(32),
        content: content.to_string(),
        kind: 1,
        created_at: 1_700_000_000,
        tags_json: "[]".to_string(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    }
}

#[test]
fn ingest_text_note_caches_post_and_emits_feed() {
    let db = test_db();
    let keys = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(&keys, Kind::TextNote, "hello mesh", vec![]);
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
    let db = test_db();
    let keys = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(
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
    let db = test_db();
    let keys = Keys::generate();
    let event = signed_event(&keys, Kind::TextNote, &"x".repeat(70 * 1024), vec![]);
    let (tx, _rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    assert!(PostRepo::new(&db)
        .get_by_id(&event.id.to_hex())
        .unwrap()
        .is_none());
}

#[test]
fn ingest_dm_addressed_to_me_emits_update() {
    let db = test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let event = signed_event(
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
    let db = test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let other = Keys::generate();
    let event = signed_event(
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
    let db = test_db();
    let me = Keys::generate();
    let peer = Keys::generate();
    let event = signed_event(
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
    let db = test_db();
    let keys = Keys::generate();
    let event = signed_event(
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
    let db = test_db();
    let keys = Keys::generate();
    let target = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(
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
    let db = test_db();
    let keys = Keys::generate();
    let event = signed_event(&keys, Kind::Reaction, "+", vec![]);
    let (tx, mut rx) = channel();
    handle(&db, "", &event, &tx).unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn ingest_metadata_upserts_user() {
    let db = test_db();
    let keys = Keys::generate();
    let event = signed_event(
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
    let db = test_db();
    let keys = Keys::generate();
    let mut event = signed_event(&keys, Kind::TextNote, "hello", vec![]);
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
    let db = test_db();
    let keys = Keys::generate();
    let e1 = signed_event(&keys, Kind::TextNote, "one", vec![]);
    let e2 = signed_event(&keys, Kind::TextNote, "two", vec![]);
    seed_user(&db, &e1.pubkey.to_hex());
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
    assert_eq!(watermark_key(Kind::EncryptedDirectMessage), None);
    assert_eq!(watermark_key(Kind::Reaction), None);

    let db = test_db();
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
    let db = test_db();
    seed_user(&db, &"aa".repeat(32));
    PostRepo::new(&db)
        .upsert(&post_row("post-1", "hello"))
        .unwrap();
    let conn = db.conn().unwrap();
    revert(&conn, KIND_POST, r#"{"id":"post-1"}"#).unwrap();
    drop(conn);
    let row = PostRepo::new(&db).get_by_id("post-1").unwrap().unwrap();
    assert!(row.is_deleted);
}

#[test]
fn revert_like_removes_reaction() {
    let db = test_db();
    let keys = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(
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
    let db = test_db();
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
    let db = test_db();
    let conn = db.conn().unwrap();
    revert(&conn, "unknown-kind", r#"{"id":"x"}"#).unwrap();
    assert!(revert(&conn, KIND_POST, "not-json").is_err());
    assert!(revert(&conn, KIND_POST, r#"{"id":42}"#).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn gossip_forwards_to_eager_peers_and_ingests() {
    let db = test_db();
    let keys = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(&keys, Kind::TextNote, "gossiped", vec![]);
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
    let db = test_db();
    let keys = Keys::generate();
    seed_user(&db, &keys.public_key().to_hex());
    let event = signed_event(&keys, Kind::TextNote, "dup", vec![]);
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
