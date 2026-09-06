//! Persistence for which posts the user has already seen, per account.
//!
//! The `post_views` table is keyed by `(pubkey, post_id)` so multi-account
//! installs keep per-account seen history. Used by the popular feed to
//! exclude posts the user has already scrolled into view.

use crate::Database;
use libsql::params;
use soshal_common_core::format::now_secs;

/// Count of placeholders used by [`PostViewsRepo::mark_seen`] batches.
const BATCH_MAX: usize = 500;

/// Max seen-history rows retained per pubkey.
const MAX_SEEN_PER_USER: i64 = 5000;

pub struct PostViewsRepo<'a> {
    db: &'a Database,
}

impl<'a> PostViewsRepo<'a> {
    soshal_repo_new!();

    /// Records `post_id` as seen by `pubkey` at the current unix time.
    /// Re-seeing a post is a no-op (INSERT OR IGNORE).
    pub fn mark_seen(
        &self,
        pubkey: &str,
        post_ids: &[String],
    ) -> Result<(), crate::error::DbError> {
        if post_ids.is_empty() {
            return Ok(());
        }
        let now = now_secs();
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO post_views (pubkey, post_id, seen_at) VALUES (?1, ?2, ?3)",
                )
                .await?;
            for chunk in post_ids.chunks(BATCH_MAX) {
                for id in chunk {
                    stmt.run(params![pubkey, id.as_str(), now]).await?;
                    stmt.reset();
                }
            }
            let count: i64 = tx
                .query(
                    "SELECT COUNT(*) FROM post_views WHERE pubkey = ?1",
                    params![pubkey],
                )
                .await?
                .next()
                .await?
                .ok_or_else(|| {
                    crate::error::DbError::Migration("count query returned no row".to_string())
                })?
                .get(0)?;
            if count > MAX_SEEN_PER_USER {
                tx.execute("DELETE FROM post_views WHERE pubkey = ?1 AND rowid IN (SELECT rowid FROM post_views WHERE pubkey = ?1 ORDER BY seen_at DESC, rowid DESC LIMIT -1 OFFSET ?2)", params![pubkey, MAX_SEEN_PER_USER]).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    /// Returns the set of post ids `pubkey` has seen, for exclusion in the
    /// popular feed query.
    pub fn seen_ids(&self, pubkey: &str) -> Result<Vec<String>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT post_id FROM post_views WHERE pubkey = ?1 ORDER BY seen_at DESC LIMIT ?2",
            params![pubkey, MAX_SEEN_PER_USER],
            |row| row.get(0),
        )
    }
}
