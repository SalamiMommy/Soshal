use crate::Database;
use libsql::params;

pub struct BookmarkRepo<'a> {
    db: &'a Database,
}

impl<'a> BookmarkRepo<'a> {
    soshal_repo_new!();

    pub fn get_by_id(&self, id: &str) -> Result<Option<BookmarkRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE id = ?1",
            params![id.trim().to_ascii_lowercase()],
            Self::map_row,
        )
    }

    pub fn get_user_bookmarks(
        &self,
        pubkey: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BookmarkRow>, crate::error::DbError> {
        let (limit, offset) = crate::repos::clamp_page(limit, offset);
        let conn = self.db.conn()?;
        let pk_clean = pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE pubkey = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            params![pk_clean.as_str(), limit, offset],
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &BookmarkRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, row).await?;
            tx.commit().await?;
            Ok(())
        })?;
        self.db.notify_change(
            crate::change_bus::Table::Bookmarks,
            Some(row.pubkey.trim().to_ascii_lowercase()),
        );
        Ok(())
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &BookmarkRow,
    ) -> Result<(), crate::error::DbError> {
        let normed = [normalize(row)];
        self.upsert_many_in(tx, &normed).await
    }

    /// Upsert many bookmarks for one author in one statement each for the
    /// `users` ensure, one for the rows.
    ///
    /// Ingest built a whole NIP-51 bookmark list with one `upsert_in` per e-tag,
    /// which was two statements per bookmark — the `INSERT OR IGNORE` into
    /// `users` and the row upsert. Both collapse: the user ensure is
    /// idempotent, so one covers the whole batch, and the rows go in as a
    /// single multi-row upsert.
    ///
    /// The conflict clause is shared with the single-row form so the two
    /// cannot drift on which columns win.
    ///
    /// No-op on an empty slice.
    pub async fn upsert_many_in(
        &self,
        tx: &libsql::Transaction,
        rows: &[BookmarkRow],
    ) -> Result<(), crate::error::DbError> {
        if rows.is_empty() {
            return Ok(());
        }
        let normed: Vec<BookmarkRow> = rows.iter().map(normalize).collect();

        // Key columns are lowercased, not just trimmed: `bookmark_remove` and
        // the list queries match case-insensitively, so storing a mixed-case id
        // makes the row unreachable from its own lookup.
        //
        // Batches in practice are one author, so this is one statement; the
        // distinct set keeps it correct if that ever stops holding.
        let authors: std::collections::BTreeSet<String> =
            normed.iter().map(|r| r.pubkey.clone()).collect();
        for pk in &authors {
            tx.execute(
                "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
                params![pk.as_str()],
            )
            .await?;
        }

        let n = normed.len();
        let values = (0..n)
            .map(|i| {
                format!(
                    "(?{},?{},?{},?{})",
                    i * 4 + 1,
                    i * 4 + 2,
                    i * 4 + 3,
                    i * 4 + 4
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO bookmarks (id, pubkey, event_id, created_at) VALUES {values} \
             ON CONFLICT(id) DO UPDATE SET pubkey=excluded.pubkey, \
             event_id=excluded.event_id, created_at=excluded.created_at"
        );
        let mut binds: Vec<libsql::Value> = Vec::with_capacity(n * 4);
        for row in &normed {
            binds.push(libsql::Value::from(row.id.as_str()));
            binds.push(libsql::Value::from(row.pubkey.as_str()));
            binds.push(libsql::Value::from(row.event_id.as_str()));
            binds.push(libsql::Value::from(row.created_at));
        }
        tx.execute(&sql, libsql::params_from_iter(binds)).await?;
        Ok(())
    }

    /// Drop this author's bookmarks whose target post has been soft-deleted
    /// (`is_deleted = 1`). Orphaned rows (target gone or absent) are left
    /// alone: a bookmark may legitimately arrive before its target is cached.
    pub async fn delete_for_deleted_targets_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
    ) -> Result<u64, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let res = tx
            .execute(
                "DELETE FROM bookmarks WHERE pubkey=?1 AND event_id IN (SELECT id FROM posts WHERE is_deleted = 1)",
                params![norm_pk.as_str()],
            )
            .await?;
        Ok(res)
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM bookmarks WHERE id = ?1",
            params![id.trim().to_ascii_lowercase()],
        )?;
        self.db
            .notify_change(crate::change_bus::Table::Bookmarks, None);
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<BookmarkRow> {
        Ok(BookmarkRow {
            id: row.get(0)?,
            pubkey: row.get(1)?,
            event_id: row.get(2)?,
            created_at: row.get(3)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkRow {
    pub id: String,
    pub pubkey: String,
    pub event_id: String,
    pub created_at: i64,
}

/// Lowercase the three key columns, in place.
///
/// `bookmark_remove` and the list queries match case-insensitively, so storing
/// a mixed-case id makes the row unreachable from its own lookup. Every write
/// path goes through this so the batched form cannot skip what the single-row
/// form does.
fn normalize(row: &BookmarkRow) -> BookmarkRow {
    BookmarkRow {
        id: row.id.trim().to_ascii_lowercase(),
        pubkey: row.pubkey.trim().to_ascii_lowercase(),
        event_id: row.event_id.trim().to_ascii_lowercase(),
        created_at: row.created_at,
    }
}
