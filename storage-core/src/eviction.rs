/// Executes page eviction for cold media chunks & stale indexes, reclaiming SQLite database space.
pub fn execute_incremental_vacuum(
    conn: &libsql::Connection,
    pages_to_free: u32,
) -> Result<usize, libsql::Error> {
    soshal_db_core::block_on(conn.execute(
        &format!("PRAGMA incremental_vacuum({});", pages_to_free),
        (),
    ))
    .map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_incremental_vacuum_empty_db() {
        let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
        let conn = db.connect().unwrap();
        assert_eq!(execute_incremental_vacuum(&conn, 0).unwrap(), 0);
        assert_eq!(execute_incremental_vacuum(&conn, 10).unwrap(), 0);
    }
}
