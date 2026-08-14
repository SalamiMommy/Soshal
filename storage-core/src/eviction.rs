#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct EvictionEstimate {
    pub overshoot_bytes: i64,
    pub posts_to_delete: usize,
    pub posts_after_eviction: i64,
}

pub fn estimate_eviction(
    cap_bytes: i64,
    db_size_bytes: i64,
    avg_post_bytes: i64,
) -> Option<EvictionEstimate> {
    if cap_bytes <= 0 || avg_post_bytes <= 0 {
        return None;
    }
    let overshoot = db_size_bytes - cap_bytes;
    if overshoot <= 0 {
        return Some(EvictionEstimate {
            overshoot_bytes: 0,
            posts_to_delete: 0,
            posts_after_eviction: db_size_bytes,
        });
    }
    let posts_to_delete = ((overshoot / avg_post_bytes) + 1).max(1) as usize;
    Some(EvictionEstimate {
        overshoot_bytes: overshoot,
        posts_to_delete,
        posts_after_eviction: db_size_bytes - (posts_to_delete as i64 * avg_post_bytes),
    })
}

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
    fn test_estimate_eviction_cases() {
        assert!(estimate_eviction(0, 100, 10).is_none());
        assert!(estimate_eviction(100, 100, 0).is_none());

        let no_overshoot = estimate_eviction(1000, 500, 10).unwrap();
        assert_eq!(no_overshoot.overshoot_bytes, 0);
        assert_eq!(no_overshoot.posts_to_delete, 0);
        assert_eq!(no_overshoot.posts_after_eviction, 500);

        let overshoot = estimate_eviction(100, 250, 50).unwrap();
        assert_eq!(overshoot.overshoot_bytes, 150);
        assert_eq!(overshoot.posts_to_delete, 4);
        assert_eq!(overshoot.posts_after_eviction, 50);
    }

    #[test]
    fn test_execute_incremental_vacuum_empty_db() {
        let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
        let conn = db.connect().unwrap();
        assert_eq!(execute_incremental_vacuum(&conn, 0).unwrap(), 0);
        assert_eq!(execute_incremental_vacuum(&conn, 10).unwrap(), 0);
    }
}
