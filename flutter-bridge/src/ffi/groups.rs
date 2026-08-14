//! Groups FFI module
//!
//! NIP-29 group metadata (DB-backed), membership, and message envelopes.
//! Group chat encryption uses groups-core (`group_enc`) envelope builders;
//! keys live Rust-side only.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::group::GroupRepo;
use soshal_db_core::repos::role::{GroupRoleRepo, GroupRoleRow};

/// Group info result
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub picture: String,
    pub owner: String,
    pub members: i32,
    pub is_member: bool,
    pub role: String,
    pub created_at: u64,
}

fn row_to_group(row: &soshal_db_core::repos::group::GroupRow, viewer: Option<&str>) -> GroupInfo {
    GroupInfo {
        id: row.id.clone(),
        name: row.name.clone(),
        description: row.about.clone().unwrap_or_default(),
        picture: row.picture.clone().unwrap_or_default(),
        owner: row.pubkey.clone(),
        members: 0,
        is_member: viewer.map(|v| row.pubkey == v).unwrap_or(false),
        role: if row.pubkey == viewer.unwrap_or_default() {
            "owner".to_string()
        } else {
            "member".to_string()
        },
        created_at: row.created_at.max(0) as u64,
    }
}

fn member_count(group_id: &str) -> i32 {
    super::db::db_query_raw(format!(
        "SELECT COUNT(*) AS c FROM group_members WHERE group_id = '{}'",
        group_id.replace('\'', "''")
    ))
    .ok()
    .and_then(|json| serde_json::from_str::<Vec<serde_json::Value>>(&json).ok())
    .and_then(|rows| rows.first().and_then(|r| r["c"].as_i64()))
    .unwrap_or(0) as i32
}

/// Fetch all groups the user belongs to (DB-backed).
#[frb(sync, serialize)]
pub fn groups_fetch_groups(user_pubkey: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = GroupRepo::new(db).get_user_groups(&user_pubkey)?;
        let groups: Vec<GroupInfo> = rows
            .iter()
            .map(|r| {
                let mut g = row_to_group(r, Some(&user_pubkey));
                g.members = member_count(&r.id);
                g
            })
            .collect();
        Ok(groups)
    })
    .map(super::util::json_ok)?
}

/// Get detailed group info by id.
#[frb(sync, serialize)]
pub fn groups_get_group_info(group_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let row = GroupRepo::new(db)
            .get_by_id(&group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let mut g = row_to_group(&row, Some(&row.pubkey));
        g.members = member_count(&group_id);
        Ok(g)
    })
    .map(super::util::json_ok)?
}

/// Get group members (pubkeys + roles from the membership table).
#[frb(sync, serialize)]
pub fn groups_get_members(group_id: String) -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        Ok(GroupRepo::new(db)
            .get_members(&group_id)?
            .into_iter()
            .map(|m| m.pubkey)
            .collect())
    })
}

/// Join a group: insert the local membership row (admin approval flows stay
/// relay-side; the local row records intent).
#[frb(sync, serialize)]
pub fn groups_join(group_id: String, user_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        GroupRepo::new(db).add_member(
            &group_id,
            &user_pubkey,
            "member",
            soshal_common_core::format::now_secs(),
        )?;
        Ok(true)
    })
}

/// Leave a group: remove the local membership row.
#[frb(sync, serialize)]
pub fn groups_leave(group_id: String, user_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        GroupRepo::new(db).remove_member(&group_id, &user_pubkey)?;
        Ok(true)
    })
}

/// Sign a kind-1059 group message envelope and persist it to the local
/// `group_messages` table so offline fetches work. Returns the signed event
/// JSON (for relay publish in a later pipeline).
#[frb(sync, serialize)]
pub fn groups_post_message(group_id: String, content: String) -> Result<String, String> {
    let signed = super::signer::sign_builder(nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(1059),
        content.clone(),
    ))?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let sender = event["pubkey"].as_str().unwrap_or_default().to_string();
    let created_at = event["created_at"].as_u64().unwrap_or(0) as i64;
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO group_messages (id, group_id, sender_pubkey, content, created_at, sync_status, is_deleted) \
             VALUES (?1,?2,?3,?4,?5,0,0) \
             ON CONFLICT(id) DO UPDATE SET content=excluded.content",
            libsql::params![id, group_id, sender, content, created_at],
        ))
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(())
    })?;
    Ok(signed).into()
}

