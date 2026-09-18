use soshal_db_core::repos::audit_log::{AuditLogRepo, AuditLogRow};
use soshal_db_core::repos::banned_member::{BannedMemberRepo, BannedMemberRow};
use soshal_db_core::repos::block::{BlockRepo, BlockRow};
use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::conversation_mute::ConversationMuteRepo;
use soshal_db_core::repos::dating_unmatch::DatingUnmatchRepo;
use soshal_db_core::repos::diagnostic_log::{DiagnosticLogRepo, DiagnosticLogRow};
use soshal_db_core::repos::ephemeral_media::{EphemeralMediaRepo, EphemeralMediaRow};
use soshal_db_core::repos::escrow::{EscrowRepo, EscrowRow};
use soshal_db_core::repos::friend_backup::{FriendBackupRepo, FriendBackupRow};
use soshal_db_core::repos::geohash_peer::{GeohashPeerRepo, GeohashPeerRow};
use soshal_db_core::repos::group::{GroupRepo, GroupRow};
use soshal_db_core::repos::group_invite::{GroupInviteRepo, GroupInviteRow};
use soshal_db_core::repos::group_join_request::{GroupJoinRequestRepo, GroupJoinRequestRow};
use soshal_db_core::repos::guestbook::{GuestbookEntryRow, GuestbookRepo};
use soshal_db_core::repos::hashtag::{HashtagRepo, HashtagRow};
use soshal_db_core::repos::huddle_post::{HuddlePostRepo, HuddlePostRow};
use soshal_db_core::repos::limits::{self, notification_too_big, row_too_big};
use soshal_db_core::repos::link_preview::{LinkPreviewRepo, LinkPreviewRow};
use soshal_db_core::repos::marketplace_review::{MarketplaceReviewRepo, MarketplaceReviewRow};
use soshal_db_core::repos::media::{MediaRepo, MediaRow};
use soshal_db_core::repos::message::{MessageRepo, MessageRow};
use soshal_db_core::repos::musicloud::{
    MusicloudCommentRepo, MusicloudCommentRow, MusicloudRepo, MusicloudRow,
};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::poll::{PollRepo, PollRow, PollVoteRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::post_views::PostViewsRepo;
use soshal_db_core::repos::profile_node::{ProfileNodeRepo, ProfileNodeRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::refetch_item::{RefetchItemRepo, RefetchItemRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::reminder::{ReminderRepo, ReminderRow};
use soshal_db_core::repos::repost::{RepostRepo, RepostRow};
use soshal_db_core::repos::role::{GroupRoleRepo, GroupRoleRow};
use soshal_db_core::repos::saved::{MusicloudPlaylistRepo, SavedContentRepo, SavedContentRow};
use soshal_db_core::repos::search_index::{SearchIndexRepo, SearchIndexRow};
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::spam_report::{SpamReportRepo, SpamReportRow};
use soshal_db_core::repos::story_reaction::{StoryReactionRepo, StoryReactionRow};
use soshal_db_core::repos::stream_chat::{StreamChatRepo, StreamChatRow};
use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_db_core::repos::zap::{ZapRepo, ZapRow};
use soshal_db_core::schema::SCHEMA_VERSION;
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

#[test]
fn open_and_migrate() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
}

#[test]
fn test_schema_migration_flow() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let conn = db.conn().unwrap();

    let version: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(version, SCHEMA_VERSION);

    let tables: Vec<String> = soshal_db_core::query::query(
        &conn,
        "SELECT name FROM sqlite_master WHERE type='table'",
        (),
        |row| row.get(0),
    )
    .unwrap();

    assert!(tables.contains(&"users".to_string()));
    assert!(tables.contains(&"posts".to_string()));
    assert!(tables.contains(&"messages".to_string()));
    assert!(tables.contains(&"reminders".to_string()));
    assert!(tables.contains(&"group_roles".to_string()));
    assert!(tables.contains(&"audit_logs".to_string()));
    assert!(tables.contains(&"post_views".to_string()));

    let fts: Vec<String> = soshal_db_core::query::query(
        &conn,
        "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='posts_ai'",
        (),
        |row| row.get(0),
    )
    .unwrap();
    assert_eq!(fts.len(), 1, "posts_ai trigger must exist (v7)");
    assert!(
        fts[0].contains("INSERT OR REPLACE"),
        "v7 trigger must use INSERT OR REPLACE: {}",
        fts[0]
    );
}

#[test]
fn test_v7_fts_trigger_replaces_and_deletes() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pkFts");
    let conn = db.conn().unwrap();

    soshal_db_core::block_on(conn.execute(
        "INSERT INTO posts (id, pubkey, content, created_at) VALUES ('ev1', 'pkFts', 'hello world', 1)",
        (),
    ))
    .unwrap();
    let rows: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COUNT(*) FROM posts_fts WHERE rowid > 0",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(rows, 1, "insert must populate posts_fts via trigger");

    soshal_db_core::block_on(conn.execute(
        "UPDATE posts SET content = 'replaced text' WHERE id = 'ev1'",
        (),
    ))
    .unwrap();
    let text: String = soshal_db_core::query::query_first(
        &conn,
        "SELECT content FROM posts_fts WHERE rowid > 0",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or_default();
    assert_eq!(text, "replaced text", "update must replace FTS row (v7)");

    soshal_db_core::block_on(conn.execute("DELETE FROM posts WHERE id = 'ev1'", ())).unwrap();
    let rows: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COUNT(*) FROM posts_fts WHERE rowid > 0",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(rows, 0, "delete must remove FTS row via trigger");
}

#[test]
fn test_v6_purges_orphan_fts_rows() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO posts_fts(rowid, id, pubkey, content) VALUES (999, 'orphan', 'pk', 'stale')",
            (),
        ))
        .unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO posts_fts(rowid, id, pubkey, content) VALUES (1, 'kept', 'pk', 'live')",
            (),
        ))
        .unwrap();
    }
    insert_test_user(&db, "pkLive");
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO posts (rowid, id, pubkey, content, created_at) VALUES (1, 'live1', 'pkLive', 'kept', 1)",
            (),
        ))
        .unwrap();
    }

    soshal_db_core::block_on(db.conn().unwrap().execute_batch(
        "DELETE FROM posts_fts
                 WHERE rowid > 0
                   AND rowid NOT IN (SELECT rowid FROM posts);",
    ))
    .unwrap();

    let conn = db.conn().unwrap();
    let orphan: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COUNT(*) FROM posts_fts WHERE rowid = 999",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(orphan, 0, "positive rowid without posts row must be purged");
    let kept: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COUNT(*) FROM posts_fts WHERE rowid = 1",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(kept, 1, "posts-backed row must survive");
}

#[test]
fn test_schema_migration_idempotent() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db.migrate().unwrap();
    let conn = db.conn().unwrap();

    let version: i64 = soshal_db_core::query::query_first(
        &conn,
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        (),
        |row| row.get(0),
    )
    .unwrap()
    .unwrap_or(0);
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn insert_and_read_post() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    let post = PostRow {
        id: "test1".into(),
        pubkey: "pk1".into(),
        content: "hello".into(),
        kind: 1,
        created_at: 1000,
        tags_json: "[]".into(),
        sig: Some("sig1".into()),
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: "[]".into(),
        mentioned_hashtags: "[]".into(),
        subject: None,
        sync_status: "synced".into(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: true,
        rsvp_event_id: None,
    };
    repo.upsert(&post).unwrap();
    let found = repo.get_by_id("test1").unwrap().unwrap();
    assert_eq!(found.content, "hello");
}

#[test]
fn insert_and_read_user() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = UserRepo::new(&db);
    let user = UserRow {
        pubkey: "pk1".into(),
        npub: "npub1".into(),
        name: Some("alice".into()),
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
    let found = repo.get_by_pubkey("pk1").unwrap().unwrap();
    assert_eq!(found.name.unwrap(), "alice");
}

#[test]
fn test_user_ensure_exists_and_search() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = UserRepo::new(&db);

    repo.ensure_exists("pk_test_100").unwrap();
    let user = repo
        .get_by_pubkey("pk_test_100")
        .unwrap()
        .expect("user exists");
    assert_eq!(user.pubkey, "pk_test_100");

    let updated_user = UserRow {
        pubkey: "pk_test_100".into(),
        npub: "npub_test_100".into(),
        name: Some("Alice%Bob".into()),
        display_name: Some("Alice Special".into()),
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1000,
        updated_at: 2000,
        metadata_json: None,
        contact_pubkeys: "[]".into(),
        relay_list: "[]".into(),
        follower_count: 0,
    };
    repo.upsert(&updated_user).unwrap();

    let results = repo.search("Alice%", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name.as_deref(), Some("Alice%Bob"));
}

#[test]
fn test_user_existing_pubkeys_case_insensitive() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = UserRepo::new(&db);

    repo.ensure_exists("pk_user_abc").unwrap();
    repo.ensure_exists("PK_USER_XYZ").unwrap();

    // Single pubkey check
    let single_lower = repo.existing_pubkeys(&["pk_user_abc".to_string()]).unwrap();
    assert_eq!(single_lower.len(), 1);
    let single_upper = repo.existing_pubkeys(&["PK_USER_ABC".to_string()]).unwrap();
    assert_eq!(single_upper.len(), 1);

    // Multi pubkey check
    let multi = repo
        .existing_pubkeys(&[
            "PK_USER_ABC".to_string(),
            "pk_user_xyz".to_string(),
            "nonexistent".to_string(),
        ])
        .unwrap();
    assert_eq!(multi.len(), 2);
}

