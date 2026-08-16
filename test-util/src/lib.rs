//! Shared test fixtures for the Soshal workspace crates.
//!
//! Consolidates the pid-scoped temp-dir helpers and in-memory test DB
//! builders that were previously copy-pasted across ~18 modules.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use soshal_db_core::Database;

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
