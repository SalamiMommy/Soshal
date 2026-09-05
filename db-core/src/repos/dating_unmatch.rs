use crate::Database;
use libsql::params;

pub struct DatingUnmatchRepo<'a> {
    db: &'a Database,
}

impl<'a> DatingUnmatchRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, actor: &str, pubkey: &str, at: i64) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO dating_unmatches (actor_pubkey, pubkey, unmatched_at) VALUES (?1,?2,?3) ON CONFLICT(actor_pubkey, pubkey) DO UPDATE SET unmatched_at=excluded.unmatched_at",
            params![actor, pubkey, at],
        )?;
        Ok(())
    }

    pub fn is_unmatched(&self, actor: &str, pubkey: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM dating_unmatches WHERE actor_pubkey=?1 AND pubkey=?2",
            params![actor, pubkey],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn delete(&self, actor: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM dating_unmatches WHERE actor_pubkey=?1 AND pubkey=?2",
            params![actor, pubkey],
        )?;
        Ok(())
    }
}