#[test]
fn test_message_repo_upsert_and_get_conversation() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "sender1");
    let repo = MessageRepo::new(&db);
    let msg = MessageRow {
        id: "msg1".into(),
        conversation_id: "conv1".into(),
        pubkey: "sender1".into(),
        content: "hello world".into(),
        created_at: 1000,
        tags_json: "[]".into(),
        reply_to: None,
        sync_status: "synced".into(),
        is_deleted: false,
    };
    repo.upsert(&msg).unwrap();
    let found = repo.get_by_id("msg1").unwrap().expect("find message");
    assert_eq!(found.content, "hello world");

    let conv = repo.get_conversation("conv1", 10, None).unwrap();
    assert_eq!(conv.len(), 1);
    assert_eq!(conv[0].id, "msg1");
}

#[test]
fn test_notification_repo_upsert_and_get_unread() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "target_user");
    let repo = NotificationRepo::new(&db);
    let notif = NotificationRow {
        id: "n1".into(),
        pubkey: "target_user".into(),
        type_: "mention".into(),
        event_id: Some("e1".into()),
        from_pubkey: Some("sender1".into()),
        content: Some("Hey!".into()),
        created_at: 2000,
        is_read: false,
    };
    repo.upsert(&notif).unwrap();
    let unread = repo.get_unread("target_user", 10).unwrap();
    assert_eq!(unread.len(), 1);
    assert_eq!(unread[0].id, "n1");

    let unread_upper = repo.get_unread("TARGET_USER", 10).unwrap();
    assert_eq!(unread_upper.len(), 1);

    let ign_repo = soshal_db_core::repos::ignored_notification::IgnoredNotificationRepo::new(&db);
    ign_repo
        .ignore_user("TARGET_USER", "SENDER1", "mention", 2005)
        .unwrap();
    assert!(ign_repo
        .is_ignored("target_user", "mention", "sender1", "")
        .unwrap());
    assert!(ign_repo
        .is_ignored("TARGET_USER", "mention", "SENDER1", "")
        .unwrap());

    let unread_ignored = repo.get_unread("target_user", 10).unwrap();
    assert_eq!(unread_ignored.len(), 0);
}

#[test]
fn test_upsert_batch() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);
    let posts = vec![
        PostRow {
            id: "p1".into(),
            pubkey: "pk1".into(),
            content: "post 1".into(),
            kind: 1,
            created_at: 100,
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
            is_freenet_native: true,
            rsvp_event_id: None,
        },
        PostRow {
            id: "p2".into(),
            pubkey: "pk1".into(),
            content: "post 2".into(),
            kind: 1,
            created_at: 200,
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
            is_freenet_native: true,
            rsvp_event_id: None,
        },
    ];
    repo.upsert_batch(&posts).unwrap();
    let found = repo.get_user_posts("pk1", 10, 0).unwrap();
    assert_eq!(found.len(), 2);
}

#[test]
fn test_audit_log_insert_and_queries() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = AuditLogRepo::new(&db);

    let row1 = AuditLogRow {
        id: "al1".into(),
        group_id: "grp1".into(),
        actor_pubkey: "actor1".into(),
        action: "ban".into(),
        target_pubkey: Some("target1".into()),
        details: Some("violated terms".into()),
        created_at: 1000,
    };
    let row2 = AuditLogRow {
        id: "al2".into(),
        group_id: "grp1".into(),
        actor_pubkey: "actor2".into(),
        action: "promote".into(),
        target_pubkey: None,
        details: None,
        created_at: 2000,
    };

    repo.insert(&row1).unwrap();
    repo.insert(&row2).unwrap();

    let actor1_logs = repo.get_by_actor("actor1", 10).unwrap();
    assert_eq!(actor1_logs.len(), 1);
    assert_eq!(actor1_logs[0].id, "al1");

    let all_logs = repo.get_all(10).unwrap();
    assert_eq!(all_logs.len(), 2);
}

#[test]
fn test_block_upsert_check_delete_list() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "blocker_1");
    insert_test_user(&db, "blockee_1");
    insert_test_user(&db, "blockee_2");

    let repo = BlockRepo::new(&db);

    let row = BlockRow {
        pubkey: "blocker_1".into(),
        blocked_pubkey: "blockee_1".into(),
        created_at: 1000,
    };
    repo.upsert(&row).unwrap();
    assert!(repo.is_blocked("blocker_1", "blockee_1").unwrap());

    let list = repo.list("blocker_1").unwrap();
    assert_eq!(list.len(), 1);

    repo.delete("blocker_1", "blockee_1").unwrap();
    assert!(!repo.is_blocked("blocker_1", "blockee_1").unwrap());
}

#[test]
fn test_bookmark_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let repo = BookmarkRepo::new(&db);
    let row = BookmarkRow {
        id: "bm1".into(),
        pubkey: "pk1".into(),
        event_id: "evt1".into(),
        created_at: 100,
    };

    repo.upsert(&row).unwrap();
    let fetched = repo.get_by_id("bm1").unwrap().unwrap();
    assert_eq!(fetched.pubkey, "pk1");

    repo.delete("bm1").unwrap();
    assert!(repo.get_by_id("bm1").unwrap().is_none());
}

#[test]
fn test_group_crud_and_membership() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "user_mem_1");
    insert_test_user(&db, "user_mem_2");

    let repo = GroupRepo::new(&db);
    let group = GroupRow {
        id: "grp_100".into(),
        name: "Nostr Developers".into(),
        about: Some("Dev chat".into()),
        picture: None,
        pubkey: "admin_pk".into(),
        created_at: 1000,
        updated_at: 1000,
        access_type: "open".into(),
        relay: Some("wss://relay.example.com".into()),
        sync_status: "synced".into(),
        password_hash: None,
    };

    repo.upsert(&group).unwrap();
    let found = repo.get_by_id("grp_100").unwrap().unwrap();
    assert_eq!(found.name, "Nostr Developers");

    repo.add_member("grp_100", "user_mem_1", "admin", 1000)
        .unwrap();
    let members = repo.get_members("grp_100").unwrap();
    assert_eq!(members.len(), 1);
}

#[test]
fn test_group_private_password_hash() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GroupRepo::new(&db);
    let group = GroupRow {
        id: "grp_private".into(),
        name: "Private VIP Lounge".into(),
        about: Some("Secret talk".into()),
        picture: None,
        pubkey: "vip_owner".into(),
        created_at: 1000,
        updated_at: 1000,
        access_type: "private".into(),
        relay: None,
        sync_status: "synced".into(),
        password_hash: Some("11223344:aabbccdd".into()),
    };

    repo.upsert(&group).unwrap();
    let found = repo.get_by_id("grp_private").unwrap().unwrap();
    assert_eq!(found.access_type, "private");
    assert_eq!(found.password_hash.as_deref(), Some("11223344:aabbccdd"));
}

#[test]
fn test_hashtag_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = HashtagRepo::new(&db);

    let row = HashtagRow {
        tag: "nostr".into(),
        pubkey: "pk1".into(),
        last_used_at: 100,
        count: 5,
    };

    repo.upsert(&row).unwrap();
    let fetched = repo.get_by_tag("nostr", "pk1").unwrap().unwrap();
    assert_eq!(fetched.count, 5);
}

#[test]
fn test_media_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let repo = MediaRepo::new(&db);
    let row = MediaRow {
        id: "m1".into(),
        pubkey: "pk1".into(),
        url: "https://example.com/img.jpg".into(),
        file_hash: Some("abc".into()),
        file_size: Some(1024),
        mime_type: Some("image/jpeg".into()),
        created_at: 500,
        blob_hash: None,
    };

    repo.upsert(&row).unwrap();
    let fetched = repo.get_by_id("m1").unwrap().unwrap();
    assert_eq!(fetched.url, "https://example.com/img.jpg");
}

#[test]
fn test_post_views_mark_and_retrieve() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = PostViewsRepo::new(&db);

    repo.mark_seen("pk1", &["p1".to_string(), "p2".to_string()])
        .unwrap();
    let seen = repo.seen_ids("pk1").unwrap();
    assert_eq!(seen.len(), 2);
}

#[test]
fn test_reaction_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let repo = ReactionRepo::new(&db);
    let row = ReactionRow {
        id: "r1".into(),
        pubkey: "pk1".into(),
        event_id: "evt1".into(),
        kind: 7,
        content: Some("🤙".into()),
        created_at: 100,
    };

    repo.upsert(&row).unwrap();
    let reactions = repo.get_by_event("evt1").unwrap();
    assert_eq!(reactions.len(), 1);
}

#[test]
fn test_relay_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = RelayRepo::new(&db);

    let row = RelayRow {
        url: "wss://relay.damus.io".into(),
        pubkey: None,
        name: Some("Damus".into()),
        read_enabled: true,
        write_enabled: true,
        priority: 1,
        last_connected_at: Some(100),
        health_score: 1.0,
    };

    repo.upsert(&row).unwrap();
    let fetched = repo.get_by_url("wss://relay.damus.io").unwrap().unwrap();
    assert_eq!(fetched.name.as_deref(), Some("Damus"));
}

