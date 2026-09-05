use crate::Database;
use libsql::params;

pub struct ZapRepo<'a> {
    db: &'a Database,
}

impl<'a> ZapRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &ZapRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO zaps (id, pubkey, sender_pubkey, recipient_pubkey, event_id, amount, amount_msat, content, created_at, zap_type) VALUES (?1,?2,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET amount=excluded.amount, amount_msat=excluded.amount_msat, recipient_pubkey=excluded.recipient_pubkey",
            params![
                row.id.as_str(),
                row.pubkey.as_str(),
                row.recipient_pubkey.as_str(),
                row.event_id.as_deref(),
                row.amount,
                row.amount_msat,
                row.content.as_deref(),
                row.created_at,
                row.zap_type.as_str(),
            ],
        )
        .await?;
        Ok(())
    }

    pub fn upsert(&self, row: &ZapRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, row).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn sum_by_event(&self, event_id: &str) -> Result<i64, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT COALESCE(SUM(amount),0) FROM zaps WHERE event_id = ?1",
            params![event_id],
            |row| row.get(0),
        )?
        .unwrap_or(0))
    }
}

pub struct ZapRow {
    pub id: String,
    pub pubkey: String,
    pub recipient_pubkey: String,
    pub event_id: Option<String>,
    pub amount: i64,
    pub amount_msat: i64,
    pub content: Option<String>,
    pub created_at: i64,
    pub zap_type: String,
}