#[derive(Serialize)]
struct GroupMessageRow {
    id: String,
    group_id: String,
    sender_pubkey: String,
    content: String,
    created_at: i64,
    sync_status: i64,
    is_deleted: i64,
}

/// Fetch group chat messages from DB with pagination.
#[frb(sync, serialize)]
pub fn groups_fetch_messages(group_id: String, limit: i32, offset: i32) -> Result<String, String> {
    let limit = (limit.clamp(1, 200)) as i64;
    let offset = (offset.max(0)) as i64;
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let out = soshal_db_core::block_on(async {
            let mut stmt = conn
                .prepare(
                    "SELECT id, group_id, sender_pubkey, content, created_at, sync_status, is_deleted \
                     FROM group_messages \
                     WHERE group_id = ?1 AND is_deleted = 0 \
                     ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                )
                .await?;
            let mut rows = stmt.query(libsql::params![group_id, limit, offset]).await?;
            let mut out = Vec::with_capacity(32);
            while let Some(row) = rows.next().await? {
                out.push(GroupMessageRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    sender_pubkey: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                    sync_status: row.get(5)?,
                    is_deleted: row.get(6)?,
                });
            }
            Ok::<_, libsql::Error>(out)
        })?;
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })
}

/// Change member role (admin only).
#[frb(sync, serialize)]
pub fn groups_set_member_role(
    group_id: String,
    member_pubkey: String,
    role: String,
    admin_pubkey: String,
) -> Result<bool, String> {
    let info: GroupInfo = serde_json::from_str(&groups_get_group_info(group_id.clone())?)
        .map_err(|e| format!("parse group info: {e}"))?;
    if info.owner != admin_pubkey {
        return Err("only the group owner can change roles".to_string()).into();
    }
    super::db::with_db_result(|db| {
        GroupRepo::new(db).add_member(
            &group_id,
            &member_pubkey,
            &role,
            soshal_common_core::format::now_secs(),
        )?;
        Ok(true)
    })
}

/// Remove member (admin only).
#[frb(sync, serialize)]
pub fn groups_remove_member(
    group_id: String,
    member_pubkey: String,
    admin_pubkey: String,
) -> Result<bool, String> {
    let info: GroupInfo = serde_json::from_str(&groups_get_group_info(group_id.clone())?)
        .map_err(|e| format!("parse group info: {e}"))?;
    if info.owner != admin_pubkey {
        return Err("only the group owner can remove members".to_string()).into();
    }
    super::db::with_db_result(|db| {
        GroupRepo::new(db).remove_member(&group_id, &member_pubkey)?;
        Ok(true)
    })
}