#[test]
fn test_reminder_crud_and_due_filtering() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = ReminderRepo::new(&db);

    let r1 = ReminderRow {
        id: "rem_1".into(),
        event_id: "evt_1".into(),
        title: "Team Sync".into(),
        start_time: 900,
        minutes_before: 10,
        created_at: 0,
    };

    repo.upsert(&r1).unwrap();
    let list = repo.list().unwrap();
    assert_eq!(list.len(), 1);

    assert_eq!(repo.due(300, 60_000).unwrap().len(), 1);
    assert!(repo.due(400, 1_000).unwrap().is_empty());
}

#[test]
fn test_repost_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let repo = RepostRepo::new(&db);
    let row = RepostRow {
        id: "rp1".into(),
        pubkey: "pk1".into(),
        event_id: "evt1".into(),
        created_at: 100,
    };

    repo.upsert(&row).unwrap();
    assert_eq!(repo.count_by_event("evt1").unwrap(), 1);
}

#[test]
fn test_group_role_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::query::execute(
            &conn,
            "INSERT INTO groups (id, name) VALUES ('grp1', 'Test Group')",
            (),
        )
        .unwrap();
    }
    let repo = GroupRoleRepo::new(&db);

    let role = GroupRoleRow {
        id: "role1".into(),
        group_id: "grp1".into(),
        name: "Moderator".into(),
        color: "#ff0000".into(),
        position: 1,
        permissions: "[\"kick\", \"ban\"]".into(),
        created_at: 100,
    };

    repo.upsert(&role).unwrap();
    let roles = repo.list("grp1").unwrap();
    assert_eq!(roles.len(), 1);
}

#[test]
fn test_search_index_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let post_repo = PostRepo::new(&db);
    let fts_repo = SearchIndexRepo::new(&db);

    let post = PostRow {
        id: "p1".into(),
        pubkey: "pk1".into(),
        content: "Hello Nostr world".into(),
        kind: 1,
        created_at: 100,
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
        is_freenet_native: true,
        rsvp_event_id: None,
    };
    post_repo.upsert(&post).unwrap();

    let results = fts_repo.search("Nostr", 10, 0).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_index_no_fts_rowid_collision() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");

    let post_repo = PostRepo::new(&db);
    let fts_repo = SearchIndexRepo::new(&db);

    let post = |id: &str, content: &str| PostRow {
        id: id.into(),
        pubkey: "pk1".into(),
        content: content.into(),
        kind: 1,
        created_at: 100,
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
        is_freenet_native: true,
        rsvp_event_id: None,
    };

    post_repo.upsert(&post("pa", "first post")).unwrap();
    fts_repo
        .upsert(&SearchIndexRow {
            id: "pa".into(),
            pubkey: "pk1".into(),
            content: "first post".into(),
            kind: 1,
            created_at: 100,
            ..SearchIndexRow::default()
        })
        .unwrap();
    post_repo.upsert(&post("pb", "second post")).unwrap();
    assert_eq!(fts_repo.search("second", 10, 0).unwrap().len(), 1);

    fts_repo.delete("pa").unwrap();
    assert_eq!(fts_repo.search("first", 10, 0).unwrap().len(), 0);

    fts_repo
        .upsert(&SearchIndexRow {
            id: "profile:pk1".into(),
            pubkey: "pk1".into(),
            content: "carol the profile".into(),
            kind: 0,
            created_at: 100,
            ..SearchIndexRow::default()
        })
        .unwrap();
    fts_repo.delete("profile:pk1").unwrap();
}

#[test]
fn test_settings_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = SettingsRepo::new(&db);

    repo.set("theme", "dark").unwrap();
    assert_eq!(repo.get("theme").unwrap().as_deref(), Some("dark"));
}

#[test]
fn test_zap_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = ZapRepo::new(&db);

    let row1 = ZapRow {
        id: "z1".into(),
        pubkey: "pk1".into(),
        recipient_pubkey: "recipient_pk1".into(),
        event_id: Some("evt1".into()),
        amount: 1000,
        amount_msat: 1_000_000,
        content: Some("Great post!".into()),
        created_at: 100,
        zap_type: "public".into(),
    };
    repo.upsert(&row1).unwrap();
    assert_eq!(repo.sum_by_event("evt1").unwrap(), 1000);

    let row1_updated = ZapRow {
        id: "z1".into(),
        pubkey: "pk1".into(),
        recipient_pubkey: "recipient_pk1".into(),
        event_id: Some("evt1".into()),
        amount: 2500,
        amount_msat: 2_500_000,
        content: Some("Great post!".into()),
        created_at: 100,
        zap_type: "public".into(),
    };
    repo.upsert(&row1_updated).unwrap();
    assert_eq!(
        repo.sum_by_event("evt1").unwrap(),
        2500,
        "upsert same id updates amount, no duplicate rows"
    );

    let row2 = ZapRow {
        id: "z2".into(),
        pubkey: "pk2".into(),
        recipient_pubkey: "recipient_pk2".into(),
        event_id: Some("evt2".into()),
        amount: 777,
        amount_msat: 777_000,
        content: None,
        created_at: 200,
        zap_type: "private".into(),
    };
    repo.upsert(&row2).unwrap();
    assert_eq!(
        repo.sum_by_event("evt1").unwrap(),
        2500,
        "per-event isolation"
    );
    assert_eq!(repo.sum_by_event("evt2").unwrap(), 777);
}

#[test]
fn test_settings_delete_and_db_clone() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = SettingsRepo::new(&db);

    repo.set("theme", "dark").unwrap();
    assert_eq!(repo.get("theme").unwrap().as_deref(), Some("dark"));
    repo.delete("theme").unwrap();
    assert!(repo.get("theme").unwrap().is_none());

    let db2 = db.clone();
    let conn2 = db2.conn().unwrap();
    let val: i32 = soshal_db_core::query::query_first(&conn2, "SELECT 1", (), |r| r.get(0))
        .unwrap()
        .unwrap();
    assert_eq!(val, 1);
}

#[test]
fn test_escrow_create_update_list() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = EscrowRepo::new(&db);

    let row = EscrowRow {
        id: "esc1".into(),
        listing_id: "lis1".into(),
        buyer_pubkey: "buyer1".into(),
        seller_pubkey: "seller1".into(),
        amount_msats: 5000,
        currency: "sats".into(),
        status: "created".into(),
        escrow_note: None,
        created_at: 1000,
        updated_at: 1000,
    };
    repo.create(&row).unwrap();
    let found = repo.get("esc1").unwrap().unwrap();
    assert_eq!(found.amount_msats, 5000);
    assert_eq!(repo.get_by_listing("lis1").unwrap().len(), 1);

    repo.update_status("esc1", "funded").unwrap();
    assert_eq!(repo.get("esc1").unwrap().unwrap().status, "funded");
    repo.set_note("esc1", "dispute note").unwrap();
    assert_eq!(
        repo.get("esc1").unwrap().unwrap().escrow_note.as_deref(),
        Some("dispute note")
    );
}

#[test]
fn test_ephemeral_media_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = EphemeralMediaRepo::new(&db);

    let row = EphemeralMediaRow {
        id: "em1".into(),
        message_id: "msg1".into(),
        conversation_id: "conv1".into(),
        conversation_type: "dm".into(),
        media_url: "https://example.com/x.jpg".into(),
        media_type: "image".into(),
        sender_pubkey: "sender1".into(),
        recipient_pubkey: "recipient1".into(),
        max_views: 2,
        current_views: 0,
        state: "pending".into(),
        expires_at: Some(2000),
        created_at: 1000,
        viewed_at: None,
    };
    repo.create(&row).unwrap();
    let found = repo.get("em1").unwrap().unwrap();
    assert_eq!(found.recipient_pubkey, "recipient1");
    assert_eq!(
        repo.get_pending_for_recipient("recipient1").unwrap().len(),
        1
    );

    repo.increment_view_count("em1").unwrap();
    repo.increment_view_count("em1").unwrap();
    let expired = repo.get("em1").unwrap().unwrap();
    assert_eq!(expired.state, "expired");
    assert_eq!(expired.current_views, 2);

    let gone = repo.clean_expired(3000).unwrap();
    assert_eq!(gone, vec!["em1".to_string()]);
    assert!(repo.get("em1").unwrap().is_none());
}

#[test]
fn test_marketplace_review_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = MarketplaceReviewRepo::new(&db);

    let r1 = MarketplaceReviewRow {
        id: "rev1".into(),
        listing_id: "lis1".into(),
        reviewer_pubkey: "buyer1".into(),
        rating: 5,
        text: "great".into(),
        created_at: 1000,
    };
    let r2 = MarketplaceReviewRow {
        id: "rev2".into(),
        listing_id: "lis1".into(),
        reviewer_pubkey: "buyer2".into(),
        rating: 3,
        text: "ok".into(),
        created_at: 2000,
    };
    assert_eq!(repo.average_for("lis1").unwrap(), None);
    repo.insert(&r1).unwrap();
    repo.insert(&r2).unwrap();
    let list = repo.list_by_listing("lis1", 10).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(repo.average_for("lis1").unwrap().unwrap(), 4.0);
    assert_eq!(repo.average_for("nonexistent").unwrap(), None);
}

