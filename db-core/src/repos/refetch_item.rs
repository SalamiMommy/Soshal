use crate::Database;
use libsql::params;

pub struct RefetchItemRepo<'a> {
    db: &'a Database,
}

impl<'a> RefetchItemRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, r: &RefetchItemRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO do_not_refetch_items (id, pubkey, reason, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO NOTHING",
            params![r.id.as_str(), r.pubkey.as_deref(), r.reason.as_deref(), r.created_at],
        )?;
        Ok(())
    }

    pub fn contains(&self, id: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM do_not_refetch_items WHERE id=?1",
            params![id],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn list(&self, limit: i64) -> Result<Vec<RefetchItemRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, reason, created_at FROM do_not_refetch_items ORDER BY created_at DESC LIMIT ?1",
            params![limit],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM do_not_refetch_items WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<RefetchItemRow> {
        Ok(RefetchItemRow {
            id: r.get(0)?,
            pubkey: r.get(1)?,
            reason: r.get(2)?,
            created_at: r.get(3)?,
        })
    }
}

pub struct RefetchItemRow {
    pub id: String,
    pub pubkey: Option<String>,
    pub reason: Option<String>,
    pub created_at: i64,
}
