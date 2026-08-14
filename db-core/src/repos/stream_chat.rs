use crate::Database;
use libsql::params;

pub struct StreamChatRepo<'a> {
    db: &'a Database,
}

impl<'a> StreamChatRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn insert(&self, m: &StreamChatRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO stream_chat (id, stream_id, pubkey, text, created_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO NOTHING",
            params![m.id.as_str(), m.stream_id.as_str(), m.pubkey.as_str(), m.text.as_str(), m.created_at],
        )?;
        Ok(())
    }

    pub fn list_by_stream(
        &self,
        stream_id: &str,
        limit: i64,
    ) -> Result<Vec<StreamChatRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, stream_id, pubkey, text, created_at FROM stream_chat WHERE stream_id=?1 ORDER BY created_at ASC LIMIT ?2",
            params![stream_id, limit],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM stream_chat WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn delete_for_stream(&self, stream_id: &str) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM stream_chat WHERE stream_id=?1",
            params![stream_id],
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<StreamChatRow> {
        Ok(StreamChatRow {
            id: r.get(0)?,
            stream_id: r.get(1)?,
            pubkey: r.get(2)?,
            text: r.get(3)?,
            created_at: r.get(4)?,
        })
    }
}

pub struct StreamChatRow {
    pub id: String,
    pub stream_id: String,
    pub pubkey: String,
    pub text: String,
    pub created_at: i64,
}
