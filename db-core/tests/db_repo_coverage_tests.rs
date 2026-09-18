use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::ephemeral_media::{EphemeralMediaRepo, EphemeralMediaRow};
use soshal_db_core::repos::escrow::{EscrowRepo, EscrowRow};
use soshal_db_core::repos::geohash_peer::{GeohashPeerRepo, GeohashPeerRow};
use soshal_db_core::repos::group::{GroupRepo, GroupRow};
use soshal_db_core::repos::hashtag::{HashtagRepo, HashtagRow};
use soshal_db_core::repos::media::{MediaRepo, MediaRow};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::reminder::{ReminderRepo, ReminderRow};
use soshal_db_core::repos::role::{GroupRoleRepo, GroupRoleRow};
use soshal_db_core::repos::search_index::build_fts_query;
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_db_core::repos::zap::{ZapRepo, ZapRow};
use soshal_db_core::Database;

fn insert_test_user(db: &Database, pubkey: &str) {
    let repo = UserRepo::new(db);
    let user = UserRow {
        pubkey: pubkey.into(),
        npub: format!("npub_{}", pubkey),
        name: Some("test".into()),
        display_name: None,
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1000,
        updated_at: 1000,
        metadata_json: None,
        contact_pubkeys: "[]".into(),
        relay_list: "[]".into(),
        follower_count: 0,
    };
    repo.upsert(&user).unwrap();
}

fn test_post(id: &str, pubkey: &str, content: &str, created_at: i64) -> PostRow {
    PostRow {
        id: id.into(),
        pubkey: pubkey.into(),
        content: content.into(),
        kind: 1,
        created_at,
        tags_json: "[]".into(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: "[]".into(),
        mentioned_hashtags: "[]".into(),
        subject: None,
        sync_status: "synced".into(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    }
}

fn new_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

#[test]
fn bookmark_get_user_bookmarks_paged() {
    let db = new_db();
    insert_test_user(&db, "pkBm");
    let repo = BookmarkRepo::new(&db);
    for i in 0..3 {
        repo.upsert(&BookmarkRow {
            id: format!("bm{i}"),
            pubkey: "pkBm".into(),
            event_id: format!("ev{i}"),
            created_at: 1000 + i,
        })
        .unwrap();
    }
    let rows = repo.get_user_bookmarks("pkBm", 2, 0).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].event_id, "ev2", "newest first");
    let page2 = repo.get_user_bookmarks("pkBm", 2, 2).unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].event_id, "ev0");
    let none = repo.get_user_bookmarks("pkOther", 10, 0).unwrap();
    assert!(none.is_empty());
}

#[test]
fn bookmark_upsert_in_conflict_updates_event_id() {
    let db = new_db();
    insert_test_user(&db, "pk");
    let repo = BookmarkRepo::new(&db);
    let row = BookmarkRow {
        id: "bm1".into(),
        pubkey: "pk".into(),
        event_id: "ev1".into(),
        created_at: 1000,
    };
    repo.upsert(&row).unwrap();
    let conn = db.conn().unwrap();
    let mut replaced = row;
    replaced.event_id = "ev2".into();
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &replaced).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let found = repo.get_by_id("bm1").unwrap().unwrap();
    assert_eq!(found.event_id, "ev2", "conflict must refresh event_id");
}

#[test]
fn ephemeral_media_get_by_message_id() {
    let db = new_db();
    let repo = EphemeralMediaRepo::new(&db);
    let row = EphemeralMediaRow {
        id: "em1".into(),
        message_id: "msg1".into(),
        conversation_id: "conv1".into(),
        conversation_type: "dm".into(),
        media_url: "blob:abc".into(),
        media_type: "image/jpeg".into(),
        sender_pubkey: "pkA".into(),
        recipient_pubkey: "pkB".into(),
        max_views: 1,
        current_views: 0,
        state: "pending".into(),
        expires_at: None,
        created_at: 1000,
        viewed_at: None,
    };
    repo.create(&row).unwrap();
    let found = repo.get_by_message_id("msg1").unwrap().unwrap();
    assert_eq!(found.id, "em1");
    assert!(repo.get_by_message_id("nope").unwrap().is_none());
}

