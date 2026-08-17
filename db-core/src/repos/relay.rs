use crate::Database;
use libsql::params;

pub struct RelayRepo<'a> {
    db: &'a Database,
}

impl<'a> RelayRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_url(&self, url: &str) -> Result<Option<RelayRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT url, pubkey, name, read_enabled, write_enabled, priority, last_connected_at, health_score FROM relays WHERE url = ?1",
            params![url],
            Self::map_row,
        )
    }

    pub fn get_all(&self) -> Result<Vec<RelayRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT url, pubkey, name, read_enabled, write_enabled, priority, last_connected_at, health_score FROM relays ORDER BY priority ASC",
            (),
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &RelayRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, row).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &RelayRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO relays (url, pubkey, name, read_enabled, write_enabled, priority, last_connected_at, health_score) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(url) DO UPDATE SET name=excluded.name, read_enabled=excluded.read_enabled, write_enabled=excluded.write_enabled, priority=excluded.priority, last_connected_at=excluded.last_connected_at, health_score=excluded.health_score",
            params![
                row.url.as_str(),
                row.pubkey.as_deref(),
                row.name.as_deref(),
                row.read_enabled,
                row.write_enabled,
                row.priority,
                row.last_connected_at,
                row.health_score,
            ],
        )
        .await?;
        Ok(())
    }

    pub fn upsert_batch(&self, rows: &[RelayRow]) -> Result<(), crate::error::DbError> {
        if rows.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let sql = "INSERT INTO relays (url, pubkey, name, read_enabled, write_enabled, priority, last_connected_at, health_score) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(url) DO UPDATE SET name=excluded.name, read_enabled=excluded.read_enabled, write_enabled=excluded.write_enabled, priority=excluded.priority, last_connected_at=excluded.last_connected_at, health_score=excluded.health_score";
            for row in rows {
                tx.execute(
                    sql,
                    params![
                        row.url.as_str(),
                        row.pubkey.as_deref(),
                        row.name.as_deref(),
                        row.read_enabled,
                        row.write_enabled,
                        row.priority,
                        row.last_connected_at,
                        row.health_score,
                    ],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn delete(&self, url: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM relays WHERE url = ?1", params![url])?;
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<RelayRow> {
        Ok(RelayRow {
            url: row.get(0)?,
            pubkey: row.get(1)?,
            name: row.get(2)?,
            read_enabled: row.get(3)?,
            write_enabled: row.get(4)?,
            priority: row.get(5)?,
            last_connected_at: row.get(6)?,
            health_score: row.get(7)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelayRow {
    pub url: String,
    pub pubkey: Option<String>,
    pub name: Option<String>,
    pub read_enabled: bool,
    pub write_enabled: bool,
    pub priority: i64,
    pub last_connected_at: Option<i64>,
    pub health_score: f64,
}
