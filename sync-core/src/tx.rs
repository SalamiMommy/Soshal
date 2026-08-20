//! Rust-managed transaction graph for optimistic UI.
//!
//! Every user action is recorded as a node in a DAG (`tx_nodes` + `tx_edges`).
//! Dependencies are explicit: a comment on a post links to the create-post
//! node. When a node's network action ultimately fails, `tx_fail` walks the
//! dependent closure in reverse-topological order and reverts each local
//! side-effect (tombstone the post, undo the reaction, restore the profile),
//! so the UI can simply re-render whatever Rust dictates.

use crate::revert;
use libsql::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use soshal_db_core::error::DbError;
use soshal_db_core::{block_on, Database};

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_APPLIED: &str = "applied";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_ROLLED_BACK: &str = "rolled_back";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxNode {
    pub id: String,
    pub kind: String,
    pub payload_json: String,
    pub status: String,
    pub created_at: i64,
}

/// Records a new transaction node in `pending` state.
pub fn tx_begin(
    db: &Database,
    id: &str,
    kind: &str,
    payload_json: &str,
    now_secs: i64,
) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(conn.execute(
        "INSERT OR REPLACE INTO tx_nodes (id, kind, payload_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind, payload_json, STATUS_PENDING, now_secs],
    ))
    .map_err(|e| format!("tx_begin: {e}"))?;
    Ok(())
}

/// Links `child_id` to `parent_id` (child depends on parent).
pub fn tx_link(db: &Database, parent_id: &str, child_id: &str) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(conn.execute(
        "INSERT OR IGNORE INTO tx_edges (parent_id, child_id) VALUES (?1, ?2)",
        params![parent_id, child_id],
    ))
    .map_err(|e| format!("tx_link: {e}"))?;
    Ok(())
}

/// Marks a node applied after its network action succeeded.
pub fn tx_mark_applied(db: &Database, id: &str) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    set_status(&conn, id, STATUS_APPLIED)
}

/// Marks a node failed and rolls back its entire dependent closure.
/// Returns the ids rolled back (including `id` itself), newest-first.
pub fn tx_fail(db: &Database, id: &str) -> Result<Vec<String>, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let adj = load_edges(&conn)?;
    let closure = dependent_closure(&adj, id)?;
    let order = reverse_topological(&adj, &closure)?;
    if order.is_empty() {
        return Ok(Vec::new());
    }
    let rolled_back: Vec<String> = soshal_db_core::query::with_tx(&conn, |tx| async {
        let mut nodes = std::collections::HashMap::new();
        let placeholders: Vec<String> = (1..=order.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT id, kind, payload_json FROM tx_nodes WHERE id IN ({})",
            placeholders.join(", ")
        );
        let stmt = tx.prepare(&sql).await?;
        let mut rows = stmt
            .query(params_from_iter(order.iter().map(String::as_str)))
            .await?;
        while let Some(row) = rows.next().await? {
            nodes.insert(
                row.get::<String>(0)?,
                (row.get::<String>(1)?, row.get::<String>(2)?),
            );
        }
        let mut revert_errors: Vec<String> = Vec::new();
        let mut rolled_back: Vec<String> = Vec::new();
        for node_id in &order {
            let (kind, payload) = nodes.get(node_id).cloned().unwrap_or_default();
            // A per-node revert failure (e.g. malformed payload JSON) must
            // not abort the rollback chain: record it and keep rolling back
            // the remaining nodes (revert.rs: a revert must never fail the
            // whole rollback chain). Reported as an aggregated error only
            // after the chain has fully committed.
            if let Err(e) = revert::revert_tx(&tx, &kind, &payload).await {
                revert_errors.push(format!("{node_id}: {e}"));
                continue;
            }
            set_status_tx(&tx, node_id, STATUS_ROLLED_BACK).await?;
            rolled_back.push(node_id.clone());
        }
        set_status_tx(&tx, id, STATUS_FAILED).await?;
        tx.commit().await?;
        if !revert_errors.is_empty() {
            return Err(DbError::Migration(format!(
                "{} node revert(s) failed after chain rollback: {}",
                revert_errors.len(),
                revert_errors.join("; ")
            )));
        }
        Ok(rolled_back)
    })
    .map_err(|e| format!("tx_fail: {e}"))?;
    Ok(rolled_back_after(rolled_back, Vec::new(), id))
}

fn load_edges(conn: &Connection) -> Result<std::collections::HashMap<String, Vec<String>>, String> {
    block_on(async {
        let stmt = conn
            .prepare("SELECT parent_id, child_id FROM tx_edges")
            .await
            .map_err(|e| format!("edges prepare: {e}"))?;
        let mut rows = stmt
            .query(())
            .await
            .map_err(|e| format!("edges query: {e}"))?;
        let mut adj: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        while let Some(row) = rows.next().await.map_err(|e| format!("edges rows: {e}"))? {
            adj.entry(row.get::<String>(0).map_err(|e| e.to_string())?)
                .or_default()
                .push(row.get::<String>(1).map_err(|e| e.to_string())?);
        }
        Ok(adj)
    })
}