#[test]
fn ephemeral_media_mark_state() {
    let db = new_db();
    let repo = EphemeralMediaRepo::new(&db);
    let err = repo.mark_state("missing", "burned").unwrap_err();
    assert!(matches!(err, soshal_db_core::error::DbError::NotFound));
    let row = EphemeralMediaRow {
        id: "em1".into(),
        message_id: "msg1".into(),
        conversation_id: "conv1".into(),
        conversation_type: "dm".into(),
        media_url: "blob:abc".into(),
        media_type: "image/jpeg".into(),
        sender_pubkey: "pkA".into(),
        recipient_pubkey: "pkB".into(),
        max_views: 1,
        current_views: 0,
        state: "pending".into(),
        expires_at: None,
        created_at: 1000,
        viewed_at: None,
    };
    repo.create(&row).unwrap();
    repo.mark_state("em1", "burned").unwrap();
    let found = repo.get("em1").unwrap().unwrap();
    assert_eq!(found.state, "burned");
}

fn escrow_row(id: &str, buyer: &str, seller: &str, created_at: i64) -> EscrowRow {
    EscrowRow {
        id: id.into(),
        listing_id: format!("listing_{id}"),
        buyer_pubkey: buyer.into(),
        seller_pubkey: seller.into(),
        amount_msats: 1000,
        currency: "sat".into(),
        status: "created".into(),
        escrow_note: None,
        created_at,
        updated_at: created_at,
    }
}

#[test]
fn escrow_get_by_participant() {
    let db = new_db();
    let repo = EscrowRepo::new(&db);
    repo.create(&escrow_row("e1", "pkBuyer", "pkSeller", 1000))
        .unwrap();
    repo.create(&escrow_row("e2", "pkSeller", "pkBuyer", 2000))
        .unwrap();
    repo.create(&escrow_row("e3", "pkBuyer", "pkBuyer", 3000))
        .unwrap();
    let rows = repo.get_by_participant("pkBuyer").unwrap();
    assert_eq!(
        rows.len(),
        3,
        "buyer branch includes self-trade; seller branch excludes it"
    );
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    assert!(ids.contains(&"e1".to_string()));
    assert!(ids.contains(&"e2".to_string()));
    assert!(ids.contains(&"e3".to_string()));
    assert_eq!(rows[0].id, "e3", "newest first");
}

#[test]
fn escrow_confirms_roundtrip() {
    let db = new_db();
    let repo = EscrowRepo::new(&db);
    let err = repo.get_confirms("missing").unwrap_err();
    assert!(matches!(err, soshal_db_core::error::DbError::NotFound));
    let err = repo.set_buyer_confirmed("missing", true).unwrap_err();
    assert!(matches!(err, soshal_db_core::error::DbError::NotFound));
    let err = repo.set_seller_confirmed("missing", true).unwrap_err();
    assert!(matches!(err, soshal_db_core::error::DbError::NotFound));

    repo.create(&escrow_row("e1", "pkBuyer", "pkSeller", 1000))
        .unwrap();
    let (b, s) = repo.get_confirms("e1").unwrap();
    assert_eq!((b, s), (false, false));
    repo.set_buyer_confirmed("e1", true).unwrap();
    repo.set_seller_confirmed("e1", true).unwrap();
    let (b, s) = repo.get_confirms("e1").unwrap();
    assert_eq!((b, s), (true, true));
    repo.set_buyer_confirmed("e1", false).unwrap();
    let (b, _) = repo.get_confirms("e1").unwrap();
    assert!(!b);
}

#[test]
fn geohash_peer_purge_stale() {
    let db = new_db();
    let repo = GeohashPeerRepo::new(&db);
    let now = soshal_common_core::format::now_secs();
    repo.upsert(&GeohashPeerRow {
        pubkey: "pkOld".into(),
        geohash: "u33d".into(),
        purpose: "meet".into(),
        first_seen: now - 200,
        last_seen: now - 200,
    })
    .unwrap();
    repo.upsert(&GeohashPeerRow {
        pubkey: "pkFresh".into(),
        geohash: "u33d".into(),
        purpose: "meet".into(),
        first_seen: now + 60,
        last_seen: now + 60,
    })
    .unwrap();
    let purged = repo.purge_stale(100).unwrap();
    assert_eq!(purged, 1, "only stale peer deleted");
    let kept = repo.purge_stale(0).unwrap();
    assert_eq!(kept, 0, "fresh peer survives zero cutoff");
    let remaining = repo.list_by_geohash("u33d").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].pubkey, "pkFresh");
}

