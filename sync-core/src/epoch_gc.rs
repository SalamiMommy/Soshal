//! CRDT Epoch Garbage Collector (State Pruning).
//!
//! Monotonically growing CRDT tombstones are pruned once the mesh reaches vector clock consensus
//! that all active peers acknowledged deletions older than the GC threshold.
//! Flattens SQLite CRDT state, purges tombstones, and prevents long-term storage inflation.

use libsql::Connection;
use soshal_db_core::{block_on, query::query_first};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochGcSummary {
    pub domain: String,
    pub epoch_counter: u64,
    pub pruned_tombstones: u64,
    pub bytes_reclaimed: u64,
}

pub struct EpochGarbageCollector;

impl EpochGarbageCollector {
    /// Evaluates vector clock horizons across active peers and triggers tombstone purging.
    pub fn prune_tombstones_if_consensus_reached(
        conn: &Connection,
        domain: &str,
        peer_vector_clocks: &HashMap<String, u64>,
        gc_threshold_secs: u64,
    ) -> Result<EpochGcSummary, String> {
        if peer_vector_clocks.is_empty() {
            return Ok(EpochGcSummary {
                domain: domain.to_string(),
                epoch_counter: 0,
                pruned_tombstones: 0,
                bytes_reclaimed: 0,
            });
        }

        let min_horizon = peer_vector_clocks.values().copied().min().unwrap_or(0);
        // Clock floor: a zero / stale horizon (fresh peer, reset clock) must
        // not collapse the cutoff to 0 and wipe every tombstone; require a
        // real consensus horizon above the threshold.
        if min_horizon <= gc_threshold_secs {
            return Ok(EpochGcSummary {
                domain: domain.to_string(),
                epoch_counter: 0,
                pruned_tombstones: 0,
                bytes_reclaimed: 0,
            });
        }
        let cutoff_timestamp = min_horizon.saturating_sub(gc_threshold_secs);

        block_on(async {
            conn.execute("BEGIN IMMEDIATE", ())
                .await
                .map_err(|e| format!("Failed to start transaction: {}", e))?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS _crdt_epoch_boundaries (
                    domain TEXT PRIMARY KEY,
                    epoch_counter INTEGER NOT NULL DEFAULT 0,
                    vector_clock_horizon INTEGER NOT NULL DEFAULT 0,
                    pruned_tombstone_count INTEGER NOT NULL DEFAULT 0,
                    last_gc_at TEXT
                )",
                (),
            )
            .await
            .map_err(|e| e.to_string())?;

            let delete_res = conn
                .execute(
                    "DELETE FROM posts WHERE is_deleted = 1 AND created_at <= ?1",
                    [cutoff_timestamp.to_string()],
                )
                .await;

            let pruned_count = match delete_res {
                Ok(count) => count as i64,
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", ()).await;
                    return Err(format!("Tombstone deletion failed: {}", e));
                }
            };

            let update_res = conn
                .execute(
                    "INSERT INTO _crdt_epoch_boundaries (domain, epoch_counter, vector_clock_horizon, pruned_tombstone_count)
                     VALUES (?1, 1, ?2, ?3)
                     ON CONFLICT(domain) DO UPDATE SET
                        epoch_counter = epoch_counter + 1,
                        vector_clock_horizon = excluded.vector_clock_horizon,
                        pruned_tombstone_count = pruned_tombstone_count + excluded.pruned_tombstone_count,
                        last_gc_at = datetime('now')",
                    (domain, cutoff_timestamp.to_string(), pruned_count),
                )
                .await;

            if let Err(e) = update_res {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(format!("Epoch tracking update failed: {}", e));
            }

            conn.execute("COMMIT", ())
                .await
                .map_err(|e| format!("Commit failed: {}", e))?;

            Ok(pruned_count as u64)
        })
        .map(|pruned| {
            let epoch_counter_i64: i64 = query_first(
                conn,
                "SELECT epoch_counter FROM _crdt_epoch_boundaries WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .unwrap_or(None)
            .unwrap_or(1);

            EpochGcSummary {
                domain: domain.to_string(),
                epoch_counter: epoch_counter_i64 as u64,
                pruned_tombstones: pruned,
                bytes_reclaimed: pruned * 512, // Estimated memory reclaimed per row
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crdt_epoch_gc_tombstone_pruning() {
        let db = soshal_test_util::test_db();
        let conn = db.conn().unwrap();

        let mut clocks = HashMap::new();
        clocks.insert("peer1".to_string(), 100000);
        clocks.insert("peer2".to_string(), 90000);

        let summary = EpochGarbageCollector::prune_tombstones_if_consensus_reached(
            &conn,
            "posts_feed",
            &clocks,
            3600,
        )
        .unwrap();

        assert_eq!(summary.domain, "posts_feed");
        assert_eq!(summary.epoch_counter, 1);
    }
}
