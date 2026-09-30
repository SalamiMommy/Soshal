use soshal_db_core::change_bus::Table;
use soshal_db_core::observable::ObservableOptions;
use soshal_db_core::repos::message::{MessageRepo, MessageRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::Database;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn test_observable_query_initial_and_updates() {
    let db = Database::open_in_memory().expect("open db");
    db.migrate().expect("migrate");

    let count_calls = Arc::new(AtomicUsize::new(0));
    let cc = count_calls.clone();

    // Observe posts count
    let handle = db
        .observe(
            &[Table::Posts],
            ObservableOptions::default().with_debounce(Duration::from_millis(20)),
            move |db| {
                cc.fetch_add(1, Ordering::SeqCst);
                let conn = db.conn().unwrap();
                let count: i64 = soshal_db_core::query::query_first(
                    &conn,
                    "SELECT COUNT(*) FROM posts WHERE is_deleted = 0",
                    (),
                    |r| r.get(0),
                )
                .unwrap()
                .unwrap_or(0);
                Ok(count)
            },
        )
        .expect("create observable query");

    // Initial value is immediately available
    assert_eq!(handle.current(), 0);
    assert_eq!(count_calls.load(Ordering::SeqCst), 1);

    let mut rx = handle.subscribe();

    // Insert a post
    let repo = PostRepo::new(&db);
    let post1 = PostRow {
        id: "p_obs_1".to_string(),
        pubkey: "author_1".to_string(),
        content: "First observable post".to_string(),
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
    repo.upsert(&post1).expect("upsert post1");

    // Wait for the watch channel to receive the updated count
    rx.changed().await.expect("watch channel updated");
    assert_eq!(*rx.borrow(), 1);
    assert_eq!(handle.current(), 1);

    // Insert an unrelated change (Settings) — should NOT trigger re-evaluation
    let settings = SettingsRepo::new(&db);
    settings.set("theme", "oled").expect("set setting");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(*rx.borrow(), 1);
    assert_eq!(count_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_observable_query_debounces_bursts() {
    let db = Database::open_in_memory().expect("open db");
    db.migrate().expect("migrate");

    let eval_count = Arc::new(AtomicUsize::new(0));
    let ec = eval_count.clone();

    let handle = db
        .observe(
            &[Table::Messages],
            ObservableOptions::default().with_debounce(Duration::from_millis(50)),
            move |db| {
                ec.fetch_add(1, Ordering::SeqCst);
                let conn = db.conn().unwrap();
                let count: i64 = soshal_db_core::query::query_first(
                    &conn,
                    "SELECT COUNT(*) FROM messages",
                    (),
                    |r| r.get(0),
                )
                .unwrap()
                .unwrap_or(0);
                Ok(count)
            },
        )
        .expect("create observable query");

    assert_eq!(handle.current(), 0);
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);

    let mut rx = handle.subscribe();

    // Burst insert 5 messages rapidly
    let repo = MessageRepo::new(&db);
    for i in 0..5 {
        repo.upsert(&MessageRow {
            id: format!("burst_msg_{i}"),
            conversation_id: "conv:a:b".to_string(),
            pubkey: "alice".to_string(),
            content: format!("Burst {i}"),
            created_at: 1000 + i,
            tags_json: "[]".to_string(),
            reply_to: None,
            sync_status: "synced".to_string(),
            is_deleted: false,
        })
        .expect("upsert burst msg");
    }

    // Wait for the debounced re-evaluation
    rx.changed()
        .await
        .expect("watch channel updated after burst");
    assert_eq!(*rx.borrow(), 5);

    // Wait a little longer to ensure no delayed duplicate re-evaluations
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Evaluated only once for initial + once (or at most twice) for the burst, not 5 separate times!
    let total_evals = eval_count.load(Ordering::SeqCst);
    assert!(
        total_evals <= 3,
        "Burst should be debounced: got {total_evals} evaluations"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_observable_query_account_filter() {
    let db = Database::open_in_memory().expect("open db");
    db.migrate().expect("migrate");

    let eval_count = Arc::new(AtomicUsize::new(0));
    let ec = eval_count.clone();

    // Filter to only events affecting "alice"
    let handle = db
        .observe(
            &[Table::Messages],
            ObservableOptions::default()
                .with_debounce(Duration::from_millis(20))
                .with_account("alice"),
            move |db| {
                ec.fetch_add(1, Ordering::SeqCst);
                let repo = MessageRepo::new(db);
                let msgs = repo.get_conversation("conv:alice:bob", 10, None).unwrap();
                Ok(msgs.len())
            },
        )
        .expect("create observable query with account filter");

    assert_eq!(handle.current(), 0);
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);

    let mut rx = handle.subscribe();

    // Message from carol to dave (does NOT affect alice)
    let repo = MessageRepo::new(&db);
    repo.upsert(&MessageRow {
        id: "msg_carol".to_string(),
        conversation_id: "conv:carol:dave".to_string(),
        pubkey: "carol".to_string(),
        content: "Hi Dave".to_string(),
        created_at: 1000,
        tags_json: "[]".to_string(),
        reply_to: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
    })
    .expect("upsert carol msg");

    tokio::time::sleep(Duration::from_millis(50)).await;
    // Alice's query should NOT have been re-evaluated
    assert_eq!(eval_count.load(Ordering::SeqCst), 1);

    // Message from alice
    repo.upsert(&MessageRow {
        id: "msg_alice".to_string(),
        conversation_id: "conv:alice:bob".to_string(),
        pubkey: "alice".to_string(),
        content: "Hi Bob from Alice".to_string(),
        created_at: 1001,
        tags_json: "[]".to_string(),
        reply_to: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
    })
    .expect("upsert alice msg");

    rx.changed().await.expect("watch channel updated for alice");
    assert_eq!(*rx.borrow(), 1);
    assert_eq!(eval_count.load(Ordering::SeqCst), 2);
}