#[test]
fn test_poll_crud_and_votes() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = PollRepo::new(&db);

    let poll = PollRow {
        id: "poll1".into(),
        pubkey: "pk1".into(),
        question: "Best option?".into(),
        options: "[\"a\",\"b\"]".into(),
        expires_at: 2000,
        closed: false,
        created_at: 1000,
    };
    repo.upsert_poll(&poll).unwrap();
    let found = repo.get_poll("poll1").unwrap().unwrap();
    assert_eq!(found.options, "[\"a\",\"b\"]");
    assert_eq!(repo.list_by_author("pk1", 10).unwrap().len(), 1);
    assert_eq!(repo.list_by_author("PK1", 10).unwrap().len(), 1);

    let vote = PollVoteRow {
        id: "v1".into(),
        poll_id: "poll1".into(),
        option_id: 1,
        voter_pubkey: "voter1".into(),
        voted_at: 1500,
    };
    repo.vote(&vote).unwrap();
    assert!(repo.has_voted("poll1", "voter1").unwrap());
    assert!(repo.has_voted("poll1", "VOTER1").unwrap());
    assert_eq!(repo.option_count("poll1", 1).unwrap(), 1);

    repo.set_closed("poll1", true).unwrap();
    assert!(repo.get_poll("poll1").unwrap().unwrap().closed);
}

#[test]
fn test_poll_revote_new_id_updates_option() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = PollRepo::new(&db);

    let poll = PollRow {
        id: "poll2".into(),
        pubkey: "pk1".into(),
        question: "Pick one?".into(),
        options: "[\"a\",\"b\"]".into(),
        expires_at: 2000,
        closed: false,
        created_at: 1000,
    };
    repo.upsert_poll(&poll).unwrap();

    let first = PollVoteRow {
        id: "v1".into(),
        poll_id: "poll2".into(),
        option_id: 1,
        voter_pubkey: "voter1".into(),
        voted_at: 1500,
    };
    repo.vote(&first).unwrap();
    assert!(repo.has_voted("poll2", "voter1").unwrap());
    assert_eq!(repo.option_count("poll2", 1).unwrap(), 1);

    // Re-vote with a DIFFERENT id + new option: must not error (unique
    // (poll_id, voter_pubkey) conflict) and must switch the stored option.
    let second = PollVoteRow {
        id: "v2".into(),
        poll_id: "poll2".into(),
        option_id: 2,
        voter_pubkey: "voter1".into(),
        voted_at: 1600,
    };
    repo.vote(&second).unwrap();

    assert!(repo.has_voted("poll2", "voter1").unwrap());
    assert_eq!(repo.option_count("poll2", 1).unwrap(), 0);
    assert_eq!(repo.option_count("poll2", 2).unwrap(), 1);
}

#[test]
fn test_spam_report_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = SpamReportRepo::new(&db);

    let row = SpamReportRow {
        id: "sp1".into(),
        pubkey: "reporter1".into(),
        target_id: Some("evt1".into()),
        target_pubkey: Some("target1".into()),
        reason: Some("spam".into()),
        tags: "[\"spam\",\"duplicate\"]".into(),
        created_at: 1000,
    };
    repo.insert(&row).unwrap();
    let list = repo.list_by_target("target1", 10).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].tags, "[\"spam\",\"duplicate\"]");

    repo.delete("sp1").unwrap();
    assert!(repo.list_by_target("target1", 10).unwrap().is_empty());
}

#[test]
fn test_diagnostic_log_insert_list_purge() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = DiagnosticLogRepo::new(&db);

    let l1 = DiagnosticLogRow {
        id: "dl1".into(),
        level: "error".into(),
        service: "feed".into(),
        method: "load".into(),
        message: "boom".into(),
        created_at: 1000,
    };
    let l2 = DiagnosticLogRow {
        id: "dl2".into(),
        level: "info".into(),
        service: "auth".into(),
        method: "login".into(),
        message: "ok".into(),
        created_at: 2000,
    };
    repo.insert(&l1).unwrap();
    repo.insert(&l2).unwrap();
    assert_eq!(repo.list(10, None).unwrap().len(), 2);
    let errors = repo.list(10, Some("error")).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].id, "dl1");

    assert_eq!(repo.purge_before(0).unwrap(), 2);
    assert!(repo.list(10, None).unwrap().is_empty());
}

#[test]
fn test_refetch_item_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = RefetchItemRepo::new(&db);

    let row = RefetchItemRow {
        id: "evt1".into(),
        pubkey: Some("pk1".into()),
        reason: Some("too large".into()),
        created_at: 1000,
    };
    repo.insert(&row).unwrap();
    assert!(repo.contains("evt1").unwrap());
    assert_eq!(repo.list(10).unwrap().len(), 1);

    repo.delete("evt1").unwrap();
    assert!(!repo.contains("evt1").unwrap());
}

#[test]
fn test_profile_node_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = ProfileNodeRepo::new(&db);

    let row = ProfileNodeRow {
        id: "pn1".into(),
        user_pubkey: "u1".into(),
        node_type: "text".into(),
        styles: "{\"bg\":\"#fff\"}".into(),
        properties: "{\"links\":[\"a\",\"b\"]}".into(),
        layout_row: 0,
        layout_col: 0,
        sort_order: 1,
    };
    repo.upsert(&row).unwrap();
    let list = repo.list_by_user("u1").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].properties, "{\"links\":[\"a\",\"b\"]}");
    assert_eq!(repo.delete_all_for("u1").unwrap(), 1);
    assert!(repo.list_by_user("u1").unwrap().is_empty());

    // Upsert overwrites (same id, new owner + order); list is ordered by
    // sort_order then layout_row.
    let mut row2 = ProfileNodeRow {
        id: row.id.clone(),
        user_pubkey: "u2".into(),
        node_type: row.node_type.clone(),
        styles: row.styles.clone(),
        properties: row.properties.clone(),
        layout_row: row.layout_row,
        layout_col: row.layout_col,
        sort_order: row.sort_order,
    };
    row2.user_pubkey = "u2".into();
    row2.node_type = "image".into();
    row2.layout_row = 3;
    row2.sort_order = 2;
    repo.upsert(&row2).unwrap();
    row2.sort_order = 1;
    row2.layout_row = 1;
    repo.upsert(&row2).unwrap();
    let list2 = repo.list_by_user("u2").unwrap();
    assert_eq!(list2.len(), 1);
    assert_eq!(list2[0].node_type, "image");
    assert_eq!(list2[0].layout_row, 1);
    // Upsert keyed by id: re-upserting "pn1" moved the row to u2.
    assert!(repo.list_by_user("u1").unwrap().is_empty());
    // delete removes only the matching id (id, not user).
    assert_eq!(repo.delete("u2").unwrap(), ());
    assert_eq!(
        repo.list_by_user("u2").unwrap().len(),
        1,
        "delete matched nothing"
    );
    assert_eq!(repo.delete("pn1").unwrap(), ());
    assert!(repo.list_by_user("u2").unwrap().is_empty());
}

#[test]
fn test_geohash_peer_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GeohashPeerRepo::new(&db);

    let p1 = GeohashPeerRow {
        pubkey: "peer1".into(),
        geohash: "u33d".into(),
        purpose: "dating".into(),
        first_seen: 1000,
        last_seen: 2000,
    };
    let p2 = GeohashPeerRow {
        pubkey: "peer2".into(),
        geohash: "u33d".into(),
        purpose: "both".into(),
        first_seen: 1000,
        last_seen: 3000,
    };
    repo.upsert(&p1).unwrap();
    repo.upsert(&p2).unwrap();
    assert_eq!(repo.list_by_geohash("u33d").unwrap().len(), 2);
    assert_eq!(repo.list_by_purpose("dating").unwrap().len(), 2);

    repo.delete("peer1").unwrap();
    assert_eq!(repo.list_by_geohash("u33d").unwrap().len(), 1);
}

#[test]
fn test_friend_backup_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = FriendBackupRepo::new(&db);

    let row = FriendBackupRow {
        user_pubkey: "u1".into(),
        encrypted_data: "{\"v\":1}".into(),
        updated_at: 1000,
    };
    repo.upsert(&row).unwrap();
    let found = repo.get("u1").unwrap().unwrap();
    assert_eq!(found.encrypted_data, "{\"v\":1}");

    let updated = FriendBackupRow {
        user_pubkey: "u1".into(),
        encrypted_data: "{\"v\":2}".into(),
        updated_at: 2000,
    };
    repo.upsert(&updated).unwrap();
    assert_eq!(repo.get("u1").unwrap().unwrap().updated_at, 2000);

    repo.delete("u1").unwrap();
    assert!(repo.get("u1").unwrap().is_none());
}

#[test]
fn test_link_preview_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = LinkPreviewRepo::new(&db);

    let row = LinkPreviewRow {
        url: "https://example.com/a".into(),
        domain: "example.com".into(),
        title: "Example".into(),
        description: "A page".into(),
        image: Some("https://example.com/i.png".into()),
        favicon: None,
        cached_at: 1000,
    };
    repo.upsert(&row).unwrap();
    let found = repo.get("https://example.com/a").unwrap().unwrap();
    assert_eq!(found.title, "Example");
    assert_eq!(found.image.as_deref(), Some("https://example.com/i.png"));
    assert_eq!(repo.list_recent(10).unwrap().len(), 1);

    repo.delete("https://example.com/a").unwrap();
    assert!(repo.get("https://example.com/a").unwrap().is_none());
}

#[test]
fn test_stream_chat_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = StreamChatRepo::new(&db);

    let m1 = StreamChatRow {
        id: "sc1".into(),
        stream_id: "s1".into(),
        pubkey: "pk1".into(),
        text: "hi".into(),
        created_at: 1000,
    };
    let m2 = StreamChatRow {
        id: "sc2".into(),
        stream_id: "s1".into(),
        pubkey: "pk2".into(),
        text: "yo".into(),
        created_at: 2000,
    };
    repo.insert(&m1).unwrap();
    repo.insert(&m2).unwrap();
    let list = repo.list_by_stream("s1", 10).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].text, "hi");

    assert_eq!(repo.delete_for_stream("s1").unwrap(), 2);
    assert!(repo.list_by_stream("s1", 10).unwrap().is_empty());
}

