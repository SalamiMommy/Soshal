use crate::Database;
use libsql::params;

/// The `INSERT … ON CONFLICT` fragment, split so the single-row and multi-row
/// forms cannot drift on the conflict clause — which is the part that carries
/// the trend-count semantics.
///
/// `count` only bumps for a strictly-newer tag usage (`last_used_at` = the
/// post's created_at), so relay replays of an already-seen post (same
/// timestamp) cannot inflate trend counts. A same-second batch of
/// genuinely-distinct posts sharing a tag under-counts by one; accepted
/// tradeoff, predates this split.
const UPSERT_HEAD: &str = "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES ";
const UPSERT_TAIL: &str = " ON CONFLICT(tag, pubkey) DO UPDATE SET \
                           last_used_at=MAX(last_used_at, excluded.last_used_at), \
                           count=count+excluded.count \
                           WHERE excluded.last_used_at > hashtags.last_used_at";

pub struct HashtagRepo<'a> {
    db: &'a Database,
}

impl<'a> HashtagRepo<'a> {
    soshal_repo_new!();

    pub fn get_by_tag(
        &self,
        tag: &str,
        pubkey: &str,
    ) -> Result<Option<HashtagRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT tag, pubkey, last_used_at, count FROM hashtags WHERE tag = ?1 AND pubkey = ?2",
            params![tag, pubkey],
            Self::map_row,
        )
    }

    pub fn get_trending(&self, limit: i64) -> Result<Vec<HashtagRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT tag, pubkey, last_used_at, count FROM hashtags ORDER BY count DESC LIMIT ?1",
            params![limit],
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &HashtagRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            &format!("{UPSERT_HEAD}(?1,?2,?3,?4){UPSERT_TAIL}"),
            params![
                row.tag.as_str(),
                row.pubkey.as_str(),
                row.last_used_at,
                row.count
            ],
        )?;
        Ok(())
    }

    /// Upsert many hashtag usages in one statement.
    ///
    /// Ingest used to call [`Self::upsert_in`] once per tag per post, inside
    /// the batch transaction — a post with 20 hashtags was 20 sequential
    /// statements. This is the same upsert collapsed into one multi-row
    /// `VALUES` list.
    ///
    /// The `WHERE excluded.last_used_at > hashtags.last_used_at` guard is the
    /// load-bearing part and it is preserved verbatim: `count` only bumps for
    /// a *strictly newer* usage, so a relay replay of an already-seen post
    /// (same timestamp) cannot inflate trend counts.
    ///
    /// A multi-row upsert is exactly equivalent to N single-row calls, not
    /// merely close: SQLite applies the conflict clause row by row against the
    /// progressively-updated table, so row *i* sees the writes of rows 0..*i*.
    /// Verified rather than assumed —
    /// `batched_hashtag_upsert_chains_strictly_increasing_rows` pins it.
    ///
    /// No-op on an empty slice.
    pub async fn upsert_many_in(
        &self,
        tx: &libsql::Transaction,
        rows: &[HashtagRow],
    ) -> Result<(), crate::error::DbError> {
        if rows.is_empty() {
            return Ok(());
        }
        let n = rows.len();
        // One `(?,?,?,?)` group per row, numbered in order of appearance, so
        // the bind vector is the rows flattened in the same order.
        let values = (0..n)
            .map(|i| {
                format!(
                    "(?{},?{},?{},?{})",
                    i * 4 + 1,
                    i * 4 + 2,
                    i * 4 + 3,
                    i * 4 + 4
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("{UPSERT_HEAD}{values}{UPSERT_TAIL}");
        let mut binds: Vec<libsql::Value> = Vec::with_capacity(n * 4);
        for row in rows {
            binds.push(libsql::Value::from(row.tag.as_str()));
            binds.push(libsql::Value::from(row.pubkey.as_str()));
            binds.push(libsql::Value::from(row.last_used_at));
            binds.push(libsql::Value::from(row.count));
        }
        tx.execute(&sql, libsql::params_from_iter(binds)).await?;
        Ok(())
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &HashtagRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            &format!("{UPSERT_HEAD}(?1,?2,?3,?4){UPSERT_TAIL}"),
            params![
                row.tag.as_str(),
                row.pubkey.as_str(),
                row.last_used_at,
                row.count
            ],
        )
        .await?;
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<HashtagRow> {
        Ok(HashtagRow {
            tag: row.get(0)?,
            pubkey: row.get(1)?,
            last_used_at: row.get(2)?,
            count: row.get(3)?,
        })
    }
}

pub struct HashtagRow {
    pub tag: String,
    pub pubkey: String,
    pub last_used_at: i64,
    pub count: i64,
}
