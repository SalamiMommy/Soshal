use crate::Database;
use libsql::params;

pub struct DatingUnmatchRepo<'a> {
    db: &'a Database,
}

impl<'a> DatingUnmatchRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, pubkey: &str, at: i64) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO dating_unmatches (pubkey, unmatched_at) VALUES (?1,?2) ON CONFLICT(pubkey) DO UPDATE SET unmatched_at=excluded.unmatched_at",
            params![pubkey, at],
        )?;
        Ok(())
    }

    pub fn is_unmatched(&self, pubkey: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM dating_unmatches WHERE pubkey=?1",
            params![pubkey],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn delete(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM dating_unmatches WHERE pubkey=?1",
            params![pubkey],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<String>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT pubkey FROM dating_unmatches ORDER BY unmatched_at DESC",
            (),
            |r| r.get::<String>(0),
        )
    }
}