fn group_row(id: &str, pubkey: &str, updated_at: i64) -> GroupRow {
    GroupRow {
        id: id.into(),
        name: format!("group {id}"),
        about: None,
        picture: None,
        pubkey: pubkey.into(),
        created_at: 1000,
        updated_at,
        access_type: "public".into(),
        relay: None,
        sync_status: "synced".into(),
        password_hash: None,
    }
}

#[test]
fn group_user_groups_and_members() {
    let db = new_db();
    let repo = GroupRepo::new(&db);
    repo.upsert(&group_row("g1", "pkOwner", 1000)).unwrap();
    repo.upsert(&group_row("g2", "pkOwner", 2000)).unwrap();
    insert_test_user(&db, "pkMember");
    insert_test_user(&db, "pkOther");
    repo.add_member("g1", "pkMember", "member", 100).unwrap();
    repo.add_member("g2", "pkMember", "member", 200).unwrap();
    repo.add_member("g2", "pkOther", "member", 300).unwrap();

    let mine = repo.get_user_groups("pkMember").unwrap();
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].id, "g2", "updated_at DESC");
    assert_eq!(mine[1].id, "g1");

    repo.remove_member("g1", "pkMember").unwrap();
    let after = repo.get_user_groups("pkMember").unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, "g2");

    let counts = repo.member_count_many(&["g1".into(), "g2".into()]).unwrap();
    assert_eq!(counts.get("g1"), None, "empty group absent from map");
    assert_eq!(counts.get("g2"), Some(&2));
    let empty = repo.member_count_many(&[]).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn group_shared_key_roundtrip() {
    let db = new_db();
    let repo = GroupRepo::new(&db);
    assert!(repo.get_shared_key("g1").unwrap().is_none());
    repo.set_shared_key("g1", "deadbeef").unwrap();
    assert_eq!(repo.get_shared_key("g1").unwrap().unwrap(), "deadbeef");
    repo.set_shared_key("g1", "cafe").unwrap();
    assert_eq!(
        repo.get_shared_key("g1").unwrap().unwrap(),
        "cafe",
        "replace semantics"
    );
}

#[test]
fn hashtag_get_trending() {
    let db = new_db();
    let repo = HashtagRepo::new(&db);
    for (tag, count) in [("rust", 5), ("nostr", 9), ("soshal", 2)] {
        repo.upsert(&HashtagRow {
            tag: tag.into(),
            pubkey: "pk".into(),
            last_used_at: 1000,
            count,
        })
        .unwrap();
    }
    let trending = repo.get_trending(2).unwrap();
    assert_eq!(trending.len(), 2);
    assert_eq!(trending[0].tag, "nostr");
    assert_eq!(trending[1].tag, "rust");
}

#[test]
fn media_get_user_media_paged() {
    let db = new_db();
    insert_test_user(&db, "pkMedia");
    let repo = MediaRepo::new(&db);
    for i in 0..3 {
        repo.upsert(&MediaRow {
            id: format!("m{i}"),
            pubkey: "pkMedia".into(),
            url: format!("blob:{i}"),
            file_hash: None,
            file_size: None,
            mime_type: Some("image/png".into()),
            created_at: 1000 + i,
            blob_hash: None,
        })
        .unwrap();
    }
    let rows = repo.get_user_media("pkMedia", 2, 0).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "m2");
    let rest = repo.get_user_media("pkMedia", 2, 2).unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].id, "m0");
    assert!(repo.get_user_media("pkNone", 5, 0).unwrap().is_empty());
}

