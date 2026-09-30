use crate::Database;
use libsql::params;

pub struct ReactionRepo<'a> {
    db: &'a Database,
}

impl<'a> ReactionRepo<'a> {
    soshal_repo_new!();

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &ReactionRow,
    ) -> Result<(), crate::error::DbError> {
        let norm_eid = row.event_id.trim().to_ascii_lowercase();
        let norm_pk = row.pubkey.trim().to_ascii_lowercase();
        // `id` is the primary key. It used to be bound as `row.id.trim()`
        // (no lowercasing) while the dedup DELETE and the existing-row probe
        // right above compared case-insensitively — so a mixed-case id
        // deleted the old row, then inserted under a *different* primary key
        // than the one the next upsert would collide with. Normalize it here
        // so the PK, the dedup, and the ON CONFLICT all agree.
        let norm_id = row.id.trim().to_ascii_lowercase();
        let existing: Option<i64> = crate::query::query_first_async(
            tx,
            "SELECT created_at FROM reactions WHERE LOWER(event_id) = LOWER(?1) AND LOWER(pubkey) = LOWER(?2)",
            params![norm_eid.as_str(), norm_pk.as_str()],
            |r| r.get::<i64>(0),
        )
        .await?;
        if let Some(existing_ts) = existing {
            if existing_ts > row.created_at {
                return Ok(());
            }
        }
        tx.execute(
            "DELETE FROM reactions WHERE LOWER(event_id) = LOWER(?1) AND LOWER(pubkey) = LOWER(?2)",
            params![norm_eid.as_str(), norm_pk.as_str()],
        )
        .await?;
        if row.content.as_deref() == Some("-") {
            return Ok(());
        }
        tx.execute(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![norm_pk.as_str()],
        )
        .await?;
        tx.execute(
            "INSERT INTO reactions (id, pubkey, event_id, kind, content, created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET content=excluded.content",
            params![
                norm_id.as_str(),
                norm_pk.as_str(),
                norm_eid.as_str(),
                row.kind,
                row.content.as_deref(),
                row.created_at,
            ],
        )
        .await?;
        Ok(())
    }

    pub fn upsert(&self, row: &ReactionRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, row).await?;
            tx.commit().await?;
            Ok(())
        })?;
        self.db.notify_change(
            crate::change_bus::Table::Reactions,
            Some(row.pubkey.clone()),
        );
        Ok(())
    }

    pub fn get_by_event(&self, event_id: &str) -> Result<Vec<ReactionRow>, crate::error::DbError> {
        let norm_eid = event_id.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, event_id, kind, content, created_at FROM reactions WHERE LOWER(event_id) = LOWER(?1) ORDER BY created_at DESC LIMIT 2000",
            params![norm_eid.as_str()],
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
