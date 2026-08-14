use crate::Database;
use libsql::params;

pub struct ReactionRepo<'a> {
    db: &'a Database,
}

impl<'a> ReactionRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, r: &ReactionRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO reactions (id, pubkey, event_id, kind, content, created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET content=excluded.content",
            params![
                r.id.as_str(),
                r.pubkey.as_str(),
                r.event_id.as_str(),
                r.kind,
                r.content.as_deref(),
                r.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_by_event(&self, event_id: &str) -> Result<Vec<ReactionRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, event_id, kind, content, created_at FROM reactions WHERE event_id = ?1 ORDER BY created_at DESC",
            params![event_id],
            |row| {
                Ok(ReactionRow {
                    id: row.get(0)?,
                    pubkey: row.get(1)?,
                    event_id: row.get(2)?,
                    kind: row.get(3)?,
                    content: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }
}

pub struct ReactionRow {
    pub id: String,
    pub pubkey: String,
    pub event_id: String,
    pub kind: i64,
    pub content: Option<String>,
    pub created_at: i64,
}
