//! Flash-Aware SQLite WAL (Write-Ahead Logging) Management.
//! Aggregates database writes in memory ring buffers and flushes to disk in flash-friendly,
//! sequential 64KB/128KB block sizes to minimize NAND page wear and write amplification.

use libsql::Connection;
use soshal_db_core::block_on;

/// Recommended WAL block size (64 KB).
pub const FLASH_WAL_BLOCK_SIZE: usize = 64 * 1024;

/// Configures SQLite connection pragmas for flash-friendly NAND longevity.
pub fn configure_flash_pragmas(conn: &Connection) -> Result<(), libsql::Error> {
    block_on(conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA page_size = 8192;
        PRAGMA wal_autocheckpoint = 1000;
        PRAGMA trusted_schema = OFF;
        PRAGMA secure_delete = FAST;
        PRAGMA auto_vacuum = INCREMENTAL;
        ",
    ))?;
    Ok(())
}

/// Aggregated write buffer holding pending SQL batch items before sequential flush.
#[derive(Debug, Default)]
pub struct FlashWalFlusher {
    pub pending_sqls: Vec<String>,
    pub current_bytes: usize,
}

impl FlashWalFlusher {
    pub fn new() -> Self {
        Self {
            pending_sqls: Vec::new(),
            current_bytes: 0,
        }
    }

    /// Appends a SQL statement to the pending write buffer.
    pub fn push_sql(&mut self, sql: &str) {
        self.current_bytes += sql.len();
        self.pending_sqls.push(sql.to_string());
    }

    /// Checks if buffer reached the flash block size threshold.
    pub fn should_flush(&self) -> bool {
        self.current_bytes >= FLASH_WAL_BLOCK_SIZE
    }

    /// Flushes aggregated pending SQL batch inside a single transactional block.
    pub fn flush_to_db(&mut self, conn: &Connection) -> Result<usize, libsql::Error> {
        if self.pending_sqls.is_empty() {
            return Ok(0);
        }

        let count = self.pending_sqls.len();
        block_on(conn.execute_batch("BEGIN IMMEDIATE;"))?;
        for sql in &self.pending_sqls {
            let _ = block_on(conn.execute_batch(sql));
        }
        block_on(conn.execute_batch("COMMIT;"))?;

        self.pending_sqls.clear();
        self.current_bytes = 0;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flash_wal_flusher() {
        let mut flusher = FlashWalFlusher::new();
        flusher.push_sql("CREATE TABLE IF NOT EXISTS test_wal (id INT);");
        assert_eq!(flusher.pending_sqls.len(), 1);

        let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
        let conn = db.connect().unwrap();
        configure_flash_pragmas(&conn).unwrap();

        let flushed = flusher.flush_to_db(&conn).unwrap();
        assert_eq!(flushed, 1);
        assert_eq!(flusher.pending_sqls.len(), 0);
    }

    #[test]
    fn test_should_flush_boundaries() {
        let mut flusher = FlashWalFlusher::new();
        assert!(!flusher.should_flush());

        flusher.push_sql(&"x".repeat(FLASH_WAL_BLOCK_SIZE - 1));
        assert!(!flusher.should_flush());

        flusher.push_sql("x");
        assert!(flusher.should_flush());

        let mut at = FlashWalFlusher::new();
        at.push_sql(&"x".repeat(FLASH_WAL_BLOCK_SIZE));
        assert!(at.should_flush());

        let mut above = FlashWalFlusher::new();
        above.push_sql(&"x".repeat(FLASH_WAL_BLOCK_SIZE + 1));
        assert!(above.should_flush());
    }

    #[test]
    fn test_flush_resets_watermark_state() {
        let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
        let conn = db.connect().unwrap();
        let mut flusher = FlashWalFlusher::new();
        flusher.push_sql(&"x".repeat(FLASH_WAL_BLOCK_SIZE));
        assert!(flusher.should_flush());
        assert_eq!(flusher.flush_to_db(&conn).unwrap(), 1);
        assert!(!flusher.should_flush());
        assert_eq!(flusher.current_bytes, 0);
    }

    #[test]
    fn test_configure_flash_pragmas_enables_wal() {
        let path =
            std::env::temp_dir().join(format!("soshal-flash-wal-test-{}", std::process::id()));
        let db = soshal_db_core::block_on(libsql::Builder::new_local(&path).build()).unwrap();
        let conn = db.connect().unwrap();
        configure_flash_pragmas(&conn).unwrap();
        let journal_mode = soshal_db_core::block_on(async {
            let mut stmt = conn.prepare("PRAGMA journal_mode;").await.unwrap();
            let mut rows = stmt.query(()).await.unwrap();
            match rows.next().await.unwrap().unwrap().get_value(0).unwrap() {
                libsql::Value::Text(s) => s,
                _ => String::new(),
            }
        });
        assert_eq!(journal_mode, "wal");
        drop(conn);
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
