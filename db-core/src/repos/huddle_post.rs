use crate::Database;
use libsql::params;

pub struct HuddlePostRepo<'a> {
    db: &'a Database,
}

impl<'a> HuddlePostRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, p: &HuddlePostRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO huddle_posts (id, huddle_id, pubkey, content, created_at, expires_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",
            params![p.id.as_str(), p.huddle_id.as_str(), p.pubkey.as_str(), p.content.as_str(), p.created_at, p.expires_at],
        )?;
        Ok(())
    }

    pub fn list_by_huddle(
        &self,
        huddle_id: &str,
        limit: i64,
        include_expired: bool,
    ) -> Result<Vec<HuddlePostRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let sql = if include_expired {
            "SELECT id, huddle_id, pubkey, content, created_at, expires_at FROM huddle_posts WHERE huddle_id=?1 ORDER BY created_at ASC LIMIT ?2"
        } else {
            "SELECT id, huddle_id, pubkey, content, created_at, expires_at FROM huddle_posts WHERE huddle_id=?1 AND expires_at >= ?2 ORDER BY created_at ASC LIMIT ?3"
        };
        if include_expired {
            crate::query::query(&conn, sql, params![huddle_id, limit], Self::map_row)
        } else {
            let now = soshal_common_core::format::now_secs();
            crate::query::query(&conn, sql, params![huddle_id, now, limit], Self::map_row)
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM huddle_posts WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn delete_expired(&self) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM huddle_posts WHERE expires_at < ?1",
            params![soshal_common_core::format::now_secs()],
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<HuddlePostRow> {
        Ok(HuddlePostRow {
            id: r.get(0)?,
            huddle_id: r.get(1)?,
            pubkey: r.get(2)?,
            content: r.get(3)?,
            created_at: r.get(4)?,
            expires_at: r.get(5)?,
        })
    }
}

pub struct HuddlePostRow {
    pub id: String,
    pub huddle_id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: i64,
    pub expires_at: i64,
}