/// All node ids reachable from `root` following child edges (root included).
fn dependent_closure(
    adj: &std::collections::HashMap<String, Vec<String>>,
    root: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let mut closure = std::collections::HashSet::new();
    let mut stack = vec![root.to_string()];
    while let Some(cur) = stack.pop() {
        if !closure.insert(cur.clone()) {
            continue;
        }
        if let Some(children) = adj.get(&cur) {
            for c in children {
                stack.push(c.clone());
            }
        }
    }
    Ok(closure)
}

/// Orders the closure so every node comes after all of its descendants
/// (children reverted before parents).
fn reverse_topological(
    adj: &std::collections::HashMap<String, Vec<String>>,
    closure: &std::collections::HashSet<String>,
) -> Result<Vec<String>, String> {
    let mut order = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut in_progress = std::collections::HashSet::new();
    let mut stack: Vec<(String, bool)> = closure.iter().map(|n| (n.clone(), false)).collect();
    while let Some((node, expanded)) = stack.pop() {
        if expanded {
            in_progress.remove(&node);
            if visited.insert(node.clone()) {
                order.push(node);
            }
            continue;
        }
        if visited.contains(&node) || in_progress.contains(&node) {
            continue;
        }
        in_progress.insert(node.clone());
        stack.push((node.clone(), true));
        if let Some(children) = adj.get(&node) {
            for c in children {
                if closure.contains(c) && !visited.contains(c) && !in_progress.contains(c) {
                    stack.push((c.clone(), false));
                }
            }
        }
    }
    Ok(order)
}

fn rolled_back_after(mut rolled: Vec<String>, order: Vec<String>, failed_id: &str) -> Vec<String> {
    rolled.extend(order);
    if !rolled.contains(&failed_id.to_string()) {
        rolled.push(failed_id.to_string());
    }
    rolled
}

fn set_status(conn: &Connection, id: &str, status: &str) -> Result<(), String> {
    block_on(conn.execute(
        "UPDATE tx_nodes SET status = ?1 WHERE id = ?2",
        params![status, id],
    ))
    .map_err(|e| format!("tx status: {e}"))?;
    Ok(())
}

async fn set_status_tx(conn: &Connection, id: &str, status: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE tx_nodes SET status = ?1 WHERE id = ?2",
        params![status, id],
    )
    .await?;
    Ok(())
}