#[test]
fn notification_get_unread_filtered() {
    let db = new_db();
    insert_test_user(&db, "pk");
    insert_test_user(&db, "pkOther");
    let repo = NotificationRepo::new(&db);
    for (i, read) in [(0, false), (1, true), (2, false)] {
        repo.upsert(&NotificationRow {
            id: format!("n{i}"),
            pubkey: "pk".into(),
            type_: "reply".into(),
            event_id: None,
            from_pubkey: None,
            content: None,
            created_at: 1000 + i,
            is_read: read,
        })
        .unwrap();
    }
    let unread = repo.get_unread_filtered("pk", "reply", 10).unwrap();
    assert_eq!(unread.len(), 2);
    assert_eq!(unread[0].id, "n2", "newest first");
    let read = repo.get_unread_filtered("pk", "zap", 10).unwrap();
    assert!(read.is_empty());
    let other = repo.get_unread_filtered("pkOther", "reply", 10).unwrap();
    assert!(other.is_empty());
}

#[test]
fn post_get_by_ids() {
    let db = new_db();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    repo.upsert(&test_post("p1", "pk1", "one", 1000)).unwrap();
    repo.upsert(&test_post("p2", "pk1", "two", 2000)).unwrap();
    let rows = repo
        .get_by_ids(&["p1".into(), "p2".into(), "missing".into()])
        .unwrap();
    assert_eq!(rows.len(), 2, "missing ids absent");
    let rows_mixed = repo.get_by_ids(&["P1".into(), "P2".into()]).unwrap();
    assert_eq!(rows_mixed.len(), 2, "case-insensitive matching for ids");
    assert!(repo.get_by_ids(&[]).unwrap().is_empty());
}

#[test]
fn post_get_replies_for_root() {
    let db = new_db();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    let root = test_post("root", "pk1", "root post", 1000);
    repo.upsert(&root).unwrap();
    let mut reply = test_post("r1", "pk1", "reply", 2000);
    reply.root_id = Some("root".into());
    repo.upsert(&reply).unwrap();
    let mut deleted = test_post("r2", "pk1", "gone", 3000);
    deleted.root_id = Some("root".into());
    deleted.is_deleted = true;
    repo.upsert(&deleted).unwrap();
    let rows = repo.get_replies_for_root("root").unwrap();
    assert_eq!(
        rows.len(),
        2,
        "root itself plus live reply, deleted excluded"
    );
    assert_eq!(rows[0].id, "root", "ascending created_at");
    assert_eq!(rows[1].id, "r1");
}

#[test]
fn post_upsert_in_and_batch() {
    let db = new_db();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    let conn = db.conn().unwrap();
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &test_post("p1", "pk1", "single", 1000))
                .await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    assert_eq!(repo.get_by_id("p1").unwrap().unwrap().content, "single");

    let posts = vec![
        test_post("b1", "pk1", "batch a", 2000),
        test_post("b2", "pk1", "batch b", 3000),
    ];
    let conn = db.conn().unwrap();
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_batch_in(&tx, &posts).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let found = repo.get_by_ids(&["b1".into(), "b2".into()]).unwrap();
    assert_eq!(found.len(), 2);
}

#[test]
fn post_delete_older_than_and_all() {
    let db = new_db();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    repo.upsert(&test_post("old", "pk1", "old", 1000)).unwrap();
    repo.upsert(&test_post("new", "pk1", "new", 2000)).unwrap();
    let deleted = repo.delete_older_than(1500).unwrap();
    assert_eq!(deleted, 1);
    let old = repo.get_by_id("old").unwrap().unwrap();
    assert!(old.is_deleted, "soft delete");
    assert!(!repo.get_by_id("new").unwrap().unwrap().is_deleted);

    let all = repo.delete_all_posts().unwrap();
    assert_eq!(all, 1, "new post soft-deleted");
    let rows = repo.get_paged(10, 0).unwrap();
    assert!(rows.is_empty(), "paged excludes deleted");
}

