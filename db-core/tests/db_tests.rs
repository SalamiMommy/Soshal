use soshal_db_core::repos::audit_log::{AuditLogRepo, AuditLogRow};
use soshal_db_core::repos::block::{BlockRepo, BlockRow};
use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::group::{GroupRepo, GroupRow};
use soshal_db_core::repos::hashtag::{HashtagRepo, HashtagRow};
use soshal_db_core::repos::media::{MediaRepo, MediaRow};
use soshal_db_core::repos::message::{MessageRepo, MessageRow};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::post_views::PostViewsRepo;
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::reminder::{ReminderRepo, ReminderRow};
use soshal_db_core::repos::repost::{RepostRepo, RepostRow};
use soshal_db_core::repos::role::{GroupRoleRepo, GroupRoleRow};
use soshal_db_core::repos::search_index::SearchIndexRepo;
use soshal_db_core::repos::settings::SettingsRepo;
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
    };
    repo.upsert(&updated_user).unwrap();

    let results = repo.search("Alice%", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name.as_deref(), Some("Alice%Bob"));
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
    };
    post_repo.upsert(&post).unwrap();

    let results = fts_repo.search("Nostr", 10, 0).unwrap();
    assert_eq!(results.len(), 1);
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
        content: Some("Great post!".into()),
        created_at: 100,
        zap_type: "public".into(),
    };
    repo.upsert(&row1).unwrap();
    assert_eq!(repo.sum_by_event("evt1").unwrap(), 1000);
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
