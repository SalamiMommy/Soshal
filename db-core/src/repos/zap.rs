use crate::Database;
use libsql::params;

pub struct ZapRepo<'a> {
    db: &'a Database,
}

impl<'a> ZapRepo<'a> {
    soshal_repo_new!();

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &ZapRow,
    ) -> Result<(), crate::error::DbError> {
        let id_clean = row.id.trim();
        let pubkey_clean = row.pubkey.trim();
        let recipient_clean = row.recipient_pubkey.trim();
        let event_id_clean = row.event_id.as_ref().map(|s| s.trim());
        tx.execute(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![pubkey_clean],
        )
        .await?;
        tx.execute(
            "INSERT INTO zaps (id, pubkey, sender_pubkey, recipient_pubkey, event_id, amount, amount_msat, content, created_at, zap_type) VALUES (?1,?2,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET amount=excluded.amount, amount_msat=excluded.amount_msat, recipient_pubkey=excluded.recipient_pubkey",
            params![
                id_clean,
                pubkey_clean,
                recipient_clean,
                event_id_clean,
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
        let clean_id = event_id.trim().to_ascii_lowercase();
        Ok(crate::query::query_first(
            &conn,
            "SELECT COALESCE(SUM(amount),0) FROM zaps WHERE LOWER(event_id) = LOWER(?1)",
            params![clean_id.as_str()],
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