#[test]
fn post_get_feed() {
    let db = new_db();
    insert_test_user(&db, "pkA");
    insert_test_user(&db, "pkB");
    insert_test_user(&db, "pkC");
    let repo = PostRepo::new(&db);
    repo.upsert(&test_post("a1", "pkA", "from a", 1000))
        .unwrap();
    repo.upsert(&test_post("b1", "pkB", "from b", 2000))
        .unwrap();
    repo.upsert(&test_post("c1", "pkC", "from c", 3000))
        .unwrap();
    let feed = repo.get_feed(&["pkA".into(), "pkB".into()], 10, 0).unwrap();
    let ids: Vec<String> = feed.iter().map(|p| p.id.clone()).collect();
    assert_eq!(ids, vec!["b1".to_string(), "a1".to_string()]);
    assert!(repo.get_feed(&[], 10, 0).unwrap().is_empty());
}

#[test]
fn post_get_recent_paged_meta_scheduled() {
    let db = new_db();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    for i in 0..4 {
        repo.upsert(&test_post(
            &format!("p{i}"),
            "pk1",
            &format!("c{i}"),
            1000 + i,
        ))
        .unwrap();
    }
    let recent = repo.get_recent(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, "p3");
    let paged = repo.get_paged(2, 1).unwrap();
    assert_eq!(paged.len(), 2);
    assert_eq!(paged[0].id, "p2");
    let meta = repo.get_paged_meta(2, 0).unwrap();
    assert_eq!(meta.len(), 2);
    assert_eq!(meta[0].content, "c3");

    let mut sched = test_post("sch1", "pk1", "later", 5000);
    sched.scheduled_at = Some(9000);
    repo.upsert(&sched).unwrap();
    let scheduled = repo.get_scheduled("pk1").unwrap();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(scheduled[0].id, "sch1");
    assert!(repo.get_scheduled("pkOther").unwrap().is_empty());
}

#[test]
fn reaction_upsert_in_conflict_updates_content() {
    let db = new_db();
    insert_test_user(&db, "pk");
    let repo = ReactionRepo::new(&db);
    let row = ReactionRow {
        id: "r1".into(),
        pubkey: "pk".into(),
        event_id: "ev1".into(),
        kind: 7,
        content: Some("+".into()),
        created_at: 1000,
    };
    repo.upsert(&row).unwrap();
    let conn = db.conn().unwrap();
    let replaced = ReactionRow {
        id: row.id,
        pubkey: row.pubkey,
        event_id: row.event_id,
        kind: row.kind,
        content: Some("🔥".into()),
        created_at: row.created_at,
    };
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &replaced).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let rows = repo.get_by_event("ev1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content.as_deref(), Some("🔥"));
}

#[test]
fn relay_upsert_in_conflict_updates_fields() {
    let db = new_db();
    let repo = RelayRepo::new(&db);
    let row = RelayRow {
        url: "wss://relay.example".into(),
        pubkey: Some("pk".into()),
        name: Some("first".into()),
        read_enabled: true,
        write_enabled: true,
        priority: 1,
        last_connected_at: None,
        health_score: 1.0,
    };
    repo.upsert(&row).unwrap();
    let conn = db.conn().unwrap();
    let mut replaced = row;
    replaced.name = Some("second".into());
    replaced.read_enabled = false;
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &replaced).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let found = repo.get_by_url("wss://relay.example").unwrap().unwrap();
    assert_eq!(found.name.as_deref(), Some("second"));
    assert!(!found.read_enabled);
}

#[test]
fn reminder_due_window() {
    let db = new_db();
    let repo = ReminderRepo::new(&db);
    repo.upsert(&ReminderRow {
        id: "rem1".into(),
        event_id: "ev1".into(),
        title: "party".into(),
        start_time: 100_000,
        minutes_before: 60,
        created_at: 1000,
    })
    .unwrap();
    let due_at = 100_000 - 60 * 60;
    let rows = repo.due(due_at, 0).unwrap();
    assert_eq!(rows.len(), 1, "inclusive lower bound");
    assert!(repo.due(due_at + 1, 0).unwrap().is_empty());
    let lookahead = repo.due(due_at, 5_000).unwrap();
    assert_eq!(lookahead.len(), 1, "lookahead covers window");
}

