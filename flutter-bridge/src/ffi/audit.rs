//! Audit log FFI module
//!
//! Read access to db-core's `audit_logs` table (group moderation trail).

use flutter_rust_bridge::frb;

/// List audit log rows, newest first. Returns JSON array of
/// `{id, group_id, actor_pubkey, action, target_pubkey, details, created_at}`.
#[frb(sync, serialize)]
pub fn audit_list(limit: i64, actor_pubkey: Option<String>) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::audit_log::AuditLogRepo::new(db);
        let rows = match actor_pubkey {
            Some(actor) => repo.get_by_actor(&actor, limit)?,
            None => repo.get_all(limit)?,
        };
        Ok(rows)
    })
    .and_then(super::util::json_ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "audit")
    }

    fn insert_log(id: &str, actor: &str, action: &str, created_at: i64) {
        db::db_execute_params(
            "INSERT INTO audit_logs (id, group_id, actor_pubkey, action, target_pubkey, details, created_at) VALUES (?1,'g1',?2,?3,'t1','d1',?4)",
            &[
                id.to_string(),
                actor.to_string(),
                action.to_string(),
                created_at.to_string(),
            ],
        )
        .unwrap();
    }

    fn parse_arr(json: &str) -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn test_audit_list_empty() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("empty");
        assert_eq!(audit_list(10, None).unwrap(), "[]");
    }

    #[test]
    fn test_audit_list_newest_first_and_limit_clamp() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("order");
        insert_log("a1", "pk1", "kick", 1000);
        insert_log("a2", "pk1", "ban", 3000);
        insert_log("a3", "pk1", "mute", 2000);
        let arr = parse_arr(&audit_list(10, None).unwrap());
        assert_eq!(arr.len(), 3, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "a2");
        assert_eq!(arr[0]["action"], "ban");
        assert_eq!(arr[1]["id"], "a3");
        assert_eq!(arr[2]["id"], "a1");
        let arr = parse_arr(&audit_list(0, None).unwrap());
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "a2");
    }

    #[test]
    fn test_audit_list_actor_filter() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("actor");
        insert_log("a1", "pk1", "kick", 1000);
        insert_log("a2", "pk2", "ban", 3000);
        insert_log("a3", "pk1", "mute", 2000);
        let arr = parse_arr(&audit_list(10, Some("pk1".to_string())).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "a3");
        assert_eq!(arr[1]["id"], "a1");
    }

    #[test]
    fn test_audit_list_nullable_fields() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("nullable");
        db::db_execute_raw_test(
            "INSERT INTO audit_logs (id, group_id, actor_pubkey, action, created_at) VALUES ('a1','g1','pk1','kick',1000)"
                .to_string(),
        )
        .unwrap();
        let arr = parse_arr(&audit_list(10, None).unwrap());
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["target_pubkey"], serde_json::Value::Null);
        assert_eq!(arr[0]["details"], serde_json::Value::Null);
    }

    #[test]
    fn test_audit_list_errors_when_db_not_initialized() {
        if db::db_path().is_err() {
            assert!(audit_list(10, None)
                .unwrap_err()
                .contains("not initialized"));
        }
    }
}
