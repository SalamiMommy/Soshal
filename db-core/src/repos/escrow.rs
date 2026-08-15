use crate::Database;
use libsql::params;

/// Row for the `escrows` table. Status strings follow the lifecycle
/// state machine: `created → funded|shipped|disputed → completed|refunded|cancelled`.
pub struct EscrowRow {
    pub id: String,
    pub listing_id: String,
    pub buyer_pubkey: String,
    pub seller_pubkey: String,
    pub amount_msats: i64,
    pub currency: String,
    pub status: String,
    pub escrow_note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct EscrowRepo<'a> {
    db: &'a Database,
}

impl<'a> EscrowRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn create(&self, e: &EscrowRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO escrows (id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                e.id.as_str(),
                e.listing_id.as_str(),
                e.buyer_pubkey.as_str(),
                e.seller_pubkey.as_str(),
                e.amount_msats,
                e.currency.as_str(),
                e.status.as_str(),
                e.escrow_note.as_deref(),
                e.created_at,
                e.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<EscrowRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at
             FROM escrows WHERE id=?1",
            params![id],
            row_to_escrow,
        )
    }

    pub fn get_by_listing(
        &self,
        listing_id: &str,
    ) -> Result<Vec<EscrowRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at
             FROM escrows WHERE listing_id=?1 ORDER BY created_at DESC",
            params![listing_id],
            row_to_escrow,
        )
    }

    /// All escrows where `pubkey` is buyer or seller.
    /// Uses `UNION ALL` to utilize compound indices `idx_escrows_buyer_created` and `idx_escrows_seller_created`.
    pub fn get_by_participant(
        &self,
        pubkey: &str,
    ) -> Result<Vec<EscrowRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at
             FROM escrows WHERE buyer_pubkey=?1
             UNION ALL
             SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at
             FROM escrows WHERE seller_pubkey=?1 AND buyer_pubkey != ?1
             ORDER BY created_at DESC",
            params![pubkey],
            row_to_escrow,
        )
    }

    pub fn update_status(&self, id: &str, status: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE escrows SET status=?2, updated_at=?3 WHERE id=?1",
            params![id, status, soshal_common_core::format::now_secs()],
        )?;
        if changed == 0 {
            return Err(crate::error::DbError::NotFound);
        }
        Ok(())
    }

    /// Stores a dispute/referee note; touches `updated_at`.
    pub fn set_note(&self, id: &str, note: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE escrows SET escrow_note=?2, updated_at=?3 WHERE id=?1",
            params![id, note, soshal_common_core::format::now_secs()],
        )?;
        if changed == 0 {
            return Err(crate::error::DbError::NotFound);
        }
        Ok(())
    }

    /// Returns `(buyer_confirmed, seller_confirmed)` flags; `NotFound` if the escrow is missing.
    pub fn get_confirms(&self, id: &str) -> Result<(bool, bool), crate::error::DbError> {
        let conn = self.db.conn()?;
        let row = crate::query::query_first(
            &conn,
            "SELECT buyer_confirmed, seller_confirmed FROM escrows WHERE id=?1",
            params![id],
            |r| Ok((r.get::<i64>(0)? != 0, r.get::<i64>(1)? != 0)),
        )?;
        match row {
            Some(confirms) => Ok(confirms),
            None => Err(crate::error::DbError::NotFound),
        }
    }

    /// Marks the buyer's confirmation on the escrow; touches `updated_at`.
    pub fn set_buyer_confirmed(
        &self,
        id: &str,
        confirmed: bool,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE escrows SET buyer_confirmed=?2, updated_at=?3 WHERE id=?1",
            params![id, confirmed as i64, soshal_common_core::format::now_secs()],
        )?;
        if changed == 0 {
            return Err(crate::error::DbError::NotFound);
        }
        Ok(())
    }

    /// Marks the seller's confirmation on the escrow; touches `updated_at`.
    pub fn set_seller_confirmed(
        &self,
        id: &str,
        confirmed: bool,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE escrows SET seller_confirmed=?2, updated_at=?3 WHERE id=?1",
            params![id, confirmed as i64, soshal_common_core::format::now_secs()],
        )?;
        if changed == 0 {
            return Err(crate::error::DbError::NotFound);
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<EscrowRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, listing_id, buyer_pubkey, seller_pubkey, amount_msats, currency, status, escrow_note, created_at, updated_at
             FROM escrows ORDER BY created_at DESC",
            (),
            row_to_escrow,
        )
    }
}

fn row_to_escrow(r: &libsql::Row) -> libsql::Result<EscrowRow> {
    Ok(EscrowRow {
        id: r.get(0)?,
        listing_id: r.get(1)?,
        buyer_pubkey: r.get(2)?,
        seller_pubkey: r.get(3)?,
        amount_msats: r.get(4)?,
        currency: r.get(5)?,
        status: r.get(6)?,
        escrow_note: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}