#[test]
fn role_assign_member_role() {
    let db = new_db();
    let repo = GroupRepo::new(&db);
    repo.upsert(&group_row("g1", "pkOwner", 1000)).unwrap();
    insert_test_user(&db, "pkMember");
    insert_test_user(&db, "pkNobody");
    repo.add_member("g1", "pkMember", "member", 100).unwrap();
    let role_repo = GroupRoleRepo::new(&db);
    role_repo
        .upsert(&GroupRoleRow {
            id: "roleAdmin".into(),
            group_id: "g1".into(),
            name: "admin".into(),
            color: "#f00".into(),
            position: 1,
            permissions: "all".into(),
            created_at: 1000,
        })
        .unwrap();
    role_repo
        .assign_member_role("g1", "pkMember", "roleAdmin")
        .unwrap();
    let members = repo.get_members("g1").unwrap();
    assert_eq!(members[0].role, "roleAdmin");
    role_repo
        .assign_member_role("g1", "pkNobody", "roleAdmin")
        .unwrap();
    let members = repo.get_members("g1").unwrap();
    assert_eq!(members.len(), 1, "no row created for unknown member");
}

#[test]
fn build_fts_query_formats_terms() {
    let q = build_fts_query("hello world");
    assert!(q.contains("hello"), "got {q}");
    assert!(q.contains("world"), "got {q}");
    assert!(q.contains(" OR "), "got {q}");
    let dirty = build_fts_query("alice!!! bob@example");
    assert!(dirty.contains("alice"), "got {dirty}");
    assert!(dirty.contains("bob"), "got {dirty}");
    assert!(!dirty.contains("!!!"), "got {dirty}");
    let empty = build_fts_query("");
    assert!(empty.is_empty(), "got {empty}");
    let garbage = build_fts_query("!!! ???");
    assert!(garbage.is_empty(), "got {garbage}");
    let long = build_fts_query(&"word ".repeat(100));
    assert!(long.contains(" OR "));
}

#[test]
fn settings_get_many() {
    let db = new_db();
    let repo = SettingsRepo::new(&db);
    repo.set("theme", "dark").unwrap();
    repo.set("language", "en").unwrap();
    repo.set("volume", "11").unwrap();
    let map = repo.get_many(&["theme", "language", "missing"]).unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map.get("theme").unwrap(), "dark");
    assert_eq!(map.get("language").unwrap(), "en");
    assert!(repo.get_many(&[]).unwrap().is_empty());
}