#[test]
fn test_guestbook_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GuestbookRepo::new(&db);

    let row = GuestbookEntryRow {
        id: "gb1".into(),
        profile_pubkey: "profile1".into(),
        sender_pubkey: "sender1".into(),
        sender_name: Some("Bob".into()),
        sender_avatar: None,
        content: "nice profile".into(),
        created_at: 1000,
        signature: Some("sig1".into()),
        approved: false,
    };
    repo.insert(&row).unwrap();
    assert_eq!(
        repo.list_by_profile("profile1", 10, false).unwrap().len(),
        1
    );
    assert!(repo
        .list_by_profile("profile1", 10, true)
        .unwrap()
        .is_empty());

    repo.set_approved("gb1", true).unwrap();
    assert_eq!(repo.list_by_profile("profile1", 10, true).unwrap().len(), 1);

    repo.delete("gb1").unwrap();
    assert!(repo
        .list_by_profile("profile1", 10, false)
        .unwrap()
        .is_empty());
}

#[test]
fn test_huddle_post_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = HuddlePostRepo::new(&db);

    let row = HuddlePostRow {
        id: "hp1".into(),
        huddle_id: "h1".into(),
        pubkey: "pk1".into(),
        content: "hi".into(),
        created_at: 1000,
        expires_at: 500,
    };
    repo.insert(&row).unwrap();
    assert_eq!(repo.list_by_huddle("h1", 10, true).unwrap().len(), 1);
    assert!(repo.list_by_huddle("h1", 10, false).unwrap().is_empty());

    assert_eq!(repo.delete_expired().unwrap(), 1);
    assert!(repo.list_by_huddle("h1", 10, true).unwrap().is_empty());
}

#[test]
fn test_banned_member_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = BannedMemberRepo::new(&db);

    let row = BannedMemberRow {
        group_id: "g1".into(),
        pubkey: "bad1".into(),
        banned_by: "admin1".into(),
        reason: "spam".into(),
        banned_at: 1000,
    };
    repo.insert(&row).unwrap();
    assert!(repo.is_banned("g1", "bad1").unwrap());
    assert_eq!(repo.list_by_group("g1").unwrap().len(), 1);

    repo.delete("g1", "bad1").unwrap();
    assert!(!repo.is_banned("g1", "bad1").unwrap());
}

#[test]
fn test_group_join_request_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GroupJoinRequestRepo::new(&db);

    let row = GroupJoinRequestRow {
        group_id: "g1".into(),
        pubkey: "u1".into(),
        status: "pending".into(),
        requested_at: 1000,
    };
    repo.upsert(&row).unwrap();
    let found = repo.get("g1", "u1").unwrap().unwrap();
    assert_eq!(found.status, "pending");
    assert_eq!(repo.list_by_group("g1", None).unwrap().len(), 1);
    assert_eq!(repo.list_by_group("g1", Some("pending")).unwrap().len(), 1);

    let approved = GroupJoinRequestRow {
        group_id: "g1".into(),
        pubkey: "u1".into(),
        status: "approved".into(),
        requested_at: 1000,
    };
    repo.upsert(&approved).unwrap();
    assert_eq!(repo.get("g1", "u1").unwrap().unwrap().status, "approved");

    repo.delete("g1", "u1").unwrap();
    assert!(repo.get("g1", "u1").unwrap().is_none());
}

#[test]
fn test_group_invite_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GroupInviteRepo::new(&db);

    let row = GroupInviteRow {
        id: "gi1".into(),
        group_id: "g1".into(),
        created_by: "admin1".into(),
        token: "tok1".into(),
        max_uses: 5,
        uses: 0,
        expires_at: 2000,
        created_at: 1000,
    };
    repo.create(&row).unwrap();
    let found = repo.get_by_token("tok1").unwrap().unwrap();
    assert_eq!(found.group_id, "g1");
    assert_eq!(repo.list_by_group("g1").unwrap().len(), 1);

    repo.increment_uses("gi1").unwrap();
    assert_eq!(repo.get_by_token("tok1").unwrap().unwrap().uses, 1);

    repo.delete("gi1").unwrap();
    assert!(repo.get_by_token("tok1").unwrap().is_none());
}

#[test]
fn test_musicloud_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = MusicloudRepo::new(&db);

    let row = MusicloudRow {
        id: "mc1".into(),
        pubkey: "pk1".into(),
        audio_url: "https://example.com/a.mp3".into(),
        title: Some("Song".into()),
        duration: Some(120),
        text_overlay: None,
        thumbnail: None,
        likes: 0,
        liked: false,
        bookmarked: false,
        audience: "public".into(),
        created_at: 1000,
    };
    repo.upsert(&row).unwrap();
    assert_eq!(repo.list(10, 0).unwrap().len(), 1);
    assert_eq!(repo.list_by_author("pk1", 10).unwrap().len(), 1);

    repo.set_like("mc1", true).unwrap();
    let liked = repo.list(10, 0).unwrap();
    assert!(liked[0].liked);
    assert_eq!(liked[0].likes, 1);
    repo.set_bookmark("mc1", true).unwrap();
    assert!(repo.list(10, 0).unwrap()[0].bookmarked);

    repo.delete("mc1").unwrap();
    assert!(repo.list(10, 0).unwrap().is_empty());
}

#[test]
fn test_musicloud_comment_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = MusicloudCommentRepo::new(&db);

    let c1 = MusicloudCommentRow {
        id: "cm1".into(),
        track_id: "mc1".into(),
        pubkey: "pk2".into(),
        content: "nice".into(),
        created_at: 1000,
    };
    let c2 = MusicloudCommentRow {
        id: "cm2".into(),
        track_id: "mc1".into(),
        pubkey: "pk3".into(),
        content: "cool".into(),
        created_at: 2000,
    };
    repo.insert(&c1).unwrap();
    repo.insert(&c2).unwrap();
    let list = repo.list_by_track("mc1", 10).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].content, "nice");
}

#[test]
fn test_story_reaction_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = StoryReactionRepo::new(&db);

    let row = StoryReactionRow {
        story_id: "st1".into(),
        pubkey: "u1".into(),
        emoji: "🔥".into(),
        created_at: 1000,
    };
    repo.react(&row).unwrap();
    assert!(repo.reacted_with("st1", "u1", "🔥").unwrap());
    assert_eq!(repo.list_by_story("st1", 10).unwrap().len(), 1);

    repo.react(&row).unwrap();
    assert_eq!(repo.list_by_story("st1", 10).unwrap().len(), 1);

    repo.unreact("st1", "u1", "🔥").unwrap();
    assert!(!repo.reacted_with("st1", "u1", "🔥").unwrap());
}

#[test]
fn test_conversation_mute_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = ConversationMuteRepo::new(&db);

    repo.mute("conv1", 1000).unwrap();
    assert!(repo.is_muted("conv1").unwrap());
    assert_eq!(repo.list().unwrap(), vec!["conv1".to_string()]);

    repo.unmute("conv1").unwrap();
    assert!(!repo.is_muted("conv1").unwrap());
    assert!(repo.list().unwrap().is_empty());
}

#[test]
fn test_dating_unmatch_crud() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = DatingUnmatchRepo::new(&db);

    repo.upsert("actorA", "u1", 1000).unwrap();
    assert!(repo.is_unmatched("actorA", "u1").unwrap());
    // Actor-scoped: a different actor does not see actorA's unmatch.
    assert!(!repo.is_unmatched("actorB", "u1").unwrap());

    repo.upsert("actorA", "u1", 2000).unwrap();
    repo.delete("actorA", "u1").unwrap();
    assert!(!repo.is_unmatched("actorA", "u1").unwrap());
}

#[test]
fn test_limits_helpers() {
    assert!(!row_too_big("x", "[]"));
    let big = "x".repeat(limits::MAX_CONTENT_BYTES + 1);
    assert!(row_too_big(&big, "[]"));
    let huge_tags = "y".repeat(limits::MAX_BATCH_BYTES);
    assert!(row_too_big("x", &huge_tags));
    assert!(!notification_too_big("short"));
    let long = "z".repeat(limits::MAX_NOTIFICATION_BYTES + 1);
    assert!(notification_too_big(&long));
}

#[test]
fn test_migration_rollback_on_step_failure() {
    let db = Database::open_in_memory().unwrap();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));",
        ))
        .unwrap();
        soshal_db_core::block_on(conn.execute_batch(
            "CREATE TRIGGER fail_mig BEFORE INSERT ON _migrations BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        ))
        .unwrap();
    }
    assert!(db.migrate().is_err());
    {
        let conn = db.conn().unwrap();
        let version: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COALESCE(MAX(version), 0) FROM _migrations",
            (),
            |row| row.get(0),
        )
        .unwrap()
        .unwrap_or(0);
        assert_eq!(version, 0);
        let users: Vec<String> = soshal_db_core::query::query(
            &conn,
            "SELECT name FROM sqlite_master WHERE type='table' AND name='users'",
            (),
            |row| row.get(0),
        )
        .unwrap();
        assert!(users.is_empty());
        soshal_db_core::block_on(conn.execute_batch("DROP TRIGGER fail_mig;")).unwrap();
    }
    db.migrate().unwrap();
    {
        let conn = db.conn().unwrap();
        let version: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COALESCE(MAX(version), 0) FROM _migrations",
            (),
            |row| row.get(0),
        )
        .unwrap()
        .unwrap_or(0);
        assert_eq!(version, SCHEMA_VERSION);
        let users: Vec<String> = soshal_db_core::query::query(
            &conn,
            "SELECT name FROM sqlite_master WHERE type='table' AND name='users'",
            (),
            |row| row.get(0),
        )
        .unwrap();
        assert_eq!(users.len(), 1);
    }
}