/// Lists all nodes (for the settings screen / debugging).
pub fn tx_statuses(db: &Database) -> Result<Vec<TxNode>, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let stmt = conn
            .prepare("SELECT id, kind, payload_json, status, created_at FROM tx_nodes ORDER BY created_at DESC LIMIT 200")
            .await
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(()).await.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            out.push(TxNode {
                id: row.get(0).map_err(|e| e.to_string())?,
                kind: row.get(1).map_err(|e| e.to_string())?,
                payload_json: row.get(2).map_err(|e| e.to_string())?,
                status: row.get(3).map_err(|e| e.to_string())?,
                created_at: row.get(4).map_err(|e| e.to_string())?,
            });
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_db_core::query::query_first;

    #[test]
    fn fail_post_rolls_back_dependent_like() {
        let db = soshal_test_util::test_db();
        let post_id = "post_1";
        tx_begin(&db, post_id, revert::KIND_POST, r#"{"id":"post_1"}"#, 1).unwrap();
        tx_mark_applied(&db, post_id).unwrap();

        let like_id = "like_1";
        tx_begin(
            &db,
            like_id,
            revert::KIND_LIKE,
            r#"{"id":"like_1","event_id":"post_1"}"#,
            2,
        )
        .unwrap();
        tx_link(&db, post_id, like_id).unwrap();
        tx_mark_applied(&db, like_id).unwrap();

        // Simulate the network action having hit the DB (post row + reaction row).
        {
            let conn = db.conn().unwrap();
            block_on(conn.execute(
                "INSERT INTO users (pubkey, npub) VALUES ('pk','npub1x')",
                (),
            ))
            .unwrap();
            block_on(conn.execute(
                "INSERT INTO posts (id, pubkey, content, kind, created_at) VALUES ('post_1','pk','x',1,1)",
                (),
            ))
            .unwrap();
            block_on(conn.execute(
                "INSERT INTO reactions (id, pubkey, event_id, kind, content, created_at) VALUES ('like_1','pk','post_1',7,'+','2')",
                (),
            ))
            .unwrap();
        }

        let rolled = tx_fail(&db, post_id).unwrap();
        assert_eq!(rolled, vec![like_id, post_id]);

        let conn = db.conn().unwrap();
        let deleted: i64 = query_first(
            &conn,
            "SELECT is_deleted FROM posts WHERE id='post_1'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(deleted, 1);
        let like_count: i64 = query_first(
            &conn,
            "SELECT COUNT(*) FROM reactions WHERE id='like_1'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(like_count, 0);
    }

    #[test]
    fn failed_leaf_does_not_roll_back_parent() {
        let db = soshal_test_util::test_db();
        tx_begin(&db, "p", revert::KIND_POST, r#"{"id":"p"}"#, 1).unwrap();
        tx_begin(&db, "l", revert::KIND_LIKE, r#"{"id":"l"}"#, 2).unwrap();
        tx_link(&db, "p", "l").unwrap();
        tx_mark_applied(&db, "p").unwrap();
        tx_mark_applied(&db, "l").unwrap();

        let rolled = tx_fail(&db, "l").unwrap();
        assert_eq!(rolled, vec!["l".to_string()]);

        let status_p: String = query_first(
            &db.conn().unwrap(),
            "SELECT status FROM tx_nodes WHERE id='p'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(status_p, STATUS_APPLIED);
    }

    #[test]
    fn rebegin_resets_status_and_duplicate_link_idempotent() {
        let db = soshal_test_util::test_db();

        tx_begin(&db, "t1", revert::KIND_POST, r#"{"id":"t1"}"#, 1).unwrap();
        tx_mark_applied(&db, "t1").unwrap();
        // Re-begin same id: INSERT OR REPLACE resets to pending with new payload.
        tx_begin(
            &db,
            "t1",
            revert::KIND_LIKE,
            r#"{"id":"t1","event_id":"x"}"#,
            2,
        )
        .unwrap();
        let node = tx_statuses(&db)
            .unwrap()
            .into_iter()
            .find(|n| n.id == "t1")
            .unwrap();
        assert_eq!(node.status, STATUS_PENDING);
        assert_eq!(node.kind, revert::KIND_LIKE);
        assert_eq!(node.payload_json, r#"{"id":"t1","event_id":"x"}"#);

        // Duplicate edge: INSERT OR IGNORE keeps a single row.
        tx_link(&db, "p", "c").unwrap();
        tx_link(&db, "p", "c").unwrap();
        tx_link(&db, "p", "c").unwrap();
        let edges: i64 = query_first(
            &db.conn().unwrap(),
            "SELECT COUNT(*) FROM tx_edges WHERE parent_id='p' AND child_id='c'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(edges, 1);

        // Unknown id for mark_applied: Ok no-op.
        assert!(tx_mark_applied(&db, "missing").is_ok());

        // Single-node tx (no links): tx_fail works, marks failed.
        let rolled = tx_fail(&db, "t1").unwrap();
        assert_eq!(rolled, vec!["t1".to_string()]);
        let status: String = query_first(
            &db.conn().unwrap(),
            "SELECT status FROM tx_nodes WHERE id='t1'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, STATUS_FAILED);
    }

    #[test]
    fn fail_safety_cycle_guard_diamond_and_status_limit() {
        let db = soshal_test_util::test_db();

        // Nonexistent id: no panic; empty payload fails JSON parse → Err.
        assert!(tx_fail(&db, "nope").is_err());

        // Unknown-kind node: revert is a no-op, rollback completes.
        tx_begin(&db, "u", "unknown-kind", r#"{"id":"u"}"#, 1).unwrap();
        tx_mark_applied(&db, "u").unwrap();
        assert!(tx_fail(&db, "u").is_ok());
        let status: String = query_first(
            &db.conn().unwrap(),
            "SELECT status FROM tx_nodes WHERE id='u'",
            (),
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, STATUS_FAILED);

        // Cyclic edges: closure terminates, no infinite loop.
        tx_begin(&db, "a", "unknown-kind", "{}", 1).unwrap();
        tx_begin(&db, "b", "unknown-kind", "{}", 2).unwrap();
        tx_link(&db, "a", "b").unwrap();
        tx_link(&db, "b", "a").unwrap();
        let adj = load_edges(&db.conn().unwrap()).unwrap();
        let closure = dependent_closure(&adj, "a").unwrap();
        assert_eq!(closure.len(), 2);
        assert!(closure.contains("a") && closure.contains("b"));
        assert_eq!(reverse_topological(&adj, &closure).unwrap().len(), 2);

        // Diamond: shared child appears exactly once, reverted before parents.
        tx_begin(&db, "p1", "unknown-kind", "{}", 1).unwrap();
        tx_begin(&db, "p2", "unknown-kind", "{}", 2).unwrap();
        tx_begin(&db, "c", "unknown-kind", "{}", 3).unwrap();
        tx_link(&db, "p1", "c").unwrap();
        tx_link(&db, "p2", "c").unwrap();
        tx_mark_applied(&db, "p1").unwrap();
        let rolled = tx_fail(&db, "p1").unwrap();
        assert_eq!(rolled, vec!["c".to_string(), "p1".to_string()]);
        assert_eq!(rolled.iter().filter(|id| *id == "c").count(), 1);

        // tx_statuses caps at LIMIT 200.
        for i in 0..250 {
            tx_begin(&db, &format!("s{i}"), "unknown-kind", "{}", 1000 + i).unwrap();
        }
        assert_eq!(tx_statuses(&db).unwrap().len(), 200);
    }
}
