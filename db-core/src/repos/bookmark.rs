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
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE LOWER(id) = LOWER(?1)",
            params![id.trim()],
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
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE LOWER(pubkey) = LOWER(?1) ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
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
        // Key columns are lowercased, not just trimmed: `bookmark_remove` and
        // the list queries match case-insensitively, so storing a mixed-case id
        // makes the row unreachable from its own lookup.
        let id_clean = row.id.trim().to_ascii_lowercase();
        let pk_clean = row.pubkey.trim().to_ascii_lowercase();
        let evt_clean = row.event_id.trim().to_ascii_lowercase();
        tx.execute(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![pk_clean.as_str()],
        )
        .await?;
        tx.execute(
            "INSERT INTO bookmarks (id, pubkey, event_id, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET pubkey=excluded.pubkey, event_id=excluded.event_id, created_at=excluded.created_at",
            params![id_clean.as_str(), pk_clean.as_str(), evt_clean.as_str(), row.created_at],
        )
        .await?;
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
                "DELETE FROM bookmarks WHERE LOWER(pubkey)=LOWER(?1) AND event_id IN (SELECT id FROM posts WHERE is_deleted = 1)",
                params![norm_pk.as_str()],
            )
            .await?;
        Ok(res)
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM bookmarks WHERE LOWER(id) = LOWER(?1)",
            params![id.trim()],
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