#[test]
fn test_async_query_api() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        use soshal_db_core::libsql::params;
        let n = soshal_db_core::query::execute_async(
            &conn,
            "INSERT INTO users (pubkey, npub, created_at, updated_at) VALUES (?1,?2,?3,?4)",
            params!["async_pk", "npub_async", 1000i64, 1000i64],
        )
        .await
        .unwrap();
        assert_eq!(n, 1);

        let rows = soshal_db_core::query::query_async(
            &conn,
            "SELECT pubkey, npub FROM users WHERE pubkey=?1",
            params!["async_pk"],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "async_pk");
        assert_eq!(rows[0].1, "npub_async");

        let first = soshal_db_core::query::query_first_async(
            &conn,
            "SELECT npub FROM users WHERE pubkey=?1",
            params!["async_pk"],
            |r| r.get::<String>(0),
        )
        .await
        .unwrap();
        assert_eq!(first.as_deref(), Some("npub_async"));

        let missing = soshal_db_core::query::query_first_async(
            &conn,
            "SELECT npub FROM users WHERE pubkey=?1",
            params!["nope"],
            |r| r.get::<String>(0),
        )
        .await
        .unwrap();
        assert!(missing.is_none());

        let capped = soshal_db_core::query::query_capacity_async(
            &conn,
            "SELECT pubkey FROM users",
            (),
            4,
            |r| r.get::<String>(0),
        )
        .await
        .unwrap();
        assert_eq!(capped, vec!["async_pk".to_string()]);
    });
}

#[test]
fn test_message_upsert_batch_empty_and_oversize_skip() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = MessageRepo::new(&db);

    // Empty slice is a no-op.
    repo.upsert_batch(&[]).unwrap();
    assert!(repo
        .get_conversation("conv:1", 10, None)
        .unwrap()
        .is_empty());

    let valid = MessageRow {
        id: "m1".into(),
        conversation_id: "conv:1".into(),
        pubkey: "pk1".into(),
        content: "hello".into(),
        created_at: 100,
        tags_json: "[]".into(),
        reply_to: None,
        sync_status: "synced".into(),
        is_deleted: false,
    };
    let oversize = MessageRow {
        id: "m2".into(),
        conversation_id: "conv:1".into(),
        pubkey: "pk1".into(),
        content: "x".repeat(limits::MAX_CONTENT_BYTES + 1),
        created_at: 200,
        tags_json: "[]".into(),
        reply_to: None,
        sync_status: "synced".into(),
        is_deleted: false,
    };

    repo.upsert_batch(&[valid, oversize]).unwrap();
    let msgs = repo.get_conversation("conv:1", 10, None).unwrap();
    assert_eq!(msgs.len(), 1, "oversize row must be skipped");
    assert_eq!(msgs[0].id, "m1");
    assert!(repo.get_by_id("m2").unwrap().is_none());
    // Conversation last_message_at touched by the valid row only.
    let conn = db.conn().unwrap();
    let at: Option<i64> = soshal_db_core::query::query_first(
        &conn,
        "SELECT last_message_at FROM conversations WHERE conversation_id=?1",
        soshal_db_core::libsql::params!["conv:1"],
        |r| r.get(0),
    )
    .unwrap();
    assert_eq!(at, Some(100));
}

#[test]
fn test_message_upsert_batch_conflict_update() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = MessageRepo::new(&db);

    let make = |id: &str, content: &str, created_at: i64| MessageRow {
        id: id.into(),
        conversation_id: "conv:2".into(),
        pubkey: "pk1".into(),
        content: content.into(),
        created_at,
        tags_json: "[]".into(),
        reply_to: None,
        sync_status: "synced".into(),
        is_deleted: false,
    };

    // Same id twice in one batch: last occurrence wins.
    repo.upsert_batch(&[make("m1", "first", 100), make("m1", "second", 100)])
        .unwrap();
    let got = repo.get_by_id("m1").unwrap().unwrap();
    assert_eq!(got.content, "second");

    // Re-batch with the same id updates stored fields.
    let updated = MessageRow {
        content: "edited".into(),
        is_deleted: true,
        ..make("m1", "second", 100)
    };
    repo.upsert_batch(&[updated]).unwrap();
    let got = repo.get_by_id("m1").unwrap().unwrap();
    assert_eq!(got.content, "edited");
    assert!(got.is_deleted);
    // get_conversation excludes deleted rows; undelete and re-check.
    repo.upsert_batch(&[MessageRow {
        id: "m1".into(),
        conversation_id: "conv:2".into(),
        pubkey: "pk1".into(),
        content: "edited".into(),
        created_at: 100,
        tags_json: "[]".into(),
        reply_to: None,
        sync_status: "synced".into(),
        is_deleted: false,
    }])
    .unwrap();
    assert_eq!(repo.get_conversation("conv:2", 10, None).unwrap().len(), 1);
}

#[test]
fn test_escrow_list_ordering() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = EscrowRepo::new(&db);

    // Empty db -> empty vec.
    assert!(repo.list().unwrap().is_empty());

    let make = |id: &str, created_at: i64| EscrowRow {
        id: id.into(),
        listing_id: "l1".into(),
        buyer_pubkey: "buyer".into(),
        seller_pubkey: "seller".into(),
        amount_msats: 1000,
        currency: "sat".into(),
        status: "created".into(),
        escrow_note: None,
        created_at,
        updated_at: created_at,
    };
    repo.create(&make("e1", 100)).unwrap();
    repo.create(&make("e2", 300)).unwrap();
    repo.create(&make("e3", 200)).unwrap();

    let list = repo.list().unwrap();
    let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["e2", "e3", "e1"], "list must be created_at DESC");
    let ats: Vec<i64> = list.iter().map(|e| e.created_at).collect();
    assert_eq!(ats, vec![300, 200, 100]);
}

#[test]
fn test_group_role_delete_by_id() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::query::execute(
            &conn,
            "INSERT INTO groups (id, name) VALUES ('grp_del', 'Test Group')",
            (),
        )
        .unwrap();
    }
    let repo = GroupRoleRepo::new(&db);

    let role1 = GroupRoleRow {
        id: "role_del_1".into(),
        group_id: "grp_del".into(),
        name: "Moderator".into(),
        color: "#ff0000".into(),
        position: 1,
        permissions: "[]".into(),
        created_at: 100,
    };
    let role2 = GroupRoleRow {
        id: "role_del_2".into(),
        group_id: "grp_del".into(),
        name: "Admin".into(),
        color: "#00ff00".into(),
        position: 2,
        permissions: "[]".into(),
        created_at: 200,
    };
    repo.upsert(&role1).unwrap();
    repo.upsert(&role2).unwrap();
    assert_eq!(repo.list("grp_del").unwrap().len(), 2);

    repo.delete("role_del_1").unwrap();
    let remaining = repo.list("grp_del").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "role_del_2");

    repo.delete("role_del_missing").unwrap();
    assert_eq!(repo.list("grp_del").unwrap().len(), 1);
}

#[test]
fn test_ephemeral_media_delete_by_id() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = EphemeralMediaRepo::new(&db);

    let em1 = EphemeralMediaRow {
        id: "em_del_1".into(),
        message_id: "msg1".into(),
        conversation_id: "conv1".into(),
        conversation_type: "dm".into(),
        media_url: "https://example.com/a.jpg".into(),
        media_type: "image".into(),
        sender_pubkey: "sender1".into(),
        recipient_pubkey: "recipient1".into(),
        max_views: 2,
        current_views: 0,
        state: "pending".into(),
        expires_at: Some(2000),
        created_at: 1000,
        viewed_at: None,
    };
    let em2 = EphemeralMediaRow {
        id: "em_del_2".into(),
        message_id: "msg2".into(),
        conversation_id: "conv1".into(),
        conversation_type: "dm".into(),
        media_url: "https://example.com/b.jpg".into(),
        media_type: "image".into(),
        sender_pubkey: "sender1".into(),
        recipient_pubkey: "recipient1".into(),
        max_views: 1,
        current_views: 0,
        state: "pending".into(),
        expires_at: Some(2000),
        created_at: 1001,
        viewed_at: None,
    };
    repo.create(&em1).unwrap();
    repo.create(&em2).unwrap();

    repo.delete("em_del_1").unwrap();
    assert!(repo.get("em_del_1").unwrap().is_none());
    let sibling = repo.get("em_del_2").unwrap().unwrap();
    assert_eq!(sibling.message_id, "msg2");

    repo.delete("em_del_missing").unwrap();
    assert!(repo.get("em_del_2").unwrap().is_some());
}

