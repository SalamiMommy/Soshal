use crate::Database;
use libsql::params;

pub struct GroupInviteRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupInviteRepo<'a> {
    soshal_repo_new!();

    pub fn create(&self, i: &GroupInviteRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO group_invites (id, group_id, created_by, token, max_uses, uses, expires_at, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO NOTHING",
            params![
                i.id.as_str(),
                i.group_id.as_str(),
                i.created_by.as_str(),
                i.token.as_str(),
                i.max_uses,
                i.uses,
                i.expires_at,
                i.created_at
            ],
        )?;
        Ok(())
    }

    pub fn get_by_token(
        &self,
        token: &str,
    ) -> Result<Option<GroupInviteRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, group_id, created_by, token, max_uses, uses, expires_at, created_at FROM group_invites WHERE token=?1",
            params![token],
            Self::map_row,
        )
    }

    pub fn list_by_group(
        &self,
        group_id: &str,
    ) -> Result<Vec<GroupInviteRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, created_by, token, max_uses, uses, expires_at, created_at FROM group_invites WHERE group_id=?1 ORDER BY created_at DESC LIMIT 2000",
            params![group_id],
            Self::map_row,
        )
    }

    pub fn increment_uses(&self, id: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE group_invites SET uses = uses + 1 WHERE id=?1 AND (max_uses = 0 OR uses < max_uses)",
            params![id],
        )?;
        Ok(changed > 0)
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM group_invites WHERE id=?1", params![id])?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<GroupInviteRow> {
        Ok(GroupInviteRow {
            id: r.get(0)?,
            group_id: r.get(1)?,
            created_by: r.get(2)?,
            token: r.get(3)?,
            max_uses: r.get(4)?,
            uses: r.get(5)?,
            expires_at: r.get(6)?,
            created_at: r.get(7)?,
        })
    }
}

pub struct GroupInviteRow {
    pub id: String,
    pub group_id: String,
    pub created_by: String,
    pub token: String,
    pub max_uses: i64,
    pub uses: i64,
    pub expires_at: i64,
    pub created_at: i64,
}
