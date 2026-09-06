use crate::Database;
use libsql::params;

pub struct GroupJoinRequestRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupJoinRequestRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, r: &GroupJoinRequestRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO group_join_requests (group_id, pubkey, status, requested_at) VALUES (?1,?2,?3,?4) ON CONFLICT(group_id,pubkey) DO UPDATE SET status=excluded.status, requested_at=excluded.requested_at",
            params![r.group_id.as_str(), r.pubkey.as_str(), r.status.as_str(), r.requested_at],
        )?;
        Ok(())
    }

    pub fn list_by_group(
        &self,
        group_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<GroupJoinRequestRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        match status {
            Some(s) => crate::query::query(
                &conn,
                "SELECT group_id, pubkey, status, requested_at FROM group_join_requests WHERE group_id=?1 AND status=?2 ORDER BY requested_at ASC LIMIT 2000",
                params![group_id, s],
                Self::map_row,
            ),
            None => crate::query::query(
                &conn,
                "SELECT group_id, pubkey, status, requested_at FROM group_join_requests WHERE group_id=?1 ORDER BY requested_at ASC LIMIT 2000",
                params![group_id],
                Self::map_row,
            ),
        }
    }

    pub fn get(
        &self,
        group_id: &str,
        pubkey: &str,
    ) -> Result<Option<GroupJoinRequestRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT group_id, pubkey, status, requested_at FROM group_join_requests WHERE group_id=?1 AND pubkey=?2",
            params![group_id, pubkey],
            Self::map_row,
        )
    }

    pub fn delete(&self, group_id: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_join_requests WHERE group_id=?1 AND pubkey=?2",
            params![group_id, pubkey],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<GroupJoinRequestRow> {
        Ok(GroupJoinRequestRow {
            group_id: r.get(0)?,
            pubkey: r.get(1)?,
            status: r.get(2)?,
            requested_at: r.get(3)?,
        })
    }
}

pub struct GroupJoinRequestRow {
    pub group_id: String,
    pub pubkey: String,
    pub status: String,
    pub requested_at: i64,
}