#[test]
fn user_upsert_in_and_get_by_pubkey_in() {
    let db = new_db();
    let repo = UserRepo::new(&db);
    let user = UserRow {
        pubkey: "pkTx".into(),
        npub: "npubTx".into(),
        name: Some("tx".into()),
        display_name: None,
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1000,
        updated_at: 1000,
        metadata_json: None,
        contact_pubkeys: r#"["a","b","c"]"#.into(),
        relay_list: "[]".into(),
        follower_count: 0,
    };
    let conn = db.conn().unwrap();

    // Insert follower rows first so the INSERT subquery above counts them.
    soshal_db_core::query::execute(
        &conn,
        "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('pkj', '', '[\"pkTx\",\"other\"]')",
        (),
    )
    .unwrap();
    soshal_db_core::query::execute(
        &conn,
        "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('pkc', '', 'pkTx,unrelated')",
        (),
    )
    .unwrap();
    soshal_db_core::query::execute(
        &conn,
        "INSERT INTO users (pubkey, npub, contact_pubkeys) VALUES ('pkn', '', '[\"other\"]')",
        (),
    )
    .unwrap();

    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &user).await?;
            let found = r.get_by_pubkey_in(&tx, "pkTx").await?.unwrap();
            assert_eq!(found.name.as_deref(), Some("tx"));
            // second upsert exercises the ON CONFLICT update branch
            r.upsert_in(&tx, &user).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();

    // Follower counts are materialized incrementally (upsert no longer runs a
    // correlated recount). Recompute from the seeded contact lists above.
    drop(conn);
    repo.recompute_all_follower_counts().unwrap();

    let follower_count: i64 = {
        let conn = db.conn().unwrap();
        soshal_db_core::query::query_first(
            &conn,
            "SELECT follower_count FROM users WHERE pubkey = 'pkTx'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(follower_count, 2, "counts other users following pkTx");
}

#[test]
fn user_bump_follower_count_and_upsert_batch() {
    let db = new_db();
    let repo = UserRepo::new(&db);

    {
        let conn = db.conn().unwrap();
        soshal_db_core::query::execute(
            &conn,
            "INSERT INTO users (pubkey, npub) VALUES ('alice', ''), ('bob', '')",
            (),
        )
        .unwrap();
    }

    // Upsert must NOT clobber follower_count (it is not in the ON CONFLICT
    // update clause anymore).
    let alice = UserRow {
        pubkey: "alice".into(),
        npub: "npubA".into(),
        name: Some("Alice".into()),
        display_name: None,
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1000,
        updated_at: 1000,
        metadata_json: None,
        contact_pubkeys: "[]".into(),
        relay_list: "[]".into(),
        follower_count: 0,
    };
    repo.upsert(&alice).unwrap();
    repo.bump_follower_count("alice", 3).unwrap();
    let n: i64 = {
        let conn = db.conn().unwrap();
        soshal_db_core::query::query_first(
            &conn,
            "SELECT follower_count FROM users WHERE pubkey = 'alice'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(n, 3, "bump increments the materialized count");
    repo.upsert(&alice).unwrap();
    let n: i64 = {
        let conn = db.conn().unwrap();
        soshal_db_core::query::query_first(
            &conn,
            "SELECT follower_count FROM users WHERE pubkey = 'alice'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(n, 3, "upsert must preserve the materialized follower count");
    repo.bump_follower_count("alice", -1).unwrap();
    repo.bump_follower_count("alice", -5).unwrap();
    let n: i64 = {
        let conn = db.conn().unwrap();
        soshal_db_core::query::query_first(
            &conn,
            "SELECT follower_count FROM users WHERE pubkey = 'alice'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(n, 0, "count never goes negative");

    // recompute for one follower: bob lists alice (+ self ref ignored).
    {
        let conn = db.conn().unwrap();
        soshal_db_core::query::execute(
            &conn,
            "UPDATE users SET contact_pubkeys = '[\"alice\"]' WHERE pubkey = 'bob'",
            (),
        )
        .unwrap();
    }
    repo.recompute_all_follower_counts().unwrap();
    let n: i64 = {
        let conn = db.conn().unwrap();
        soshal_db_core::query::query_first(
            &conn,
            "SELECT follower_count FROM users WHERE pubkey = 'alice'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(n, 1, "recompute reflects bob listing alice");
}

#[test]
fn zap_upsert_in_derives_msats() {
    let db = new_db();
    let repo = ZapRepo::new(&db);
    let row = ZapRow {
        id: "z1".into(),
        pubkey: "pkSender".into(),
        recipient_pubkey: "pkRecv".into(),
        event_id: Some("ev1".into()),
        amount: 21,
        amount_msat: 21_000,
        content: Some("thanks".into()),
        created_at: 1000,
        zap_type: "public".into(),
    };
    let conn = db.conn().unwrap();
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &row).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let msats: i64 = soshal_db_core::query::query_first(
        &db.conn().unwrap(),
        "SELECT amount_msat FROM zaps WHERE id = 'z1'",
        (),
        |r| r.get(0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(msats, 21_000, "amount * 1000");

    // Re-upsert same id with a new amount: amount_msat must be refreshed,
    // not left stale from the original insert.
    let bumped = ZapRow {
        id: "z1".into(),
        pubkey: "pkSender".into(),
        recipient_pubkey: "pkRecv".into(),
        event_id: Some("ev1".into()),
        amount: 42,
        amount_msat: 42_000,
        content: Some("thanks".into()),
        created_at: 1000,
        zap_type: "public".into(),
    };
    let conn = db.conn().unwrap();
    soshal_db_core::query::with_tx(&conn, |tx| {
        let r = &repo;
        async move {
            r.upsert_in(&tx, &bumped).await?;
            tx.commit().await?;
            Ok(())
        }
    })
    .unwrap();
    drop(conn);
    let msats: i64 = soshal_db_core::query::query_first(
        &db.conn().unwrap(),
        "SELECT amount_msat FROM zaps WHERE id = 'z1'",
        (),
        |r| r.get(0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(msats, 42_000, "amount_msat refreshed on conflict");
}
