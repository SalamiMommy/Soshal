use crate::Database;
use libsql::params;

pub struct BookmarkRepo<'a> {
    db: &'a Database,
}

impl<'a> BookmarkRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<BookmarkRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE id = ?1",
            params![id],
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
        crate::query::query(
            &conn,
            "SELECT id, pubkey, event_id, created_at FROM bookmarks WHERE pubkey = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            params![pubkey, limit, offset],
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &BookmarkRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, row).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &BookmarkRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO bookmarks (id, pubkey, event_id, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET pubkey=excluded.pubkey, event_id=excluded.event_id, created_at=excluded.created_at",
            params![row.id.as_str(), row.pubkey.as_str(), row.event_id.as_str(), row.created_at],
        )
        .await?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM bookmarks WHERE id = ?1", params![id])?;
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
