use soshal_db_core::change_bus::{ChangeBus, Table};
use soshal_db_core::repos::message::{MessageRepo, MessageRow};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::Database;

#[tokio::test(flavor = "multi_thread")]
async fn test_change_bus_direct_broadcast() {
    let bus = ChangeBus::new(16);
    assert_eq!(bus.receiver_count(), 0);

    let mut sub1 = bus.subscribe();
    let mut sub2 = bus.subscribe();
    assert_eq!(bus.receiver_count(), 2);

    bus.notify(Table::Posts, Some("alice".to_string()));

    let evt1 = sub1.recv().await.expect("sub1 receives event");
    assert_eq!(evt1.table, Table::Posts);
    assert_eq!(evt1.affected_account.as_deref(), Some("alice"));

    let evt2 = sub2.recv().await.expect("sub2 receives event");
    assert_eq!(evt2.table, Table::Posts);
    assert_eq!(evt2.affected_account.as_deref(), Some("alice"));
    assert!(bus.last_event_ts() > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_post_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = PostRepo::new(&db);
    let post = PostRow {
        id: "post_test_001".to_string(),
        pubkey: "author_pk_123".to_string(),
        content: "Hello reactive world".to_string(),
        kind: 1,
        created_at: 1000,
        tags_json: "[]".to_string(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: "".to_string(),
        mentioned_hashtags: "".to_string(),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    };

    repo.upsert(&post).expect("upsert post");

    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Posts);
    assert_eq!(evt.affected_account.as_deref(), Some("author_pk_123"));

    repo.delete("post_test_001").expect("delete post");
    let delete_evt = rx.recv().await.expect("receive delete event");
    assert_eq!(delete_evt.table, Table::Posts);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_reaction_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = ReactionRepo::new(&db);
    let reaction = ReactionRow {
        id: "rx_test_001".to_string(),
        pubkey: "reactor_pk_456".to_string(),
        event_id: "target_post_111".to_string(),
        kind: 7,
        content: Some("+".to_string()),
        created_at: 1001,
    };

    repo.upsert(&reaction).expect("upsert reaction");

    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Reactions);
    assert_eq!(evt.affected_account.as_deref(), Some("reactor_pk_456"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_message_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = MessageRepo::new(&db);
    let msg = MessageRow {
        id: "msg_test_001".to_string(),
        conversation_id: "conv:alice:bob".to_string(),
        pubkey: "sender_alice".to_string(),
        content: "Hi Bob".to_string(),
        created_at: 1002,
        tags_json: "[]".to_string(),
        reply_to: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
    };

    repo.upsert(&msg).expect("upsert message");

    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Messages);
    assert_eq!(evt.affected_account.as_deref(), Some("sender_alice"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_notification_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = NotificationRepo::new(&db);
    let notif = NotificationRow {
        id: "notif_test_001".to_string(),
        pubkey: "notified_user".to_string(),
        type_: "mention".to_string(),
        event_id: Some("ev_1".to_string()),
        from_pubkey: Some("sender_user".to_string()),
        content: Some("mentioned you".to_string()),
        created_at: 1003,
        is_read: false,
    };

    repo.upsert(&notif).expect("upsert notif");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Notifications);
    assert_eq!(evt.affected_account.as_deref(), Some("notified_user"));

    let marked = repo
        .mark_as_read("notified_user", "notif_test_001")
        .expect("mark read");
    assert!(marked);

    let read_evt = rx.recv().await.expect("receive read change event");
    assert_eq!(read_evt.table, Table::Notifications);
    assert_eq!(read_evt.affected_account.as_deref(), Some("notified_user"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_block_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let user_repo = soshal_db_core::repos::user::UserRepo::new(&db);
    user_repo.ensure_exists("alice_pk").expect("insert user");
    user_repo.ensure_exists("spammer_pk").expect("insert user");

    let repo = soshal_db_core::repos::block::BlockRepo::new(&db);
    let row = soshal_db_core::repos::block::BlockRow {
        pubkey: "alice_pk".to_string(),
        blocked_pubkey: "spammer_pk".to_string(),
        created_at: 1004,
    };

    repo.upsert(&row).expect("upsert block");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Blocks);
    assert_eq!(evt.affected_account.as_deref(), Some("alice_pk"));

    repo.delete("alice_pk", "spammer_pk").expect("delete block");
    let del_evt = rx.recv().await.expect("receive delete change event");
    assert_eq!(del_evt.table, Table::Blocks);
    assert_eq!(del_evt.affected_account.as_deref(), Some("alice_pk"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_bookmark_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = soshal_db_core::repos::bookmark::BookmarkRepo::new(&db);
    let row = soshal_db_core::repos::bookmark::BookmarkRow {
        id: "bm_1".to_string(),
        pubkey: "alice_pk".to_string(),
        event_id: "evt_1".to_string(),
        created_at: 1005,
    };

    repo.upsert(&row).expect("upsert bookmark");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Bookmarks);
    assert_eq!(evt.affected_account.as_deref(), Some("alice_pk"));

    repo.delete("bm_1").expect("delete bookmark");
    let del_evt = rx.recv().await.expect("receive delete change event");
    assert_eq!(del_evt.table, Table::Bookmarks);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_settings_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = soshal_db_core::repos::settings::SettingsRepo::new(&db);
    repo.set("dark_mode", "true").expect("set setting");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Settings);

    repo.delete("dark_mode").expect("delete setting");
    let del_evt = rx.recv().await.expect("receive delete change event");
    assert_eq!(del_evt.table, Table::Settings);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_relay_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = soshal_db_core::repos::relay::RelayRepo::new(&db);
    let row = soshal_db_core::repos::relay::RelayRow {
        url: "wss://relay.damus.io".to_string(),
        pubkey: None,
        name: Some("Damus".to_string()),
        read_enabled: true,
        write_enabled: true,
        priority: 1,
        last_connected_at: None,
        health_score: 1.0,
    };

    repo.upsert(&row).expect("upsert relay");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Relays);

    repo.delete("wss://relay.damus.io").expect("delete relay");
    let del_evt = rx.recv().await.expect("receive delete change event");
    assert_eq!(del_evt.table, Table::Relays);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_database_user_repo_emits_change() {
    let db = Database::open_in_memory().expect("open in memory db");
    db.migrate().expect("migrate db");

    let mut rx = db.subscribe_changes();

    let repo = soshal_db_core::repos::user::UserRepo::new(&db);
    let row = soshal_db_core::repos::user::UserRow {
        pubkey: "carol_pk".to_string(),
        npub: "npub1carol".to_string(),
        name: Some("Carol".to_string()),
        display_name: Some("Carolyn".to_string()),
        about: Some("Hello".to_string()),
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1006,
        updated_at: 1006,
        metadata_json: None,
        contact_pubkeys: "".to_string(),
        relay_list: "".to_string(),
        follower_count: 0,
        contact_count: 0,
    };

    repo.upsert(&row).expect("upsert user");
    let evt = rx.recv().await.expect("receive change event");
    assert_eq!(evt.table, Table::Profiles);
    assert_eq!(evt.affected_account.as_deref(), Some("carol_pk"));
}
