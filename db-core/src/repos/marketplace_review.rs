use crate::Database;
use libsql::params;

pub struct MarketplaceReviewRepo<'a> {
    db: &'a Database,
}

impl<'a> MarketplaceReviewRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, r: &MarketplaceReviewRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_reviewer = r.reviewer_pubkey.trim().to_ascii_lowercase();
        crate::query::execute(
            &conn,
            "INSERT INTO marketplace_reviews (id, listing_id, reviewer_pubkey, rating, text, created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",
            params![r.id.as_str(), r.listing_id.trim(), norm_reviewer.as_str(), r.rating, r.text.as_str(), r.created_at],
        )?;
        Ok(())
    }

    pub fn list_by_listing(
        &self,
        listing_id: &str,
        limit: i64,
    ) -> Result<Vec<MarketplaceReviewRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, listing_id, reviewer_pubkey, rating, text, created_at FROM marketplace_reviews WHERE listing_id=?1 ORDER BY created_at DESC LIMIT ?2",
            params![listing_id.trim(), limit],
            Self::map_row,
        )
    }

    pub fn average_for(&self, listing_id: &str) -> Result<Option<f64>, crate::error::DbError> {
        let conn = self.db.conn()?;
        let res: Option<Option<f64>> = crate::query::query_first(
            &conn,
            "SELECT AVG(rating) FROM marketplace_reviews WHERE listing_id=?1 HAVING COUNT(*) > 0",
            params![listing_id.trim()],
            |r| r.get::<Option<f64>>(0),
        )?;
        Ok(res.flatten())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<MarketplaceReviewRow> {
        Ok(MarketplaceReviewRow {
            id: r.get(0)?,
            listing_id: r.get(1)?,
            reviewer_pubkey: r.get(2)?,
            rating: r.get(3)?,
            text: r.get(4)?,
            created_at: r.get(5)?,
        })
    }
}

pub struct MarketplaceReviewRow {
    pub id: String,
    pub listing_id: String,
    pub reviewer_pubkey: String,
    pub rating: i64,
    pub text: String,
    pub created_at: i64,
}
