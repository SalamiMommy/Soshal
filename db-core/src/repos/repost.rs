use crate::Database;
use libsql::params;

pub struct RepostRepo<'a> {
    db: &'a Database,
}

impl<'a> RepostRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, r: &RepostRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, r).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        r: &RepostRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO reposts (id, pubkey, event_id, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO NOTHING",
            params![r.id.as_str(), r.pubkey.as_str(), r.event_id.as_str(), r.created_at],
        )
        .await?;
        Ok(())
    }

    pub fn count_by_event(&self, event_id: &str) -> Result<i64, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM reposts WHERE event_id = ?1",
            params![event_id],
            |row| row.get(0),
        )?
        .unwrap_or(0))
    }
}

pub struct RepostRow {
    pub id: String,
    pub pubkey: String,
    pub event_id: String,
    pub created_at: i64,
}