#[test]
fn test_huddle_post_delete_by_id() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = HuddlePostRepo::new(&db);

    let hp1 = HuddlePostRow {
        id: "hp_del_1".into(),
        huddle_id: "h_del".into(),
        pubkey: "pk1".into(),
        content: "one".into(),
        created_at: 1000,
        expires_at: 500,
    };
    let hp2 = HuddlePostRow {
        id: "hp_del_2".into(),
        huddle_id: "h_del".into(),
        pubkey: "pk2".into(),
        content: "two".into(),
        created_at: 1001,
        expires_at: 600,
    };
    repo.insert(&hp1).unwrap();
    repo.insert(&hp2).unwrap();
    assert_eq!(repo.list_by_huddle("h_del", 10, true).unwrap().len(), 2);

    repo.delete("hp_del_1").unwrap();
    let remaining = repo.list_by_huddle("h_del", 10, true).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "hp_del_2");

    repo.delete("hp_del_missing").unwrap();
    assert_eq!(repo.list_by_huddle("h_del", 10, true).unwrap().len(), 1);
}

#[test]
fn test_reminder_delete_by_id() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = ReminderRepo::new(&db);

    let r1 = ReminderRow {
        id: "rem_del_1".into(),
        event_id: "evt_del".into(),
        title: "Delete Me".into(),
        start_time: 900,
        minutes_before: 10,
        created_at: 0,
    };
    repo.upsert(&r1).unwrap();
    assert_eq!(repo.list().unwrap().len(), 1);

    repo.delete("rem_del_1").unwrap();
    assert!(repo.list().unwrap().is_empty());

    repo.delete("rem_del_missing").unwrap();
    assert!(repo.list().unwrap().is_empty());
}

#[test]
fn test_group_thread_reaction_toggle_and_sort() {
    use soshal_db_core::repos::thread::{
        GroupThreadReplyRow, GroupThreadRepo, GroupThreadRow, ThreadSort,
    };

    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    let repo = GroupThreadRepo::new(&db);
    let now = soshal_common_core::format::now_secs();

    let busy = GroupThreadRow {
        id: "thr_busy".into(),
        group_id: "g_t".into(),
        title: "Busy".into(),
        body: "".into(),
        author: "a".repeat(64),
        created_at: now - 7200,
        is_pinned: false,
        reply_count: 0,
        reaction_count: 0,
    };
    let quiet = GroupThreadRow {
        id: "thr_quiet".into(),
        group_id: "g_t".into(),
        title: "Quiet".into(),
        body: "".into(),
        author: "a".repeat(64),
        created_at: now - 100,
        is_pinned: false,
        reply_count: 0,
        reaction_count: 0,
    };
    repo.upsert(&busy).unwrap();
    repo.upsert(&quiet).unwrap();

    assert!(repo
        .add_reaction("thr_busy", "", "b".repeat(64).as_str(), "👍")
        .unwrap());
    assert!(!repo
        .add_reaction("thr_busy", "", "b".repeat(64).as_str(), "👍")
        .unwrap());
    assert!(repo
        .add_reaction("thr_busy", "", "c".repeat(64).as_str(), "👍")
        .unwrap());
    assert!(repo
        .add_reaction("thr_busy", "", "b".repeat(64).as_str(), "❤️")
        .unwrap());
    assert!(repo
        .add_reaction("thr_busy", "", "b".repeat(64).as_str(), "🔥")
        .unwrap());

    let summary = repo
        .reaction_summary("thr_busy", "b".repeat(64).as_str())
        .unwrap();
    assert_eq!(summary.len(), 3);
    let thumbs = summary.iter().find(|r| r.emoji == "👍").unwrap();
    assert_eq!(thumbs.count, 2);
    assert!(thumbs.reacted);
    assert!(thumbs.reply_id.is_empty());

    // Toggle off the viewer's own 👍.
    assert!(!repo
        .toggle_reaction("thr_busy", "", "b".repeat(64).as_str(), "👍")
        .unwrap());
    let summary = repo
        .reaction_summary("thr_busy", "b".repeat(64).as_str())
        .unwrap();
    let thumbs = summary.iter().find(|r| r.emoji == "👍").unwrap();
    assert_eq!(thumbs.count, 1);
    assert!(!thumbs.reacted);

    // Toggle back on.
    assert!(repo
        .toggle_reaction("thr_busy", "", "b".repeat(64).as_str(), "👍")
        .unwrap());

    // Reply-level reactions carry reply_id.
    let rpl = GroupThreadReplyRow {
        id: "rpl_1".into(),
        thread_id: "thr_busy".into(),
        parent_id: "".into(),
        author: "b".repeat(64),
        content: "hi".into(),
        created_at: now,
    };
    repo.add_reply(&rpl).unwrap();
    assert!(repo
        .add_reaction("thr_busy", "rpl_1", "c".repeat(64).as_str(), "🔥")
        .unwrap());
    let summary = repo
        .reaction_summary("thr_busy", "b".repeat(64).as_str())
        .unwrap();
    assert!(summary
        .iter()
        .any(|r| r.reply_id == "rpl_1" && r.emoji == "🔥"));

    // Hot sort: the 2-hour-old busy thread (4 reactions + 1 reply) outranks
    // the quiet fresh one.
    let popular = repo.list("g_t", ThreadSort::Popular).unwrap();
    assert_eq!(popular[0].id, "thr_busy");
    assert!(popular[0].reaction_count >= 4);
    assert_eq!(popular[1].id, "thr_quiet");

    // Newest sort: fresh first.
    let newest = repo.list("g_t", ThreadSort::Newest).unwrap();
    assert_eq!(newest[0].id, "thr_quiet");

    // Pinned always floats to the top.
    repo.set_pinned("thr_quiet", true).unwrap();
    let popular = repo.list("g_t", ThreadSort::Popular).unwrap();
    assert_eq!(popular[0].id, "thr_quiet");

    // Deleting a thread cascades reactions.
    repo.delete("thr_busy").unwrap();
    let summary = repo
        .reaction_summary("thr_busy", "b".repeat(64).as_str())
        .unwrap();
    assert!(summary.is_empty());
    assert!(repo.delete_reply("rpl_1").is_ok());
}

#[test]
fn test_message_cursor_pagination_no_gap_same_ts() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = MessageRepo::new(&db);

    // 5 messages, 3 sharing the same ts to stress the keyset cursor.
    let at_new = 3000;
    let at_boundary = 2000;
    let at_old = 1000;
    for (i, (id, ts)) in [
        ("m10", at_new),
        ("m9", at_boundary),
        ("m8", at_boundary),
        ("m7", at_boundary),
        ("m6", at_old),
    ]
    .iter()
    .enumerate()
    {
        repo.upsert(&MessageRow {
            id: (*id).into(),
            conversation_id: "conv:gap".into(),
            pubkey: "pk1".into(),
            content: format!("c{}", i),
            created_at: *ts,
            tags_json: "[]".into(),
            reply_to: None,
            sync_status: "synced".into(),
            is_deleted: false,
        })
        .unwrap();
    }

    // First page: limit 2, newest first → m10, m9.
    let page1 = repo
        .get_conversation("conv:gap", 2, None)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect::<Vec<_>>();
    assert_eq!(page1, vec!["m10", "m9"]);

    // Next page keyset cursor = (created_at=2000, id="m9").
    let page2 = repo
        .get_conversation("conv:gap", 2, Some((at_boundary, "m9".to_string())))
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect::<Vec<_>>();
    // Same-ts boundary rows follow by id DESC (m8 > m9) then older rows.
    assert_eq!(page2, vec!["m8", "m7"]);

    // Final page trailing rows after (2000, "m7").
    let page3 = repo
        .get_conversation("conv:gap", 2, Some((at_boundary, "m7".to_string())))
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect::<Vec<_>>();
    assert_eq!(page3, vec!["m6"]);

    // No dup no skip across all pages.
    let all: Vec<String> = page1.into_iter().chain(page2).chain(page3).collect();
    assert_eq!(all, vec!["m10", "m9", "m8", "m7", "m6"]);
}

#[test]
fn test_post_cursor_pagination_no_gap_same_ts() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    insert_test_user(&db, "pk1");
    let repo = PostRepo::new(&db);

    // 5 posts; ids chosen so id DESC order within a ts group is deterministic
    // ("post_c" > "post_b" > "post_a").
    let at_new = 3000;
    let at_boundary = 2000;
    let at_old = 1000;
    let insert = |id: &str, ts: i64| {
        repo.upsert(&PostRow {
            id: id.into(),
            pubkey: "pk1".into(),
            content: id.into(),
            kind: 1,
            created_at: ts,
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
            is_freenet_native: true,
            rsvp_event_id: None,
        })
        .unwrap();
    };
    insert("post_d", at_new);
    insert("post_c", at_boundary);
    insert("post_b", at_boundary);
    insert("post_a", at_boundary);
    insert("post_0", at_old);

    // Page 1 starts at the top: cursor timestamp above all rows.
    let page1 = repo
        .get_paged_meta_cursor(i64::MAX, "", 2)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    assert_eq!(page1, vec!["post_d", "post_c"]);

    let page2 = repo
        .get_paged_meta_cursor(at_boundary, "post_c", 2)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    assert_eq!(page2, vec!["post_b", "post_a"]);

    let page3 = repo
        .get_paged_meta_cursor(at_boundary, "post_a", 2)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    assert_eq!(page3, vec!["post_0"]);

    let all: Vec<String> = page1.into_iter().chain(page2).chain(page3).collect();
    assert_eq!(all, vec!["post_d", "post_c", "post_b", "post_a", "post_0"]);
}

