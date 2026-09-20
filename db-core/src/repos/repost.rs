use crate::Database;
use libsql::params;

pub struct RepostRepo<'a> {
    db: &'a Database,
}

impl<'a> RepostRepo<'a> {
    soshal_repo_new!();

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
        let norm_pk = r.pubkey.trim();
        let norm_eid = r.event_id.trim().to_ascii_lowercase();
        tx.execute(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![norm_pk],
        )
        .await?;
        tx.execute(
            "INSERT INTO reposts (id, pubkey, event_id, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO NOTHING",
            params![r.id.trim(), norm_pk, norm_eid.as_str(), r.created_at],
        )
        .await?;
        Ok(())
    }

    pub fn count_by_event(&self, event_id: &str) -> Result<i64, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_eid = event_id.trim().to_ascii_lowercase();
        Ok(crate::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM reposts WHERE LOWER(event_id) = ?1",
            params![norm_eid.as_str()],
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
