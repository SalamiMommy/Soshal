//! Database schema migrations.

pub mod migrations;

use crate::block_on;
use libsql::Connection;
use migrations::{
    v1_create_tables, v2_create_group_messages, v3_create_social_tables, v4_create_sync_tables,
    v5_create_missing_tables, v6_purge_orphan_fts_rows, v7_rebuild_fts_triggers,
    v8_shared_keys_escrow_confirms,
};

/// Latest schema version the migration runner produces.
pub const SCHEMA_VERSION: i64 = 8;

pub fn migrate(conn: &Connection) -> Result<(), crate::error::DbError> {
    block_on(conn.execute_batch("CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));"))?;

    let current: i64 = block_on(async {
        let mut rows = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
            .await?;
        if let Some(row) = rows.next().await? {
            Ok::<i64, crate::error::DbError>(row.get::<i64>(0)?)
        } else {
            Ok(0)
        }
    })?;

    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    block_on(conn.execute_batch("BEGIN IMMEDIATE"))?;

    type StepFn = fn(&Connection) -> Result<(), crate::error::DbError>;
    let steps: &[(i64, StepFn)] = &[
        (1, |c| v1_create_tables(c).map_err(Into::into)),
        (2, |c| v2_create_group_messages(c).map_err(Into::into)),
        (3, |c| v3_create_social_tables(c).map_err(Into::into)),
        (4, |c| v4_create_sync_tables(c).map_err(Into::into)),
        (5, |c| v5_create_missing_tables(c).map_err(Into::into)),
        (6, |c| v6_purge_orphan_fts_rows(c).map_err(Into::into)),
        (7, |c| v7_rebuild_fts_triggers(c).map_err(Into::into)),
        (8, |c| v8_shared_keys_escrow_confirms(c).map_err(Into::into)),
    ];

    for &(version, step_fn) in steps {
        if version > current {
            if let Err(e) = step_fn(conn) {
                let _ = block_on(conn.execute_batch("ROLLBACK"));
                return Err(e);
            }
        }
    }

    if let Err(e) = block_on(conn.execute_batch("COMMIT")) {
        let _ = block_on(conn.execute_batch("ROLLBACK"));
        return Err(e.into());
    }
    Ok(())
}