#[test]
fn test_case_insensitivity_and_normalization_regression() {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();

    insert_test_user(&db, "User_Alpha");
    insert_test_user(&db, "User_Beta");
    insert_test_user(&db, "Pk_FrIeNd");
    insert_test_user(&db, "ReAcToR_1");
    insert_test_user(&db, "Bm_UsEr");
    insert_test_user(&db, "MemBeR_X");
    insert_test_user(&db, "ApPlIcAnT_1");
    insert_test_user(&db, "admin_pk");
    insert_test_user(&db, "sender1");
    insert_test_user(&db, "recip1");

    let grp_setup = GroupRepo::new(&db);
    grp_setup
        .upsert(&GroupRow {
            id: "grp1".into(),
            name: "Test Group".into(),
            about: None,
            picture: None,
            pubkey: "admin_pk".into(),
            created_at: 1000,
            updated_at: 1000,
            access_type: "open".into(),
            relay: None,
            sync_status: "synced".into(),
            password_hash: None,
        })
        .unwrap();

    // 1. MusicloudPlaylistRepo: create with mixed case, list with uppercase, created_at > 0
    let pl_repo = MusicloudPlaylistRepo::new(&db);
    pl_repo
        .create("pl1", "MiXeD_pKb1", "pLaYlIsT 1", false)
        .unwrap();
    let pl_list = pl_repo.list("MIXED_PKB1", 10).unwrap();
    assert_eq!(pl_list.len(), 1);
    assert_eq!(pl_list[0].id, "pl1");
    assert!(pl_list[0].created_at > 0);

    // 2. SavedContentRepo: upsert with mixed case, get and delete with uppercase
    let saved_repo = SavedContentRepo::new(&db);
    let s_row = SavedContentRow {
        kind: 30078,
        id: "save_item_1".into(),
        pubkey: "Pk_SaVeD".into(),
        d: "d1".into(),
        media_type: "audio".into(),
        media_url: "url".into(),
        text_overlay: "".into(),
        title: "track info".into(),
        thumbnail: "".into(),
        blob_hash: "".into(),
        media_size: 100,
        audience: "public".into(),
        hashtags: "[]".into(),
        host_ready: true,
        created_at: 100,
        saved_at: 500,
    };
    saved_repo.upsert(&s_row).unwrap();
    let retrieved = saved_repo.get(30078, "save_item_1").unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().pubkey, "pk_saved");
    saved_repo.delete(30078, "save_item_1").unwrap();
    assert!(saved_repo.get(30078, "save_item_1").unwrap().is_none());

    // 3. BlockRepo: upsert with mixed-case, check and list with uppercase
    let block_repo = BlockRepo::new(&db);
    let b_row = BlockRow {
        pubkey: "User_Alpha".into(),
        blocked_pubkey: "User_Beta".into(),
        created_at: 100,
    };
    block_repo.upsert(&b_row).unwrap();
    assert!(block_repo.is_blocked("USER_ALPHA", "USER_BETA").unwrap());
    assert_eq!(
        block_repo.list("USER_ALPHA").unwrap(),
        vec!["User_Beta".to_string()]
    );
    block_repo.delete("USER_ALPHA", "USER_BETA").unwrap();
    assert!(!block_repo.is_blocked("USER_ALPHA", "USER_BETA").unwrap());

    // 4. DatingUnmatchRepo: upsert with mixed-case, check with uppercase
    let dating_repo = DatingUnmatchRepo::new(&db);
    dating_repo.upsert("Actor_A", "Target_B", 200).unwrap();
    assert!(dating_repo.is_unmatched("ACTOR_A", "TARGET_B").unwrap());
    dating_repo.delete("ACTOR_A", "TARGET_B").unwrap();
    assert!(!dating_repo.is_unmatched("ACTOR_A", "TARGET_B").unwrap());

    // 5. FriendBackupRepo: upsert with mixed-case, get with uppercase
    let fb_repo = FriendBackupRepo::new(&db);
    let fb_row = FriendBackupRow {
        user_pubkey: "Pk_FrIeNd".into(),
        encrypted_data: "secret".into(),
        updated_at: 300,
    };
    fb_repo.upsert(&fb_row).unwrap();
    assert!(fb_repo.get("PK_FRIEND").unwrap().is_some());
    fb_repo.delete("PK_FRIEND").unwrap();
    assert!(fb_repo.get("PK_FRIEND").unwrap().is_none());

    // 6. StoryReactionRepo: react with mixed-case, check with uppercase
    let story_repo = StoryReactionRepo::new(&db);
    let srx = StoryReactionRow {
        story_id: "story_123".into(),
        pubkey: "ReAcToR".into(),
        emoji: "👍".into(),
        created_at: 400,
    };
    story_repo.react(&srx).unwrap();
    assert!(story_repo
        .reacted_with("story_123", "REACTOR", "👍")
        .unwrap());
    story_repo.unreact("story_123", "REACTOR", "👍").unwrap();
    assert!(!story_repo
        .reacted_with("story_123", "REACTOR", "👍")
        .unwrap());

    // 7. ReactionRepo: upsert with mixed-case event_id, check with uppercase
    let rx_repo = ReactionRepo::new(&db);
    let rx_row1 = ReactionRow {
        id: "rx1".into(),
        event_id: "EvT_AbCd".into(),
        pubkey: "ReAcToR_1".into(),
        kind: 7,
        content: Some("+".into()),
        created_at: 500,
    };
    rx_repo.upsert(&rx_row1).unwrap();
    assert_eq!(rx_repo.get_by_event("EVT_ABCD").unwrap().len(), 1);
    // Upserting with changed casing replaces rather than duplicate
    let rx_row2 = ReactionRow {
        id: "rx2".into(),
        event_id: "evt_abcd".into(),
        pubkey: "ReAcToR_1".into(),
        kind: 7,
        content: Some("❤️".into()),
        created_at: 600,
    };
    rx_repo.upsert(&rx_row2).unwrap();
    let rx_list = rx_repo.get_by_event("EVT_ABCD").unwrap();
    assert_eq!(rx_list.len(), 1);
    assert_eq!(rx_list[0].content, Some("❤️".into()));

    // 8. BookmarkRepo: upsert with mixed-case pubkey, get with uppercase
    let bm_repo = BookmarkRepo::new(&db);
    let bm_row = BookmarkRow {
        id: "bm1".into(),
        pubkey: "Bm_UsEr".into(),
        event_id: "evt_bm".into(),
        created_at: 700,
    };
    bm_repo.upsert(&bm_row).unwrap();
    let bms = bm_repo.get_user_bookmarks("BM_USER", 10, 0).unwrap();
    assert_eq!(bms.len(), 1);
    assert_eq!(bms[0].id, "bm1");
    bm_repo.delete("BM1").unwrap();
    assert!(bm_repo
        .get_user_bookmarks("BM_USER", 10, 0)
        .unwrap()
        .is_empty());

    // 9. GroupRepo: add_member with mixed-case, then check and remove with uppercase
    let grp_repo = GroupRepo::new(&db);
    grp_repo
        .add_member("grp1", "MemBeR_X", "member", 800)
        .unwrap();
    assert!(grp_repo.is_member("grp1", "MEMBER_X").unwrap());
    // Adding again with same pubkey updates rather than duplicate
    grp_repo
        .add_member("grp1", "MemBeR_X", "admin", 850)
        .unwrap();
    assert_eq!(grp_repo.get_members("grp1").unwrap().len(), 1);
    assert_eq!(grp_repo.get_members("grp1").unwrap()[0].role, "admin");
    grp_repo.remove_member("grp1", "MEMBER_X").unwrap();
    assert!(!grp_repo.is_member("grp1", "MEMBER_X").unwrap());

    // 10. GroupJoinRequestRepo: upsert with mixed-case, get with uppercase
    let req_repo = GroupJoinRequestRepo::new(&db);
    let req_row = GroupJoinRequestRow {
        group_id: "grp1".into(),
        pubkey: "ApPlIcAnT_1".into(),
        status: "pending".into(),
        requested_at: 900,
    };
    req_repo.upsert(&req_row).unwrap();
    assert!(req_repo.get("grp1", "APPLICANT_1").unwrap().is_some());
    req_repo.delete("grp1", "APPLICANT_1").unwrap();
    assert!(req_repo.get("grp1", "APPLICANT_1").unwrap().is_none());

    // 11. ZapRepo: upsert with mixed-case event_id, sum_by_event with uppercase
    let zap_repo = ZapRepo::new(&db);
    let zap_row = ZapRow {
        id: "zap1".into(),
        pubkey: "sender1".into(),
        recipient_pubkey: "recip1".into(),
        event_id: Some("EvT_ZaPpEd".into()),
        amount: 2100,
        amount_msat: 2100000,
        content: Some("great post".into()),
        created_at: 950,
        zap_type: "public".into(),
    };
    zap_repo.upsert(&zap_row).unwrap();
    assert_eq!(zap_repo.sum_by_event("EVT_ZAPPED").unwrap(), 2100);

    // 12. UserRepo: ensure_exists with uppercase pubkey creates lowercase row, get_by_pubkey finds it
    let user_repo = UserRepo::new(&db);
    user_repo.ensure_exists("UPPER_USER").unwrap();
    let u = user_repo.get_by_pubkey("upper_user").unwrap();
    assert!(u.is_some());
    assert_eq!(u.unwrap().pubkey, "upper_user");
    let u_upper = user_repo.get_by_pubkey("UPPER_USER").unwrap();
    assert!(u_upper.is_some());
}
