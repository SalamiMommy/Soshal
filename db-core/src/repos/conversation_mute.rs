use crate::Database;
use libsql::params;

pub struct ConversationMuteRepo<'a> {
    db: &'a Database,
}

impl<'a> ConversationMuteRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn mute(&self, conversation_id: &str, at: i64) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO muted_conversations (conversation_id, created_at) VALUES (?1,?2) ON CONFLICT(conversation_id) DO NOTHING",
            params![conversation_id, at],
        )?;
        Ok(())
    }

    pub fn unmute(&self, conversation_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM muted_conversations WHERE conversation_id=?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    pub fn is_muted(&self, conversation_id: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM muted_conversations WHERE conversation_id=?1",
            params![conversation_id],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn list(&self) -> Result<Vec<String>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT conversation_id FROM muted_conversations ORDER BY created_at DESC",
            (),
            |r| r.get::<String>(0),
        )
    }
}
