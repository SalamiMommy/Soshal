use crate::Database;
use libsql::params;

pub struct ZapRepo<'a> {
    db: &'a Database,
}

impl<'a> ZapRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, z: &ZapRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO zaps (id, pubkey, recipient_pubkey, event_id, amount, amount_msat, content, created_at, zap_type) VALUES (?1,?2,?3,?4,?5,?5 * 1000,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET amount=excluded.amount, recipient_pubkey=excluded.recipient_pubkey",
            params![
                z.id.as_str(),
                z.pubkey.as_str(),
                z.recipient_pubkey.as_str(),
                z.event_id.as_deref(),
                z.amount,
                z.content.as_deref(),
                z.created_at,
                z.zap_type.as_str(),
            ],
        )?;
        Ok(())
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
    pub content: Option<String>,
    pub created_at: i64,
    pub zap_type: String,
}
