//! Test-only shared lock.
//!
//! The bridge keeps one process-global in-memory `DB` handle (see `db.rs`).
//! Test modules that call `db_init` or `tmp_db` replace that global handle,
//! so they must share a single lock — otherwise parallel `cargo test`
//! execution makes modules clobber each other's database.

use std::sync::Mutex;

/// Serializes every test that touches the global `DB` handle.
pub(crate) static DB_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serializes every test that touches the process-global signer state
/// (`signer_lock`/`signer_unlock`) — parallel test modules otherwise race
/// each other's unlock/lock calls and see flaky "signer locked" assertions.
pub(crate) static SIGNER_TEST_LOCK: Mutex<()> = Mutex::new(());
