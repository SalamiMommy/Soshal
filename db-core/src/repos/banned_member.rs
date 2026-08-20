use crate::Database;
use libsql::params;

pub struct BannedMemberRepo<'a> {
    db: &'a Database,
}

impl<'a> BannedMemberRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, b: &BannedMemberRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO banned_members (group_id, pubkey, banned_by, reason, banned_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(group_id,pubkey) DO NOTHING",
            params![b.group_id.as_str(), b.pubkey.as_str(), b.banned_by.as_str(), b.reason.as_str(), b.banned_at],
        )?;
        Ok(())
    }

    pub fn delete(&self, group_id: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM banned_members WHERE group_id=?1 AND pubkey=?2",
            params![group_id, pubkey],
        )?;
        Ok(())
    }

    pub fn is_banned(&self, group_id: &str, pubkey: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM banned_members WHERE group_id=?1 AND pubkey=?2",
            params![group_id, pubkey],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn list_by_group(
        &self,
        group_id: &str,
    ) -> Result<Vec<BannedMemberRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT group_id, pubkey, banned_by, reason, banned_at FROM banned_members WHERE group_id=?1 ORDER BY banned_at DESC LIMIT 2000",
            params![group_id],
            Self::map_row,
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<BannedMemberRow> {
        Ok(BannedMemberRow {
            group_id: r.get(0)?,
            pubkey: r.get(1)?,
            banned_by: r.get(2)?,
            reason: r.get(3)?,
            banned_at: r.get(4)?,
        })
    }
}

pub struct BannedMemberRow {
    pub group_id: String,
    pub pubkey: String,
    pub banned_by: String,
    pub reason: String,
    pub banned_at: i64,
}
