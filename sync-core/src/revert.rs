//! Per-kind revert strategies for the transaction graph.
//!
//! Each revert is idempotent and local-only: the UI re-renders whatever SQLite
//! holds afterward. Unknown kinds are no-ops — a revert must never fail the
//! whole rollback chain.

use libsql::{params, params_from_iter, Connection};
use soshal_db_core::block_on;
use soshal_db_core::error::DbError;

pub const KIND_POST: &str = "post";
pub const KIND_LIKE: &str = "like";
pub const KIND_PROFILE: &str = "profile";

/// Undoes the local side-effects of one action. Idempotent.
pub fn revert(conn: &Connection, kind: &str, payload_json: &str) -> Result<(), String> {
    block_on(revert_tx(conn, kind, payload_json)).map_err(|e| e.to_string())
}

pub async fn revert_tx(conn: &Connection, kind: &str, payload_json: &str) -> Result<(), DbError> {
    let payload: serde_json::Value = serde_json::from_str(payload_json)
        .map_err(|e| DbError::Migration(format!("revert payload: {e}")))?;
    match kind {
        KIND_POST => {
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                // Tombstone, don't hard-delete: the row must stay to propagate
                // the deletion to peers and keep reply linkage intact.
                conn.execute("UPDATE posts SET is_deleted = 1 WHERE id = ?1", params![id])
                    .await?;
            }
            Ok(())
        }
        KIND_LIKE => {
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                conn.execute("DELETE FROM reactions WHERE id = ?1", params![id])
                    .await?;
            }
            Ok(())
        }
        KIND_PROFILE => {
            let Some(pubkey) = payload.get("pubkey").and_then(|v| v.as_str()) else {
                return Ok(());
            };
            let Some(prior) = payload.get("prior") else {
                return Ok(());
            };
            let mut cols = Vec::new();
            let mut vals: Vec<String> = Vec::new();
            for (i, k) in ["name", "display_name", "about", "picture"]
                .iter()
                .enumerate()
            {
                if let Some(v) = prior.get(*k).and_then(|x| x.as_str()) {
                    cols.push(format!("{k} = ?{}", i + 1));
                    vals.push(v.to_string());
                }
            }
            if cols.is_empty() {
                return Ok(());
            }
            let sql = format!(
                "UPDATE users SET {} WHERE pubkey = ?{}",
                cols.join(", "),
                vals.len() + 1
            );
            vals.push(pubkey.to_string());
            conn.execute(&sql, params_from_iter(vals.iter().map(String::as_str)))
                .await?;
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_db_core::repos::post::PostRepo;
    use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
    use soshal_db_core::repos::user::{UserRepo, UserRow};

    fn user_row(pubkey: &str, name: &str) -> UserRow {
        UserRow {
            pubkey: pubkey.to_string(),
            npub: String::new(),
            name: Some(name.to_string()),
            display_name: Some("new-display".to_string()),
            about: Some("new-about".to_string()),
            picture: Some("new-pic".to_string()),
            banner: None,
            nip05: None,
            lud16: None,
            created_at: 0,
            updated_at: 0,
            metadata_json: None,
            contact_pubkeys: String::new(),
            relay_list: String::new(),
        }
    }

    #[test]
    fn revert_profile_restores_prior_state() {
        let db = soshal_test_util::test_db();
        let pubkey = "aa".repeat(32);
        UserRepo::new(&db)
            .upsert(&user_row(&pubkey, "new-name"))
            .unwrap();
        let payload = format!(
            r#"{{"pubkey":"{pubkey}","prior":{{"name":"old-name","display_name":"old-display","about":"old-about","picture":"old-pic"}}}}"#
        );
        let conn = db.conn().unwrap();
        revert(&conn, KIND_PROFILE, &payload).unwrap();
        drop(conn);
        let user = UserRepo::new(&db).get_by_pubkey(&pubkey).unwrap().unwrap();
        assert_eq!(user.name.as_deref(), Some("old-name"));
        assert_eq!(user.display_name.as_deref(), Some("old-display"));
        assert_eq!(user.about.as_deref(), Some("old-about"));
        assert_eq!(user.picture.as_deref(), Some("old-pic"));
    }

    #[test]
    fn revert_post_tombstones_and_double_revert_is_idempotent() {
        let db = soshal_test_util::test_db();
        soshal_test_util::seed_user(&db, &"aa".repeat(32));
        PostRepo::new(&db)
            .upsert(&soshal_test_util::post_row("post-1"))
            .unwrap();
        let conn = db.conn().unwrap();
        revert(&conn, KIND_POST, r#"{"id":"post-1"}"#).unwrap();
        revert(&conn, KIND_POST, r#"{"id":"post-1"}"#).unwrap();
        drop(conn);
        let row = PostRepo::new(&db).get_by_id("post-1").unwrap().unwrap();
        assert!(row.is_deleted);
    }

    #[test]
    fn revert_like_double_revert_is_noop() {
        let db = soshal_test_util::test_db();
        soshal_test_util::seed_user(&db, &"aa".repeat(32));
        ReactionRepo::new(&db)
            .upsert(&ReactionRow {
                id: "like-1".to_string(),
                pubkey: "aa".repeat(32),
                event_id: "target-1".to_string(),
                kind: 7,
                content: Some("+".to_string()),
                created_at: 1_700_000_000,
            })
            .unwrap();
        let conn = db.conn().unwrap();
        revert(&conn, KIND_LIKE, r#"{"id":"like-1"}"#).unwrap();
        revert(&conn, KIND_LIKE, r#"{"id":"like-1"}"#).unwrap();
        drop(conn);
        assert!(ReactionRepo::new(&db)
            .get_by_event("target-1")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn revert_empty_or_missing_targets_is_noop() {
        let db = soshal_test_util::test_db();
        let conn = db.conn().unwrap();
        revert(&conn, KIND_POST, "{}").unwrap();
        revert(&conn, KIND_POST, r#"{"id":"missing"}"#).unwrap();
        revert(&conn, KIND_LIKE, "{}").unwrap();
        revert(&conn, KIND_PROFILE, r#"{"pubkey":"aa"}"#).unwrap();
        revert(&conn, KIND_PROFILE, r#"{"prior":{"name":"x"}}"#).unwrap();
        revert(&conn, KIND_PROFILE, "{}").unwrap();
        revert(&conn, "unknown-kind", r#"{"id":"x"}"#).unwrap();
    }

    #[test]
    fn revert_invalid_targets_are_noop_or_error() {
        let db = soshal_test_util::test_db();
        soshal_test_util::seed_user(&db, &"aa".repeat(32));
        PostRepo::new(&db)
            .upsert(&soshal_test_util::post_row("post-1"))
            .unwrap();
        let conn = db.conn().unwrap();
        assert!(revert(&conn, KIND_POST, "not-json").is_err());
        assert!(revert(&conn, KIND_POST, r#"{"id":42}"#).is_ok());
        assert!(revert(
            &conn,
            KIND_PROFILE,
            r#"{"pubkey":"missing","prior":{"name":"x"}}"#
        )
        .is_ok());
        assert!(revert(&conn, KIND_PROFILE, r#"{"pubkey":"aa","prior":{}}"#).is_ok());
        drop(conn);
        let row = PostRepo::new(&db).get_by_id("post-1").unwrap().unwrap();
        assert!(!row.is_deleted);
    }
}
