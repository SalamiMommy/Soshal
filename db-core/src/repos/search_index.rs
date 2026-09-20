use crate::Database;
use libsql::params;

pub struct SearchIndexRepo<'a> {
    db: &'a Database,
}

impl<'a> SearchIndexRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, row: &SearchIndexRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_batch_in(&tx, std::slice::from_ref(row)).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Upsert many rows in one IMMEDIATE transaction (N sequential
    /// per-row autocommit writes become one commit).
    pub fn upsert_batch(&self, rows: &[SearchIndexRow]) -> Result<(), crate::error::DbError> {
        if rows.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_batch_in(&tx, rows).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_batch_in(
        &self,
        tx: &libsql::Transaction,
        rows: &[SearchIndexRow],
    ) -> Result<(), crate::error::DbError> {
        // FTS5 rows must carry an explicit rowid: posts-backed rows map to
        // the posts rowid (matching the posts_ai trigger), everything else
        // gets a deterministic negative rowid. Auto-assigned rowids collide
        // with trigger inserts and surface as bare `constraint failed`.
        let stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO posts_fts (rowid, id, pubkey, content, subject, category) SELECT COALESCE((SELECT rowid FROM posts WHERE id = ?1), ?2), ?1, ?3, ?4, COALESCE((SELECT NULLIF(subject, '') FROM posts WHERE id = ?1), ?5), COALESCE((SELECT category FROM posts WHERE id = ?1), '')",
            )
            .await?;
        for row in rows {
            let neg = negative_rowid(&row.id);
            stmt.run(params![
                row.id.as_str(),
                neg,
                row.pubkey.as_str(),
                row.content.as_str(),
                row.subject.as_deref().unwrap_or_default(),
            ])
            .await?;
            stmt.reset();
        }
        Ok(())
    }

    pub fn search(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SearchIndexRow>, crate::error::DbError> {
        let (limit, offset) = crate::repos::clamp_page(limit, offset);
        let conn = self.db.conn()?;
        let fts_query = build_fts_query(query);
        crate::query::query(
            &conn,
            "SELECT p.id, p.pubkey, p.content, p.kind, p.created_at FROM posts_fts f JOIN posts p ON f.rowid = p.rowid WHERE p.is_deleted = 0 AND p.scheduled_at IS NULL AND posts_fts MATCH ?1 ORDER BY rank LIMIT ?2 OFFSET ?3",
            params![fts_query, limit, offset],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let post_rowid: Option<i64> = crate::query::query_first(
            &conn,
            "SELECT rowid FROM posts WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        let neg = negative_rowid(id);
        match post_rowid {
            Some(rid) => {
                crate::query::execute(
                    &conn,
                    "DELETE FROM posts_fts WHERE rowid = ?1 OR rowid = ?2",
                    params![rid, neg],
                )?;
            }
            None => {
                crate::query::execute(
                    &conn,
                    "DELETE FROM posts_fts WHERE rowid = ?1",
                    params![neg],
                )?;
            }
        }
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<SearchIndexRow> {
        Ok(SearchIndexRow {
            id: row.get(0)?,
            pubkey: row.get(1)?,
            content: row.get(2)?,
            subject: None,
            kind: row.get(3)?,
            created_at: row.get(4)?,
        })
    }
}

#[derive(Default)]
pub struct SearchIndexRow {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub subject: Option<String>,
    pub kind: i64,
    pub created_at: i64,
}

/// Deterministic negative rowid for FTS rows without a backing posts row
/// (e.g. `profile:{pubkey}`). Negative space is provably disjoint from the
/// positive posts rowids used by the posts_ai trigger, so these rows can
/// never collide with trigger inserts.
fn negative_rowid(id: &str) -> i64 {
    let h = fnv1a64(id.as_bytes()) as i64;
    -h.wrapping_abs().max(1)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Builds an FTS5 MATCH expression from a raw user query. Every term is
/// reduced to alphanumerics only, so FTS operators (`*`, `-`, `NEAR`,
/// parentheses, `NOT`) cannot reach the FTS5 parser as operators — this repo
/// stays safe even if a future caller forgets to pre-sanitize.
#[doc(hidden)]
pub fn build_fts_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len().saturating_add(16));
    let mut first = true;
    for w in query
        .split_whitespace()
        .take(soshal_content_core::fts5::MAX_FTS5_TERMS)
    {
        if w.chars().all(|c| c.is_alphanumeric())
            && w.len() <= soshal_content_core::fts5::MAX_FTS5_TERM_LEN
        {
            if w.is_empty() {
                continue;
            }
            if !first {
                out.push_str(" OR ");
            } else {
                first = false;
            }
            out.push('"');
            out.push_str(w);
            out.push('"');
            continue;
        }
        let mut cleaned =
            String::with_capacity(w.len().min(soshal_content_core::fts5::MAX_FTS5_TERM_LEN));
        for c in w
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(soshal_content_core::fts5::MAX_FTS5_TERM_LEN)
        {
            cleaned.push(c);
        }
        if cleaned.is_empty() {
            continue;
        }
        if !first {
            out.push_str(" OR ");
        } else {
            first = false;
        }
        out.push('"');
        out.push_str(&cleaned);
        out.push('"');
    }
    out
}
