//! Gap-fill coverage for db-core repos: block, notification (batch/unread),
//! relay (get_all/delete), settings (set/get/get_all/delete), stream_chat,
//! user (upsert_batch).

use soshal_db_core::repos::block::{BlockRepo, BlockRow};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::stream_chat::{StreamChatRepo, StreamChatRow};
use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_db_core::Database;

fn new_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

fn notif(id: &str, type_: &str, created_at: i64, is_read: bool) -> NotificationRow {
    NotificationRow {
        id: id.into(),
        pubkey: "pk".into(),
        type_: type_.into(),
        event_id: None,
        from_pubkey: None,
        content: None,
        created_at,
        is_read,
    }
}

#[test]
fn block_upsert_list_is_blocked_delete() {
    let db = new_db();
    let repo = BlockRepo::new(&db);
    repo.upsert(&BlockRow {
        pubkey: "me".into(),
        blocked_pubkey: "b1".into(),
        created_at: 10,
    })
    .unwrap();
    repo.upsert(&BlockRow {
        pubkey: "me".into(),
        blocked_pubkey: "b2".into(),
        created_at: 20,
    })
    .unwrap();
    assert!(repo.is_blocked("me", "b1").unwrap());
    assert!(!repo.is_blocked("me", "nope").unwrap());
    assert!(!repo.is_blocked("other", "b1").unwrap());
    let list = repo.list("me").unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.contains(&"b1".to_string()));
    repo.delete("me", "b1").unwrap();
    assert!(!repo.is_blocked("me", "b1").unwrap());
    assert_eq!(repo.list("me").unwrap().len(), 1);
    repo.delete_all_for("me").unwrap();
    assert!(repo.list("me").unwrap().is_empty());
}

#[test]
fn notification_upsert_batch_skips_oversized_and_conflicts_update() {
    let db = new_db();
    let repo = NotificationRepo::new(&db);
    let huge = "x".repeat(64 * 1024 + 1);
    repo.upsert_batch(&[
        notif("n1", "reply", 100, false),
        NotificationRow {
            id: "n2".into(),
            pubkey: "pk".into(),
            type_: "zap".into(),
            event_id: None,
            from_pubkey: None,
            content: Some(huge),
            created_at: 200,
            is_read: false,
        },
    ])
    .unwrap();
    let all = repo.get_unread("pk", 10).unwrap();
    assert_eq!(all.len(), 1, "oversized payload skipped");
    assert_eq!(all[0].id, "n1");
    repo.upsert_batch(&[notif("n1", "reply", 100, true)])
        .unwrap();
    let unread = repo.get_unread("pk", 10).unwrap();
    assert!(unread.is_empty(), "conflict updates is_read");
    repo.upsert_batch(&[]).unwrap();
}

#[test]
fn notification_get_unread_orders_and_limits() {
    let db = new_db();
    let repo = NotificationRepo::new(&db);
    for i in 0..5 {
        repo.upsert(&notif(&format!("n{i}"), "reply", i, false))
            .unwrap();
    }
    repo.upsert(&notif("r1", "mention", 5, false)).unwrap();
    let unread = repo.get_unread("pk", 3).unwrap();
    assert_eq!(unread.len(), 3);
    assert_eq!(unread[0].id, "r1", "newest first");
    assert_eq!(unread[0].type_, "mention");
}

#[test]
fn relay_get_all_and_delete() {
    let db = new_db();
    let repo = RelayRepo::new(&db);
    let mk = |url: &str, priority: i64| RelayRow {
        url: url.into(),
        pubkey: None,
        name: None,
        read_enabled: true,
        write_enabled: false,
        priority,
        last_connected_at: None,
        health_score: 0.5,
    };
    repo.upsert_batch(&[mk("wss://a.example", 1), mk("wss://b.example", 2)])
        .unwrap();
    repo.upsert_batch(&[]).unwrap();
    let all = repo.get_all().unwrap();
    assert_eq!(all.len(), 2);
    repo.delete("wss://a.example").unwrap();
    let all = repo.get_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].url, "wss://b.example");
}

#[test]
fn settings_set_get_get_all_delete() {
    let db = new_db();
    let repo = SettingsRepo::new(&db);
    assert!(repo.get("missing").unwrap().is_none());
    repo.set("theme", "dark").unwrap();
    assert_eq!(repo.get("theme").unwrap().unwrap(), "dark");
    repo.set("theme", "light").unwrap();
    assert_eq!(repo.get("theme").unwrap().unwrap(), "light");
    repo.set("volume", "11").unwrap();
    let all = repo.get_all().unwrap();
    assert_eq!(all.len(), 2);
    repo.delete("theme").unwrap();
    assert!(repo.get("theme").unwrap().is_none());
    assert_eq!(repo.get_all().unwrap().len(), 1);
}

#[test]
fn stream_chat_crud() {
    let db = new_db();
    let repo = StreamChatRepo::new(&db);
    let mk = |id: &str, stream: &str, created_at: i64| StreamChatRow {
        id: id.into(),
        stream_id: stream.into(),
        pubkey: "pk".into(),
        text: "hi".into(),
        created_at,
    };
    repo.insert(&mk("m1", "s1", 1)).unwrap();
    repo.insert(&mk("m2", "s1", 2)).unwrap();
    repo.insert(&mk("m3", "s2", 3)).unwrap();
    let list = repo.list_by_stream("s1", 10).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "m1", "ascending order");
    assert_eq!(repo.list_by_stream("s1", 1).unwrap().len(), 1);
    repo.delete("m1").unwrap();
    assert_eq!(repo.list_by_stream("s1", 10).unwrap().len(), 1);
    assert_eq!(repo.delete_for_stream("s2").unwrap(), 1);
    assert!(repo.list_by_stream("s2", 10).unwrap().is_empty());
}

#[test]
fn user_upsert_batch_updates() {
    let db = new_db();
    let repo = UserRepo::new(&db);
    let mk = |pk: &str, name: &str| UserRow {
        pubkey: pk.into(),
        npub: format!("npub_{pk}"),
        name: Some(name.into()),
        display_name: None,
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1,
        updated_at: 1,
        metadata_json: None,
        contact_pubkeys: "[]".into(),
        relay_list: "[]".into(),
    };
    repo.upsert_batch(&[mk("pk1", "a"), mk("pk2", "b")])
        .unwrap();
    let first = repo.get_by_pubkey("pk1").unwrap().unwrap();
    assert_eq!(first.name.as_deref(), Some("a"));
    repo.upsert_batch(&[mk("pk1", "renamed")]).unwrap();
    let first = repo.get_by_pubkey("pk1").unwrap().unwrap();
    assert_eq!(first.name.as_deref(), Some("renamed"));
    repo.ensure_exists("pk3").unwrap();
    assert!(repo.get_by_pubkey("pk3").unwrap().is_some());
}
