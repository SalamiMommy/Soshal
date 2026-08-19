//! Shared test fixtures for the Soshal workspace crates.
//!
//! Consolidates the pid-scoped temp-dir helpers, in-memory test DB builders,
//! and event fixtures that were previously copy-pasted across ~25 modules.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::Keys;
use nostr::types::Timestamp;
use soshal_db_core::repos::post::PostRow;
use soshal_db_core::repos::user::UserRepo;
use soshal_db_core::Database;
use soshal_nostr_core::models::NostrEvent;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique pid-scoped temp directory for one test.
///
/// Each call returns a fresh directory (atomic counter suffix) under the
/// system temp dir, named after [label]. The directory is created.
pub fn tmp_root(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("soshal_{label}_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Unique pid-scoped temp file path (parent dir created via [tmp_root]).
pub fn tmp_path(label: &str, name: &str) -> PathBuf {
    tmp_root(label).join(name)
}

/// In-memory SQLite DB with migrations applied.
///
/// Note: `open_in_memory` pools connections; hold no `conn()` guard while
/// calling repo methods (see AGENTS.md db-core gotcha).
pub fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

/// Plain unsigned `NostrEvent` literal fixture (kind 1).
pub fn nostr_event(content: &str, tags: Vec<Vec<String>>) -> NostrEvent {
    NostrEvent {
        id: "id1".into(),
        pubkey: "pk1".into(),
        content: content.into(),
        tags,
        created_at: 100.0,
        kind: 1,
    }
}

/// `NostrEvent` literal with explicit kind.
pub fn nostr_event_kind(kind: u32, content: &str, tags: Vec<Vec<String>>) -> NostrEvent {
    NostrEvent {
        id: "id1".into(),
        pubkey: "pk1".into(),
        content: content.into(),
        tags,
        created_at: 100.0,
        kind,
    }
}

/// Signed nostr-sdk event via `EventBuilder::new(...).finalize(keys)`.
pub fn signed_event(keys: &Keys, kind: Kind, content: &str, created_at: u64) -> Event {
    EventBuilder::new(kind, content)
        .custom_created_at(Timestamp::from(created_at))
        .finalize(keys)
        .unwrap()
}

/// `signed_event` plus extra tags.
pub fn signed_event_tagged(
    keys: &Keys,
    kind: Kind,
    content: &str,
    tags: Vec<Vec<String>>,
) -> Event {
    let mut builder = EventBuilder::new(kind, content);
    for t in tags {
        builder = builder.tag(Tag::parse(t).unwrap());
    }
    builder.finalize(keys).unwrap()
}

/// Ensure a user row exists in the DB.
pub fn seed_user(db: &Database, pubkey: &str) {
    UserRepo::new(db).ensure_exists(pubkey).unwrap();
}

/// Minimal `PostRow` test row.
pub fn post_row(id: &str) -> PostRow {
    post_row_content(id, "hello")
}

/// `PostRow` test row with explicit content.
pub fn post_row_content(id: &str, content: &str) -> PostRow {
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
        rsvp_event_id: None,
    }
}

/// Byte-filled symmetric key fixture.
pub fn fill_key() -> [u8; 32] {
    [7u8; 32]
}

/// Shared serialization lock for tests touching process-global state.
pub fn test_lock() -> MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
