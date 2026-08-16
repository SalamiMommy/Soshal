//! Rust-managed transaction graph for optimistic UI.
//!
//! Every user action is recorded as a node in a DAG (`tx_nodes` + `tx_edges`).
//! Dependencies are explicit: a comment on a post links to the create-post
//! node. When a node's network action ultimately fails, `tx_fail` walks the
//! dependent closure in reverse-topological order and reverts each local
//! side-effect (tombstone the post, undo the reaction, restore the profile),
//! so the UI can simply re-render whatever Rust dictates.

use crate::revert;
use libsql::{params, Connection};
use serde::{Deserialize, Serialize};
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
    let closure = dependent_closure(&conn, id)?;
    let rolled_back = Vec::new();
    let order = reverse_topological(&conn, &closure)?;
    for node_id in &order {
        let (kind, payload): (String, String) = soshal_db_core::query::query_first(
            &conn,
            "SELECT kind, payload_json FROM tx_nodes WHERE id = ?1",
            params![node_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("tx_fail read node: {e}"))?
        .unwrap_or_default();
        revert::revert(&conn, &kind, &payload)?;
        set_status(&conn, node_id, STATUS_ROLLED_BACK)?;
    }
    set_status(&conn, id, STATUS_FAILED)?;
    Ok(rolled_back_after(rolled_back, order, id))
}

/// All node ids reachable from `root` following child edges (root included).
fn dependent_closure(
    conn: &Connection,
    root: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let children_of = |parent: &str| -> Result<Vec<String>, String> {
        block_on(async {
            let mut stmt = conn
                .prepare("SELECT child_id FROM tx_edges WHERE parent_id = ?1")
                .await
                .map_err(|e| format!("closure prepare: {e}"))?;
            let mut rows = stmt
                .query(params![parent])
                .await
                .map_err(|e| format!("closure query: {e}"))?;
            let mut out = Vec::new();
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| format!("closure rows: {e}"))?
            {
                out.push(row.get::<String>(0).map_err(|e| e.to_string())?);
            }
            Ok(out)
        })
    };
    let mut closure = std::collections::HashSet::new();
    let mut stack = vec![root.to_string()];
    while let Some(cur) = stack.pop() {
        if !closure.insert(cur.clone()) {
            continue;
        }
        for c in children_of(&cur)? {
            stack.push(c);
        }
    }
    Ok(closure)
}

/// Orders the closure so every node comes after all of its descendants
/// (children reverted before parents).
fn reverse_topological(
    conn: &Connection,
    closure: &std::collections::HashSet<String>,
) -> Result<Vec<String>, String> {
    let children_of = |parent: &str| -> Result<Vec<String>, String> {
        block_on(async {
            let mut stmt = conn
                .prepare("SELECT child_id FROM tx_edges WHERE parent_id = ?1")
                .await
                .map_err(|e| format!("topo prepare: {e}"))?;
            let mut rows = stmt
                .query(params![parent])
                .await
                .map_err(|e| format!("topo query: {e}"))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(|e| format!("topo rows: {e}"))? {
                out.push(row.get::<String>(0).map_err(|e| e.to_string())?);
            }
            Ok(out)
        })
    };
    let mut order = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut stack: Vec<(String, bool)> = closure.iter().map(|n| (n.clone(), false)).collect();
    while let Some((node, expanded)) = stack.pop() {
        if expanded {
            if visited.insert(node.clone()) {
                order.push(node);
            }
            continue;
        }
        if visited.contains(&node) {
            continue;
        }
        stack.push((node.clone(), true));
        for c in children_of(&node)? {
            if closure.contains(&c) {
                stack.push((c, false));
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

/// Lists all nodes (for the settings screen / debugging).
pub fn tx_statuses(db: &Database) -> Result<Vec<TxNode>, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let mut stmt = conn
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
}