/// List custom roles for a group, ordered by position.
#[frb(sync, serialize)]
pub fn groups_roles_list(group_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let roles = GroupRoleRepo::new(db).list(&group_id)?;
        serde_json::to_string(&roles)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create or update a custom role.
#[frb(sync, serialize)]
pub fn groups_role_upsert(
    role_id: String,
    group_id: String,
    name: String,
    color: String,
    position: i64,
    permissions: String,
) -> Result<bool, String> {
    let now = soshal_common_core::format::now_secs();
    let row = GroupRoleRow {
        id: if role_id.is_empty() {
            format!("role_{now}_{:x}", rand::random::<u32>())
        } else {
            role_id
        },
        group_id,
        name,
        color,
        position,
        permissions,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        GroupRoleRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Delete a custom role.
#[frb(sync, serialize)]
pub fn groups_role_delete(role_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        GroupRoleRepo::new(db).delete(&role_id)?;
        Ok(true)
    })
}

/// Members with their role ids (json array of {pubkey, role}).
#[frb(sync, serialize)]
pub fn groups_members_with_roles(group_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let members = GroupRepo::new(db).get_members(&group_id)?;
        let rows: Vec<serde_json::Value> = members
            .iter()
            .map(|m| {
                serde_json::json!({
                    "pubkey": m.pubkey,
                    "role": m.role,
                })
            })
            .collect();
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create a group row locally (NIP-29 event publishing is relay-side).
#[frb(sync, serialize)]
pub fn groups_create(
    group_id: String,
    name: String,
    description: String,
    picture_url: String,
    creator_pubkey: String,
) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::group::GroupRow {
        id: group_id.clone(),
        name,
        about: if description.is_empty() {
            None
        } else {
            Some(description)
        },
        picture: if picture_url.is_empty() {
            None
        } else {
            Some(picture_url)
        },
        pubkey: creator_pubkey.clone(),
        created_at: now,
        updated_at: now,
        access_type: "open".to_string(),
        relay: None,
        sync_status: "local".to_string(),
    };
    super::db::with_db_result(|db| {
        GroupRepo::new(db).upsert(&row)?;
        GroupRepo::new(db).add_member(&group_id, &creator_pubkey, "owner", now)?;
        Ok(())
    })?;
    groups_get_group_info(group_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TestDb {
        path: String,
    }

    impl TestDb {
        fn init(name: &str) -> Self {
            let path = format!(
                "{}/soshal_groups_{}_{}.db",
                std::env::temp_dir().to_string_lossy(),
                std::process::id(),
                name
            );
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(format!("{path}-wal"));
            let _ = std::fs::remove_file(format!("{path}-shm"));
            super::super::db::db_init(path.clone()).unwrap();
            TestDb { path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(format!("{}-wal", self.path));
            let _ = std::fs::remove_file(format!("{}-shm", self.path));
        }
    }

    fn insert_user(pubkey: &str) {
        super::super::db::db_execute_raw(format!(
            "INSERT INTO users (pubkey, npub, name) VALUES ('{pubkey}','npub1{pubkey}','tester') ON CONFLICT DO UPDATE SET name='tester'"
        ))
        .unwrap();
    }

    fn create_group(id: &str, owner: &str) {
        insert_user(owner);
        groups_create(
            id.to_string(),
            "Soshal Group".to_string(),
            "a test group".to_string(),
            String::new(),
            owner.to_string(),
        )
        .unwrap();
    }

    #[test]
    fn test_create_group_and_fetch() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("create");
        let owner = "a".repeat(64);

        assert_eq!(groups_fetch_groups(owner.clone()).unwrap(), "[]");

        let info = groups_create(
            "g1".to_string(),
            "Soshal Group".to_string(),
            "a test group".to_string(),
            "https://example.com/pic.png".to_string(),
            owner.clone(),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&info).unwrap();
        assert_eq!(v["name"], "Soshal Group");
        assert_eq!(v["owner"], owner);
        assert_eq!(v["role"], "owner");
        assert_eq!(v["members"], 1);

        let list = groups_fetch_groups(owner).unwrap();
        assert!(list.contains("\"g1\""));
        assert!(list.contains("a test group"));

        let detail = groups_get_group_info("g1".to_string()).unwrap();
        assert!(detail.contains("https://example.com/pic.png"));
    }

    #[test]
    fn test_get_group_info_missing_errors() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("missing");
        assert!(groups_get_group_info("nope".to_string()).is_err());
    }

    #[test]
    fn test_join_leave_and_members() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("members");
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        create_group("g2", &owner);
        insert_user(&member);

        assert!(groups_join("g2".to_string(), member.clone()).unwrap());

        let members = groups_get_members("g2".to_string()).unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&member));

        let with_roles = groups_members_with_roles("g2".to_string()).unwrap();
        assert!(with_roles.contains(&format!("\"pubkey\":\"{}\"", member)));
        assert!(with_roles.contains("\"role\":\"member\""));

        assert!(groups_leave("g2".to_string(), member.clone()).unwrap());
        let members = groups_get_members("g2".to_string()).unwrap();
        assert_eq!(members.len(), 1);
        assert!(!members.contains(&member));
    }

    #[test]
    fn test_role_change_admin_gate() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("role_gate");
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        create_group("g3", &owner);
        insert_user(&member);
        groups_join("g3".to_string(), member.clone()).unwrap();

        let denied = groups_set_member_role(
            "g3".to_string(),
            member.clone(),
            "mod".to_string(),
            member.clone(),
        );
        assert!(denied.unwrap_err().contains("only the group owner"));

        assert!(groups_set_member_role(
            "g3".to_string(),
            member.clone(),
            "mod".to_string(),
            owner.clone(),
        )
        .unwrap());
        let with_roles = groups_members_with_roles("g3".to_string()).unwrap();
        assert!(with_roles.contains("\"role\":\"mod\""));

        let denied = groups_remove_member("g3".to_string(), member.clone(), member.clone());
        assert!(denied.unwrap_err().contains("only the group owner"));
        assert!(groups_remove_member("g3".to_string(), member.clone(), owner).unwrap());

        let members = groups_get_members("g3".to_string()).unwrap();
        assert_eq!(members.len(), 1);
    }

    #[test]
    fn test_role_ops_on_missing_group_errors() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("missing_group");
        let admin = "a".repeat(64);
        assert!(groups_set_member_role(
            "missing".to_string(),
            "b".repeat(64),
            "mod".to_string(),
            admin.clone(),
        )
        .is_err());
        assert!(groups_remove_member("missing".to_string(), "b".repeat(64), admin).is_err());
    }

    #[test]
    fn test_roles_crud() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("roles_crud");
        let owner = "a".repeat(64);
        create_group("g4", &owner);

        assert_eq!(groups_roles_list("g4".to_string()).unwrap(), "[]");

        assert!(groups_role_upsert(
            String::new(),
            "g4".to_string(),
            "Admin".to_string(),
            "#ff0000".to_string(),
            1,
            "read,write".to_string(),
        )
        .unwrap());
        let list = groups_roles_list("g4".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "Admin");
        assert_eq!(arr[0]["color"], "#ff0000");
        assert_eq!(arr[0]["position"], 1);
        assert!(arr[0]["id"].as_str().unwrap().starts_with("role_"));

        let role_id = arr[0]["id"].as_str().unwrap().to_string();
        assert!(groups_role_upsert(
            role_id.clone(),
            "g4".to_string(),
            "Mod".to_string(),
            "#00ff00".to_string(),
            2,
            "read".to_string(),
        )
        .unwrap());
        let list = groups_roles_list("g4".to_string()).unwrap();
        assert!(list.contains("Mod"));
        assert!(!list.contains("Admin"));
        assert!(list.contains("\"position\":2"));

        assert!(groups_role_delete(role_id).unwrap());
        assert_eq!(groups_roles_list("g4".to_string()).unwrap(), "[]");
    }

    #[test]
    fn test_fetch_messages_empty() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("no_msgs");
        assert_eq!(
            groups_fetch_messages("g5".to_string(), 20, 0).unwrap(),
            "[]"
        );
    }

    #[test]
    fn test_post_message_and_fetch_messages() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("messages");
        let keys = soshal_nostr_core::keys::generate_keys();
        let owner = keys.public_key().to_hex();
        create_group("g5", &owner);
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let signed = groups_post_message("g5".to_string(), "hello group".to_string()).unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["kind"], 1059);
        assert_eq!(ev["pubkey"], keys.public_key().to_hex());
        assert!(ev["sig"].as_str().is_some());

        std::thread::sleep(std::time::Duration::from_millis(1100));
        groups_post_message("g5".to_string(), "second message".to_string()).unwrap();

        let msgs = groups_fetch_messages("g5".to_string(), 10, 0).unwrap();
        assert!(msgs.contains("second message"));
        assert!(msgs.contains("hello group"));

        let newest = groups_fetch_messages("g5".to_string(), 1, 0).unwrap();
        assert!(newest.contains("second message"));
        assert!(!newest.contains("hello group"));

        let page2 = groups_fetch_messages("g5".to_string(), 1, 1).unwrap();
        assert!(page2.contains("hello group"));

        let clamped = groups_fetch_messages("g5".to_string(), 0, -5).unwrap();
        assert!(clamped.contains("second message"));

        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_post_message_locked_signer_errors() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("msg_locked");
        super::super::signer::signer_lock().unwrap();
        assert!(groups_post_message("g5".to_string(), "hi".to_string()).is_err());
    }
}
