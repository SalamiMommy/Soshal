//! Groups FFI module
//!
//! NIP-29 group metadata (DB-backed), membership, and message envelopes.
//! Group chat encryption uses groups-core (`group_enc`) envelope builders;
//! keys live Rust-side only.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::banned_member::BannedMemberRepo;
use soshal_db_core::repos::group::GroupRepo;
use soshal_db_core::repos::role::{GroupRoleRepo, GroupRoleRow};
use soshal_groups_core::group_enc::seal::group_message_envelope;

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
    pub is_private: bool,
}

fn row_to_group(
    row: &soshal_db_core::repos::group::GroupRow,
    viewer: Option<&str>,
    is_member: bool,
) -> GroupInfo {
    let is_owner = viewer
        .map(|v| row.pubkey.eq_ignore_ascii_case(v))
        .unwrap_or(false);
    GroupInfo {
        id: row.id.clone(),
        name: row.name.clone(),
        description: row.about.clone().unwrap_or_default(),
        picture: row.picture.clone().unwrap_or_default(),
        owner: row.pubkey.clone(),
        members: 0,
        is_member: is_owner || is_member,
        role: if is_owner {
            "owner".to_string()
        } else if is_member {
            "member".to_string()
        } else {
            String::new()
        },
        created_at: row.created_at.max(0) as u64,
        is_private: row.access_type == "private" || row.password_hash.is_some(),
    }
}

/// Read a group shared key, healing legacy plaintext rows into `seal1:`
/// at-rest envelopes on first access. Returns the raw hex key for envelope
/// construction. A key stored bare (pre-seal) is re-written sealed in place so
/// the on-disk copy no longer carries the raw material in the clear.
pub(crate) fn shared_key_for_group(
    db: &soshal_db_core::Database,
    group_id: &str,
) -> Result<Option<String>, soshal_db_core::error::DbError> {
    let repo = GroupRepo::new(db);
    let stored = repo.get_shared_key(group_id)?;
    let Some(k) = stored else {
        return Ok(None);
    };
    match k.strip_prefix("seal1:") {
        Some(sealed) => {
            let plain = soshal_crypto_core::at_rest::open_at_rest(
                &super::signer::signer_at_rest_key()
                    .map_err(soshal_db_core::error::DbError::Migration)?,
                sealed,
            )
            .map_err(soshal_db_core::error::DbError::Migration)?;
            Ok(Some(hex::encode(plain)))
        }
        None => {
            // Legacy plaintext row. Only heal when the value parses as hex
            // (the `key_hex` column contract); otherwise pass through raw so
            // nothing breaks on historical non-hex entries.
            if let Ok(bytes) = hex::decode(&k) {
                let sealed = format!(
                    "seal1:{}",
                    soshal_crypto_core::at_rest::seal_at_rest(
                        &super::signer::signer_at_rest_key()
                            .map_err(soshal_db_core::error::DbError::Migration)?,
                        &bytes,
                    )
                    .map_err(soshal_db_core::error::DbError::Migration)?
                );
                repo.set_shared_key(group_id, &sealed)?;
            }
            Ok(Some(k))
        }
    }
}

/// Fetch all groups the user belongs to (DB-backed).
#[frb(sync, serialize)]
pub fn groups_fetch_groups(user_pubkey: String, audience: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let rows = repo.get_user_groups(&user_pubkey)?;
        let owners = super::identity::resolve_audience_authors(&audience)
            .map_err(soshal_db_core::error::DbError::Migration)?;
        let rows: Vec<_> = match &owners {
            Some(a) => {
                let set: std::collections::HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
                rows.into_iter()
                    .filter(|r| set.contains(r.pubkey.as_str()))
                    .collect()
            }
            None => rows,
        };
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let counts = repo.member_count_many(&ids)?;
        let groups: Vec<GroupInfo> = rows
            .iter()
            .map(|r| {
                let mut g = row_to_group(r, Some(&user_pubkey), true);
                g.members = counts.get(&r.id).copied().unwrap_or(0) as i32;
                g
            })
            .collect();
        Ok(groups)
    })
    .map(super::util::json_ok)?
}

fn get_group_info_with_viewer(group_id: &str, viewer: Option<&str>) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let row = repo
            .get_by_id(group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let is_member = match viewer {
            Some(pk) => repo.is_member(group_id, pk).unwrap_or(false),
            None => false,
        };
        let mut g = row_to_group(&row, viewer, is_member);
        let counts = repo.member_count_many(&[group_id.to_string()])?;
        g.members = counts.get(group_id).copied().unwrap_or(0) as i32;
        Ok(g)
    })
    .map(super::util::json_ok)?
}

/// Get detailed group info by id.
#[frb(sync, serialize)]
pub fn groups_get_group_info(group_id: String) -> Result<String, String> {
    let viewer = super::signer::signer_pubkey().ok();
    get_group_info_with_viewer(&group_id, viewer.as_deref())
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
/// relay-side; the local row records intent). For private groups, verifies
/// the provided password before joining.
#[frb(sync, serialize)]
pub fn groups_join(
    group_id: String,
    user_pubkey: String,
    password: Option<String>,
) -> Result<bool, String> {
    super::signer::require_identity(&user_pubkey)?;
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let group = repo
            .get_by_id(&group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        if group.access_type == "private" || group.password_hash.is_some() {
            let stored_hash = group.password_hash.as_deref().unwrap_or("");
            if stored_hash.is_empty() {
                return Err(soshal_db_core::error::DbError::Oversized(
                    "private community has no password on record; cannot verify access".to_string(),
                ));
            }
            let candidate = password.as_deref().unwrap_or("").trim();
            if candidate.is_empty() {
                return Err(soshal_db_core::error::DbError::Oversized(
                    "password required for private community".to_string(),
                ));
            }
            if !soshal_groups_core::access::verify_community_password(candidate, stored_hash) {
                return Err(soshal_db_core::error::DbError::Oversized(
                    "incorrect community password".to_string(),
                ));
            }
        }
        repo.add_member(
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
    super::signer::require_identity(&user_pubkey)?;
    super::db::with_db_result(|db| {
        GroupRepo::new(db).remove_member(&group_id, &user_pubkey)?;
        Ok(true)
    })
}

/// Sign a kind-1059 group message envelope and persist it to the local
/// `group_messages` table so offline fetches work. [room_id] scopes the
/// message to a themed room (empty = the default `#general` room). Returns
/// the signed event JSON (for relay publish in a later pipeline).
#[frb(sync, serialize)]
pub fn groups_post_message(
    group_id: String,
    room_id: String,
    content: String,
) -> Result<String, String> {
    let content = super::db::with_db_result(|db| {
        let key = shared_key_for_group(db, &group_id)?;
        Ok(match key {
            Some(k) => group_message_envelope(&content, Some(&k))
                .map_err(soshal_db_core::error::DbError::Migration)?,
            None => content,
        })
    })?;
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
        // Enforce membership + ban state before a message may be stored:
        // `is_banned` alone was dead code before — a banned member could
        // keep posting and every client would render their messages.
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&group_id)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&sender))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&group_id, &sender)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a member of this group".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&group_id, &sender)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        let conn = db.conn()?;
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO group_messages (id, group_id, room_id, sender_pubkey, content, created_at, sync_status, is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,0,0) \
             ON CONFLICT(id) DO UPDATE SET content=excluded.content",
            libsql::params![id, group_id, room_id, sender, content, created_at],
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

/// Fetch group chat messages from DB with pagination, scoped to a room
/// (empty [room_id] = the default `#general` room).
#[frb(sync, serialize)]
pub fn groups_fetch_messages(
    group_id: String,
    room_id: String,
    limit: i32,
    offset: i32,
) -> Result<String, String> {
    let limit = (limit.clamp(1, 200)) as i64;
    let offset = (offset.max(0)) as i64;
    super::db::with_db_result(|db| {
        let group_repo = GroupRepo::new(db);
        if let Some(group) = group_repo.get_by_id(&group_id)? {
            let viewer = super::signer::signer_pubkey().ok();
            if let Some(ref pk) = viewer {
                if BannedMemberRepo::new(db).is_banned(&group_id, pk)? {
                    return Ok("[]".to_string());
                }
            }
            let is_private = group.access_type == "private" || group.password_hash.is_some();
            if is_private {
                let is_member = match &viewer {
                    Some(pk) => {
                        group.pubkey.eq_ignore_ascii_case(pk)
                            || group_repo.is_member(&group_id, pk).unwrap_or(false)
                    }
                    None => false,
                };
                if !is_member {
                    return Ok("[]".to_string());
                }
            }
        }
        let conn = db.conn()?;
        let out = soshal_db_core::block_on(async {
            let stmt = conn
                .prepare(
                    "SELECT id, group_id, sender_pubkey, content, created_at, sync_status, is_deleted \
                     FROM group_messages \
                     WHERE group_id = ?1 AND room_id = ?2 AND is_deleted = 0 \
                     ORDER BY created_at DESC LIMIT ?3 OFFSET ?4",
                )
                .await?;
            let mut rows = stmt
                .query(libsql::params![group_id, room_id, limit, offset])
                .await?;
            let mut out = Vec::with_capacity(limit as usize);
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
        Ok(super::util::json_ok_or_empty(&out))
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
    require_owner(&group_id, &admin_pubkey)?;
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
    require_owner(&group_id, &admin_pubkey)?;
    if member_pubkey == admin_pubkey {
        return Err("cannot remove group owner".to_string()).into();
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
    #[derive(Serialize)]
    struct MemberRole<'a> {
        pubkey: &'a str,
        role: &'a str,
    }
    super::db::with_db_result(|db| {
        let members = GroupRepo::new(db).get_members(&group_id)?;
        let rows: Vec<MemberRole> = members
            .iter()
            .map(|m| MemberRole {
                pubkey: &m.pubkey,
                role: &m.role,
            })
            .collect();
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Verify `actor` is the group owner; error otherwise (admin-gate helper).
fn require_owner(group_id: &str, actor: &str) -> Result<(), String> {
    let owner = super::db::with_db_result(|db| {
        GroupRepo::new(db)
            .get_by_id(group_id)?
            .map(|r| r.pubkey)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    if owner != actor {
        return Err("only the group owner can do that".to_string());
    }
    super::signer::require_identity(actor)?;
    Ok(())
}

/// List themed rooms of a group (JSON rows).
#[frb(sync, serialize)]
pub fn groups_rooms_list(group_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::room::GroupRoomRepo::new(db).list(&group_id)?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create a themed room; returns its id.
#[frb(sync, serialize)]
pub fn groups_rooms_create(
    group_id: String,
    name: String,
    topic: String,
    emoji: String,
    color: String,
    creator: String,
) -> Result<String, String> {
    require_owner(&group_id, &creator)?;
    let now = soshal_common_core::format::now_secs();
    let id = format!("room_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::room::GroupRoomRow {
        id: id.clone(),
        group_id,
        name,
        topic,
        emoji,
        color,
        position: 0,
        created_by: creator,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::room::GroupRoomRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Update a room's name/topic/emoji/color (owner only).
#[frb(sync, serialize)]
pub fn groups_rooms_update(
    room_id: String,
    group_id: String,
    name: String,
    topic: String,
    emoji: String,
    color: String,
    actor: String,
) -> Result<bool, String> {
    require_owner(&group_id, &actor)?;
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::room::GroupRoomRepo::new(db);
        let existing = repo
            .get(&room_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let row = soshal_db_core::repos::room::GroupRoomRow {
            id: room_id,
            group_id,
            name,
            topic,
            emoji,
            color,
            position: existing.position,
            created_by: existing.created_by,
            created_at: existing.created_at,
        };
        repo.upsert(&row)?;
        Ok(true)
    })
}

/// Delete a room (owner only); its messages stay (room_id blanks out).
#[frb(sync, serialize)]
pub fn groups_rooms_delete(room_id: String, actor: String) -> Result<bool, String> {
    let group_id = super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::room::GroupRoomRepo::new(db);
        repo.get(&room_id)?
            .map(|r| r.group_id)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    require_owner(&group_id, &actor)?;
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::room::GroupRoomRepo::new(db);
        repo.delete(&room_id)?;
        let conn = db.conn()?;
        soshal_db_core::block_on(conn.execute(
            "UPDATE group_messages SET room_id = '' WHERE room_id = ?1",
            libsql::params![room_id],
        ))
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(true)
    })
}

/// Toggle an emoji reaction on a post in a room; returns true when added, false when removed.
#[frb(sync, serialize)]
pub fn groups_rooms_react(
    group_id: String,
    room_id: String,
    message_id: String,
    pubkey: String,
    emoji: String,
) -> Result<bool, String> {
    if emoji.is_empty() || emoji.chars().count() > 16 {
        return Err("invalid reaction emoji".into());
    }
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| {
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&group_id)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&pubkey))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&group_id, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a member of this group".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&group_id, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        soshal_db_core::repos::room::GroupRoomRepo::new(db).toggle_reaction(
            &group_id,
            &room_id,
            &message_id,
            &pubkey,
            &emoji,
        )
    })
}

/// Emoji reaction counts for all posts in a room, with whether `viewer_pubkey` reacted (JSON rows).
#[frb(sync, serialize)]
pub fn groups_rooms_reactions(
    group_id: String,
    room_id: String,
    viewer_pubkey: String,
) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::room::GroupRoomRepo::new(db).reaction_summary(
            &group_id,
            &room_id,
            &viewer_pubkey,
        )?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// List group threads, pinned-first, sorted by recency or hot engagement
/// (JSON rows with reply + reaction counts).
#[frb(sync, serialize)]
pub fn groups_threads_list(group_id: String, sort: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let group_repo = GroupRepo::new(db);
        if let Some(group) = group_repo.get_by_id(&group_id)? {
            let viewer = super::signer::signer_pubkey().ok();
            if let Some(ref pk) = viewer {
                if BannedMemberRepo::new(db).is_banned(&group_id, pk)? {
                    return Ok("[]".to_string());
                }
            }
            let is_private = group.access_type == "private" || group.password_hash.is_some();
            if is_private {
                let is_member = match &viewer {
                    Some(pk) => {
                        group.pubkey.eq_ignore_ascii_case(pk)
                            || group_repo.is_member(&group_id, pk).unwrap_or(false)
                    }
                    None => false,
                };
                if !is_member {
                    return Ok("[]".to_string());
                }
            }
        }
        let sort = soshal_db_core::repos::thread::ThreadSort::parse(&sort);
        let rows = soshal_db_core::repos::thread::GroupThreadRepo::new(db).list(&group_id, sort)?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create a thread; returns its id.
#[frb(sync, serialize)]
pub fn groups_threads_create(
    group_id: String,
    title: String,
    body: String,
    author: String,
) -> Result<String, String> {
    super::signer::require_identity(&author)?;
    let now = soshal_common_core::format::now_secs();
    let id = format!("thr_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::thread::GroupThreadRow {
        id: id.clone(),
        group_id: group_id.clone(),
        title,
        body,
        author: author.clone(),
        created_at: now,
        is_pinned: false,
        reply_count: 0,
        reaction_count: 0,
    };
    super::db::with_db_result(|db| {
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&group_id)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&author))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&group_id, &author)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a member of this group".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&group_id, &author)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        soshal_db_core::repos::thread::GroupThreadRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Delete a thread and its replies (owner only).
#[frb(sync, serialize)]
pub fn groups_threads_delete(thread_id: String, actor: String) -> Result<bool, String> {
    let group_id = super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::thread::GroupThreadRepo::new(db);
        repo.get(&thread_id)?
            .map(|r| r.group_id)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    require_owner(&group_id, &actor)?;
    super::db::with_db_result(|db| {
        soshal_db_core::repos::thread::GroupThreadRepo::new(db).delete(&thread_id)?;
        Ok(true)
    })
}

/// Pin or unpin a thread (owner only).
#[frb(sync, serialize)]
pub fn groups_threads_pin(thread_id: String, pinned: bool, actor: String) -> Result<bool, String> {
    let group_id = super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::thread::GroupThreadRepo::new(db);
        repo.get(&thread_id)?
            .map(|r| r.group_id)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    require_owner(&group_id, &actor)?;
    super::db::with_db_result(|db| {
        soshal_db_core::repos::thread::GroupThreadRepo::new(db).set_pinned(&thread_id, pinned)?;
        Ok(true)
    })
}

/// Reply to a thread (flat or nested via [parent_id]); returns reply id.
#[frb(sync, serialize)]
pub fn groups_threads_reply(
    thread_id: String,
    parent_id: String,
    content: String,
    author: String,
) -> Result<String, String> {
    super::signer::require_identity(&author)?;
    let now = soshal_common_core::format::now_secs();
    let id = format!("rpl_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::thread::GroupThreadReplyRow {
        id: id.clone(),
        thread_id: thread_id.clone(),
        parent_id,
        author: author.clone(),
        content,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        let thread_repo = soshal_db_core::repos::thread::GroupThreadRepo::new(db);
        let thread = thread_repo
            .get(&thread_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&thread.group_id)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&author))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&thread.group_id, &author)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a member of this group".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&thread.group_id, &author)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        thread_repo.add_reply(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Replies of a thread, oldest-first (JSON rows).
#[frb(sync, serialize)]
pub fn groups_threads_replies(thread_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::thread::GroupThreadRepo::new(db).replies(&thread_id)?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Toggle an emoji reaction on a thread (`reply_id` empty) or a reply;
/// returns true when added, false when removed.
#[frb(sync, serialize)]
pub fn groups_threads_react(
    thread_id: String,
    reply_id: String,
    pubkey: String,
    emoji: String,
) -> Result<bool, String> {
    super::signer::require_identity(&pubkey)?;
    if emoji.is_empty() || emoji.chars().count() > 16 {
        return Err("invalid reaction emoji".into());
    }
    super::db::with_db_result(|db| {
        let thread_repo = soshal_db_core::repos::thread::GroupThreadRepo::new(db);
        let thread = thread_repo
            .get(&thread_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&thread.group_id)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&pubkey))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&thread.group_id, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a member of this group".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&thread.group_id, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        thread_repo.toggle_reaction(&thread_id, &reply_id, &pubkey, &emoji)
    })
}

/// Emoji reaction counts for a thread, per target (thread + each reply),
/// each with whether `viewer_pubkey` reacted (JSON rows).
#[frb(sync, serialize)]
pub fn groups_threads_reactions(
    thread_id: String,
    viewer_pubkey: String,
) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::thread::GroupThreadRepo::new(db)
            .reaction_summary(&thread_id, &viewer_pubkey)?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// List voice channels of a group (JSON rows).
#[frb(sync, serialize)]
pub fn groups_voice_channels_list(group_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows =
            soshal_db_core::repos::voice::GroupVoiceRepo::new(db).list_channels(&group_id)?;
        serde_json::to_string(&rows)
            .map_err(|e| soshal_db_core::error::DbError::Oversized(e.to_string()))
    })
}

/// Create a voice channel (owner only); returns its id.
#[frb(sync, serialize)]
pub fn groups_voice_channels_create(
    group_id: String,
    name: String,
    creator: String,
) -> Result<String, String> {
    require_owner(&group_id, &creator)?;
    let now = soshal_common_core::format::now_secs();
    let id = format!("vc_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::voice::GroupVoiceChannelRow {
        id: id.clone(),
        group_id,
        name,
        position: 0,
        created_by: creator,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).upsert_channel(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// Delete a voice channel and its presence rows (owner only).
#[frb(sync, serialize)]
pub fn groups_voice_channels_delete(channel_id: String, actor: String) -> Result<bool, String> {
    let group_id = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let group_id: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT group_id FROM group_voice_channels WHERE id = ?1",
            libsql::params![channel_id.clone()],
            |r| r.get(0),
        )?;
        group_id.ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    require_owner(&group_id, &actor)?;
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::block_on(conn.execute(
            "DELETE FROM group_voice_presence WHERE channel_id = ?1",
            libsql::params![channel_id.clone()],
        ))
        .map_err(soshal_db_core::error::DbError::from)?;
        soshal_db_core::block_on(conn.execute(
            "DELETE FROM group_voice_channels WHERE id = ?1",
            libsql::params![channel_id],
        ))
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(true)
    })
}

/// Mark local presence in a voice channel (audio transport is a roadmap
/// surface; this only records intent).
#[frb(sync, serialize)]
pub fn groups_voice_join(channel_id: String, pubkey: String) -> Result<bool, String> {
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let group_id: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT group_id FROM group_voice_channels WHERE id = ?1",
            libsql::params![channel_id.as_str()],
            |r| r.get(0),
        )?;
        let gid = group_id.ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let group_repo = GroupRepo::new(db);
        let group = group_repo.get_by_id(&gid)?;
        let is_owner = group
            .as_ref()
            .map(|g| g.pubkey.eq_ignore_ascii_case(&pubkey))
            .unwrap_or(false);
        if !is_owner && !group_repo.is_member(&gid, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "not a group member".to_string(),
            ));
        }
        if BannedMemberRepo::new(db).is_banned(&gid, &pubkey)? {
            return Err(soshal_db_core::error::DbError::Oversized(
                "banned from this group".to_string(),
            ));
        }
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).join(&channel_id, &pubkey)?;
        Ok(true)
    })
}

/// Clear local presence from a voice channel.
#[frb(sync, serialize)]
pub fn groups_voice_leave(channel_id: String, pubkey: String) -> Result<bool, String> {
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| {
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).leave(&channel_id, &pubkey)?;
        Ok(true)
    })
}

/// Members currently present in a voice channel (JSON rows).
#[frb(sync, serialize)]
pub fn groups_voice_presence(channel_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let group_id: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT group_id FROM group_voice_channels WHERE id = ?1",
            libsql::params![channel_id.as_str()],
            |r| r.get(0),
        )?;
        let Some(gid) = group_id else {
            return Ok("[]".to_string());
        };
        let group_repo = GroupRepo::new(db);
        let Some(group) = group_repo.get_by_id(&gid)? else {
            return Ok("[]".to_string());
        };
        let viewer = super::signer::signer_pubkey().ok();
        if let Some(ref pk) = viewer {
            if BannedMemberRepo::new(db).is_banned(&gid, pk)? {
                return Ok("[]".to_string());
            }
        }
        let is_private = group.access_type == "private" || group.password_hash.is_some();
        if is_private {
            let is_member = match &viewer {
                Some(pk) => {
                    group.pubkey.eq_ignore_ascii_case(pk)
                        || group_repo.is_member(&gid, pk).unwrap_or(false)
                }
                None => false,
            };
            if !is_member {
                return Ok("[]".to_string());
            }
        }
        let voice_repo = soshal_db_core::repos::voice::GroupVoiceRepo::new(db);
        let rows = voice_repo.presence(&channel_id)?;
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
    is_private: bool,
    password: Option<String>,
) -> Result<String, String> {
    let group_id = group_id.trim().to_string();
    if group_id.is_empty() || group_id.len() > 128 {
        return Err("invalid group_id".into());
    }
    let trimmed_name = name.trim().to_string();
    if trimmed_name.is_empty() || trimmed_name.len() > 200 {
        return Err("invalid group name".into());
    }
    if description.len() > 5000 {
        return Err("group description too long".into());
    }
    if !picture_url.is_empty() && !soshal_common_core::url::is_valid_media_url(&picture_url) {
        return Err("invalid picture url".into());
    }
    if creator_pubkey.trim().is_empty() || creator_pubkey.len() > 128 {
        return Err("invalid creator pubkey".into());
    }
    let password_hash = if is_private {
        let pwd = password.as_deref().unwrap_or("").trim();
        if pwd.is_empty() {
            return Err("password required for private community".to_string());
        }
        Some(soshal_groups_core::access::hash_community_password(pwd)?)
    } else {
        None
    };
    let access_type = if is_private { "private" } else { "open" }.to_string();
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
        access_type,
        relay: None,
        sync_status: "local".to_string(),
        password_hash,
    };
    super::db::with_db_result(|db| {
        GroupRepo::new(db).upsert(&row)?;
        GroupRepo::new(db).add_member(&group_id, &creator_pubkey, "owner", now)?;
        Ok(())
    })?;
    get_group_info_with_viewer(&group_id, Some(&creator_pubkey))
}

/// Change or remove group password and update private/open access type (owner only).
#[frb(sync, serialize)]
pub fn groups_set_password(
    group_id: String,
    new_password: Option<String>,
    actor_pubkey: String,
) -> Result<bool, String> {
    require_owner(&group_id, &actor_pubkey)?;
    let new_password = new_password.map(zeroize::Zeroizing::new);
    let (access_type, password_hash) = match new_password.as_deref() {
        Some(p) if !p.trim().is_empty() => {
            let h = soshal_groups_core::access::hash_community_password(p.trim())?;
            ("private".to_string(), Some(h))
        }
        _ => ("open".to_string(), None),
    };
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let mut group = repo
            .get_by_id(&group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        group.access_type = access_type;
        group.password_hash = password_hash;
        group.updated_at = soshal_common_core::format::now_secs();
        repo.upsert(&group)?;
        Ok(true)
    })
}

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static ATTEMPTS: LazyLock<Mutex<HashMap<String, (u32, i64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_ATTEMPTS: u32 = 5;
const WINDOW_SECS: i64 = 30;
const MAX_ATTEMPTS_MAP: usize = 1000;

/// Evict expired entries from the ATTEMPTS map and cap its size.
fn sweep_attempts(now: i64, attempts: &mut HashMap<String, (u32, i64)>) {
    attempts.retain(|_, (_, ts)| now.saturating_sub(*ts) < WINDOW_SECS);
    // If still over cap after sweep, drop oldest entries by timestamp.
    if attempts.len() > MAX_ATTEMPTS_MAP {
        let mut entries: Vec<_> = attempts
            .iter()
            .map(|(k, &(_, ts))| (k.clone(), ts))
            .collect();
        entries.sort_by_key(|&(_, ts)| ts);
        for (key, _) in entries.into_iter().take(attempts.len() - MAX_ATTEMPTS_MAP) {
            attempts.remove(&key);
        }
    }
}

/// Test whether candidate password matches the group's password hash.
/// Rate-limited per group (5 attempts / 30 s) so the FFI surface cannot be
/// used as an offline-style guess oracle.
#[frb(sync, serialize)]
pub fn groups_verify_password(group_id: String, password: String) -> Result<bool, String> {
    let password = zeroize::Zeroizing::new(password);
    let now = soshal_common_core::format::now_secs();

    // Phase 1: Check lockout window (no counter increment yet).
    {
        let mut attempts = crate::ffi::util::lock(&ATTEMPTS);
        sweep_attempts(now, &mut attempts);
        let entry = attempts.entry(group_id.clone()).or_insert((0, now));
        if entry.1 + WINDOW_SECS <= now {
            *entry = (0, now);
        }
        if entry.0 >= MAX_ATTEMPTS {
            return Err(format!(
                "too many password attempts for this group; retry in {}s",
                entry.1 + WINDOW_SECS - now
            ));
        }
    }

    // Phase 2: Look up group + password hash. Missing groups and
    // passwordless groups return an error WITHOUT burning an attempt.
    let stored = super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let group = repo
            .get_by_id(&group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        match group.password_hash.as_deref() {
            Some(h) if !h.is_empty() => Ok(h.to_string()),
            _ => Err(soshal_db_core::error::DbError::Oversized(
                "group has no password set; cannot verify access".to_string(),
            )),
        }
    })?;

    // Phase 3: Increment counter only for a real, verifiable attempt.
    {
        let mut attempts = crate::ffi::util::lock(&ATTEMPTS);
        sweep_attempts(now, &mut attempts);
        let entry = attempts.entry(group_id.clone()).or_insert((0, now));
        if entry.1 + WINDOW_SECS <= now {
            *entry = (0, now);
        }
        if entry.0 >= MAX_ATTEMPTS {
            return Err(format!(
                "too many password attempts for this group; retry in {}s",
                entry.1 + WINDOW_SECS - now
            ));
        }
        entry.0 += 1;
    }

    // Phase 4: Verify password.
    let ok = soshal_groups_core::access::verify_community_password(password.trim(), &stored);

    // Phase 5: Successful verify resets the counter.
    if ok {
        let mut attempts = crate::ffi::util::lock(&ATTEMPTS);
        sweep_attempts(now, &mut attempts);
        if let Some(entry) = attempts.get_mut(&group_id) {
            entry.0 = 0;
        }
    }

    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        crate::ffi::db::insert_test_user(pubkey);
    }

    fn create_group(id: &str, owner: &str) {
        insert_user(owner);
        groups_create(
            id.to_string(),
            "Soshal Group".to_string(),
            "a test group".to_string(),
            String::new(),
            owner.to_string(),
            false,
            None,
        )
        .unwrap();
    }

    #[test]
    fn test_create_group_and_fetch() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("create");
        let owner = "a".repeat(64);
        crate::ffi::db::insert_test_user(&owner);

        assert_eq!(
            groups_fetch_groups(owner.clone(), "public".to_string()).unwrap(),
            "[]"
        );

        let info = groups_create(
            "g1".to_string(),
            "Soshal Group".to_string(),
            "a test group".to_string(),
            "https://example.com/pic.png".to_string(),
            owner.clone(),
            false,
            None,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&info).unwrap();
        assert_eq!(v["name"], "Soshal Group");
        assert_eq!(v["owner"], owner);
        assert_eq!(v["role"], "owner");
        assert_eq!(v["members"], 1);
        assert_eq!(v["is_private"], false);

        let list = groups_fetch_groups(owner, "public".to_string()).unwrap();
        assert!(list.contains("\"g1\""));
        assert!(list.contains("a test group"));

        let detail = groups_get_group_info("g1".to_string()).unwrap();
        assert!(detail.contains("https://example.com/pic.png"));
    }

    #[test]
    fn test_get_group_info_missing_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("missing");
        assert!(groups_get_group_info("nope".to_string()).is_err());
    }

    #[test]
    fn test_join_leave_and_members() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("members");
        let owner = "a".repeat(64);
        let keys = soshal_nostr_core::keys::generate_keys();
        let member = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        create_group("g2", &owner);
        insert_user(&member);

        assert!(groups_join("g2".to_string(), member.clone(), None).unwrap());

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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("role_gate");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let keys = soshal_nostr_core::keys::generate_keys();
        let member = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        create_group("g3", &owner);
        insert_user(&member);
        groups_join("g3".to_string(), member.clone(), None).unwrap();

        let denied = groups_set_member_role(
            "g3".to_string(),
            member.clone(),
            "mod".to_string(),
            member.clone(),
        );
        assert!(denied.unwrap_err().contains("only the group owner"));

        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_set_member_role(
            "g3".to_string(),
            member.clone(),
            "mod".to_string(),
            owner.clone(),
        )
        .unwrap());
        let with_roles = groups_members_with_roles("g3".to_string()).unwrap();
        assert!(with_roles.contains("\"role\":\"mod\""));

        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let denied = groups_remove_member("g3".to_string(), member.clone(), member.clone());
        assert!(denied.unwrap_err().contains("only the group owner"));

        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_remove_member("g3".to_string(), owner.clone(), owner.clone()).is_err());
        assert!(groups_remove_member("g3".to_string(), member, owner).unwrap());

        let members = groups_get_members("g3".to_string()).unwrap();
        assert_eq!(members.len(), 1);
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_role_ops_on_missing_group_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("no_msgs");
        assert_eq!(
            groups_fetch_messages("g5".to_string(), String::new(), 20, 0).unwrap(),
            "[]"
        );
    }

    #[test]
    fn test_post_message_and_fetch_messages() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("messages");
        let keys = soshal_nostr_core::keys::generate_keys();
        let owner = keys.public_key().to_hex();
        create_group("g5", &owner);
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let signed =
            groups_post_message("g5".to_string(), String::new(), "hello group".to_string())
                .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["kind"], 1059);
        assert_eq!(ev["pubkey"], keys.public_key().to_hex());
        assert!(ev["sig"].as_str().is_some());

        std::thread::sleep(std::time::Duration::from_millis(1100));
        groups_post_message(
            "g5".to_string(),
            String::new(),
            "second message".to_string(),
        )
        .unwrap();

        let msgs = groups_fetch_messages("g5".to_string(), String::new(), 10, 0).unwrap();
        assert!(msgs.contains("second message"));
        assert!(msgs.contains("hello group"));

        let newest = groups_fetch_messages("g5".to_string(), String::new(), 1, 0).unwrap();
        assert!(newest.contains("second message"));
        assert!(!newest.contains("hello group"));

        let page2 = groups_fetch_messages("g5".to_string(), String::new(), 1, 1).unwrap();
        assert!(page2.contains("hello group"));

        let clamped = groups_fetch_messages("g5".to_string(), String::new(), 0, -5).unwrap();
        assert!(clamped.contains("second message"));

        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_rooms_crud_and_scoped_messages() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("rooms");
        super::super::signer::signer_unlock(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        )
        .unwrap();
        let owner = super::super::signer::signer_pubkey().unwrap();
        create_group("g6", &owner);
        insert_user(&owner);
        groups_join("g6".to_string(), owner.clone(), None).unwrap();

        assert_eq!(groups_rooms_list("g6".to_string()).unwrap(), "[]");
        let room_id = groups_rooms_create(
            "g6".to_string(),
            "gaming".to_string(),
            "FPS talk".to_string(),
            "🎮".to_string(),
            "#ff0000".to_string(),
            owner.clone(),
        )
        .unwrap();
        assert!(room_id.starts_with("room_"));
        let list = groups_rooms_list("g6".to_string()).unwrap();
        assert!(list.contains("gaming"));
        assert!(list.contains("🎮"));

        assert!(groups_rooms_update(
            room_id.clone(),
            "g6".to_string(),
            "games".to_string(),
            "new topic".to_string(),
            "🕹️".to_string(),
            "#00ff00".to_string(),
            owner.clone(),
        )
        .unwrap());
        let list = groups_rooms_list("g6".to_string()).unwrap();
        assert!(list.contains("games"));
        assert!(!list.contains("gaming"));

        groups_post_message("g6".to_string(), room_id.clone(), "in room".to_string()).unwrap();
        groups_post_message("g6".to_string(), String::new(), "general chat".to_string()).unwrap();
        let room_msgs = groups_fetch_messages("g6".to_string(), room_id.clone(), 10, 0).unwrap();
        assert!(room_msgs.contains("in room"));
        assert!(!room_msgs.contains("general chat"));
        let general_msgs = groups_fetch_messages("g6".to_string(), String::new(), 10, 0).unwrap();
        assert!(general_msgs.contains("general chat"));
        assert!(!general_msgs.contains("in room"));

        let denied = groups_rooms_delete(room_id.clone(), "b".repeat(64));
        assert!(denied.unwrap_err().contains("only the group owner"));
        assert!(groups_rooms_delete(room_id.clone(), owner).unwrap());
        assert_eq!(groups_rooms_list("g6".to_string()).unwrap(), "[]");
        let orphan = groups_fetch_messages("g6".to_string(), room_id, 10, 0).unwrap();
        assert_eq!(orphan, "[]");

        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_threads_crud_replies_pin() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("threads");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let member_keys = soshal_nostr_core::keys::generate_keys();
        let member = member_keys.public_key().to_hex();
        create_group("g7", &owner);
        insert_user(&member);
        crate::ffi::db::with_db_result(|db| {
            GroupRepo::new(db).add_member("g7", &member, "member", 0)
        })
        .unwrap();
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();

        assert_eq!(
            groups_threads_list("g7".to_string(), "newest".to_string()).unwrap(),
            "[]"
        );
        let thread_id = groups_threads_create(
            "g7".to_string(),
            "First thread".to_string(),
            "body text".to_string(),
            owner.clone(),
        )
        .unwrap();
        assert!(thread_id.starts_with("thr_"));

        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        let reply_id = groups_threads_reply(
            thread_id.clone(),
            String::new(),
            "a reply".to_string(),
            member.clone(),
        )
        .unwrap();
        let nested = groups_threads_reply(
            thread_id.clone(),
            reply_id,
            "nested reply".to_string(),
            member.clone(),
        )
        .unwrap();
        assert!(nested.starts_with("rpl_"));

        let replies = groups_threads_replies(thread_id.clone()).unwrap();
        assert!(replies.contains("a reply"));
        assert!(replies.contains("nested reply"));

        let list = groups_threads_list("g7".to_string(), "newest".to_string()).unwrap();
        assert!(list.contains("First thread"));
        assert!(list.contains("\"reply_count\":2"));
        assert!(list.contains("\"reaction_count\":0"));

        let denied = groups_threads_pin(thread_id.clone(), true, member);
        assert!(denied.unwrap_err().contains("only the group owner"));

        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_threads_pin(thread_id.clone(), true, owner.clone()).unwrap());
        let pinned = groups_threads_list("g7".to_string(), "newest".to_string()).unwrap();
        assert!(pinned.contains("\"is_pinned\":true"));

        assert!(groups_threads_delete(thread_id.clone(), owner).unwrap());
        assert_eq!(groups_threads_replies(thread_id).unwrap(), "[]");
        assert_eq!(
            groups_threads_list("g7".to_string(), "newest".to_string()).unwrap(),
            "[]"
        );
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_thread_reactions_and_popular_sort() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("thread_reacts");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let member_keys = soshal_nostr_core::keys::generate_keys();
        let member = member_keys.public_key().to_hex();
        let other_keys = soshal_nostr_core::keys::generate_keys();
        let other = other_keys.public_key().to_hex();
        create_group("g9", &owner);
        insert_user(&member);
        insert_user(&other);
        crate::ffi::db::with_db_result(|db| {
            let repo = GroupRepo::new(db);
            repo.add_member("g9", &member, "member", 0)?;
            repo.add_member("g9", &other, "member", 0)
        })
        .unwrap();
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();

        let old = groups_threads_create(
            "g9".to_string(),
            "Old busy thread".to_string(),
            "".to_string(),
            owner.clone(),
        )
        .unwrap();
        // Distinct timestamps so the newest sort has a deterministic order.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let _fresh = groups_threads_create(
            "g9".to_string(),
            "Fresh quiet thread".to_string(),
            "".to_string(),
            owner,
        )
        .unwrap();

        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        assert!(
            groups_threads_react(old.clone(), String::new(), member.clone(), "👍".to_string(),)
                .unwrap()
        );
        super::super::signer::signer_unlock(other_keys.secret_key().to_secret_hex()).unwrap();
        assert!(
            groups_threads_react(old.clone(), String::new(), other.clone(), "👍".to_string(),)
                .unwrap()
        );
        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        assert!(
            groups_threads_react(old.clone(), String::new(), member.clone(), "❤️".to_string(),)
                .unwrap()
        );

        let summary = groups_threads_reactions(old.clone(), member.clone()).unwrap();
        assert!(summary.contains("\"emoji\":\"👍\""));
        assert!(summary.contains("\"count\":2"));
        assert!(summary.contains("\"reacted\":true"));

        // Toggle off removes.
        assert!(!groups_threads_react(
            old.clone(),
            String::new(),
            member.clone(),
            "👍".to_string(),
        )
        .unwrap());
        let summary = groups_threads_reactions(old.clone(), member.clone()).unwrap();
        assert!(summary.contains("\"count\":1"));
        assert!(summary.contains("\"reacted\":false"));

        // Reply-level reactions carry reply_id.
        let rpl = groups_threads_reply(
            old.clone(),
            String::new(),
            "reaction target".to_string(),
            member.clone(),
        )
        .unwrap();
        super::super::signer::signer_unlock(other_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_threads_react(old.clone(), rpl.clone(), other, "🔥".to_string(),).unwrap());
        let summary = groups_threads_reactions(old.clone(), member.clone()).unwrap();
        assert!(summary.contains(&format!("\"reply_id\":\"{}\"", rpl)));
        assert!(summary.contains("\"emoji\":\"🔥\""));

        // Empty/oversized emoji rejected.
        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        assert!(
            groups_threads_react(old.clone(), String::new(), member.clone(), String::new())
                .is_err()
        );
        assert!(groups_threads_react(old, String::new(), member, "x".repeat(20)).is_err());

        // Popular sort ranks the engaged old thread above the fresh quiet one
        // (pinned state equal, so pure hot score decides).
        let popular = groups_threads_list("g9".to_string(), "popular".to_string()).unwrap();
        assert!(
            popular.find("Old busy thread").unwrap() < popular.find("Fresh quiet thread").unwrap()
        );
        let newest = groups_threads_list("g9".to_string(), "newest".to_string()).unwrap();
        assert!(
            newest.find("Fresh quiet thread").unwrap() < newest.find("Old busy thread").unwrap()
        );
        // Unknown sort falls back to newest.
        assert_eq!(
            groups_threads_list("g9".to_string(), "bogus".to_string()).unwrap(),
            newest
        );
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_threads_member_and_ban_checks() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("threads_auth_ban");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let user_keys = soshal_nostr_core::keys::generate_keys();
        let user = user_keys.public_key().to_hex();
        create_group("g_auth", &owner);
        insert_user(&user);

        // 1. Non-member cannot create a thread
        super::super::signer::signer_unlock(user_keys.secret_key().to_secret_hex()).unwrap();
        let res = groups_threads_create(
            "g_auth".to_string(),
            "Non-member thread".to_string(),
            "body".to_string(),
            user.clone(),
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not a member"));

        // Owner creates a thread
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        let thread_id = groups_threads_create(
            "g_auth".to_string(),
            "Owner thread".to_string(),
            "body".to_string(),
            owner.clone(),
        )
        .unwrap();

        // Non-member cannot reply or react
        super::super::signer::signer_unlock(user_keys.secret_key().to_secret_hex()).unwrap();
        let reply_res = groups_threads_reply(
            thread_id.clone(),
            String::new(),
            "reply".to_string(),
            user.clone(),
        );
        assert!(reply_res.is_err());
        assert!(reply_res.unwrap_err().contains("not a member"));

        let react_res = groups_threads_react(
            thread_id.clone(),
            String::new(),
            user.clone(),
            "👍".to_string(),
        );
        assert!(react_res.is_err());
        assert!(react_res.unwrap_err().contains("not a member"));

        // 2. Add user as member -> now they can reply and react
        crate::ffi::db::with_db_result(|db| {
            GroupRepo::new(db).add_member("g_auth", &user, "member", 0)
        })
        .unwrap();

        let reply_ok = groups_threads_reply(
            thread_id.clone(),
            String::new(),
            "member reply".to_string(),
            user.clone(),
        );
        assert!(reply_ok.is_ok());

        let react_ok = groups_threads_react(
            thread_id.clone(),
            String::new(),
            user.clone(),
            "👍".to_string(),
        );
        assert!(react_ok.is_ok());

        // 3. Ban user -> now banned from creating threads, replies, and reactions
        crate::ffi::db::with_db_result(|db| {
            BannedMemberRepo::new(db).insert(
                &soshal_db_core::repos::banned_member::BannedMemberRow {
                    group_id: "g_auth".to_string(),
                    pubkey: user.clone(),
                    banned_by: owner.clone(),
                    reason: "violating rules".to_string(),
                    banned_at: soshal_common_core::format::now_secs(),
                },
            )
        })
        .unwrap();

        let banned_create = groups_threads_create(
            "g_auth".to_string(),
            "Banned thread".to_string(),
            "body".to_string(),
            user.clone(),
        );
        assert!(banned_create.is_err());
        assert!(banned_create
            .unwrap_err()
            .contains("banned from this group"));

        let banned_reply = groups_threads_reply(
            thread_id.clone(),
            String::new(),
            "banned reply".to_string(),
            user.clone(),
        );
        assert!(banned_reply.is_err());
        assert!(banned_reply.unwrap_err().contains("banned from this group"));

        let banned_react = groups_threads_react(thread_id, String::new(), user, "❤️".to_string());
        assert!(banned_react.is_err());
        assert!(banned_react.unwrap_err().contains("banned from this group"));

        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_voice_channels_and_presence() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("voice");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let member_keys = soshal_nostr_core::keys::generate_keys();
        let member = member_keys.public_key().to_hex();
        create_group("g8", &owner);
        insert_user(&member);

        assert_eq!(groups_voice_channels_list("g8".to_string()).unwrap(), "[]");
        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        let denied =
            groups_voice_channels_create("g8".to_string(), "Lounge".to_string(), member.clone());
        assert!(denied.unwrap_err().contains("only the group owner"));

        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        let ch =
            groups_voice_channels_create("g8".to_string(), "Lounge".to_string(), owner.clone())
                .unwrap();
        assert!(ch.starts_with("vc_"));

        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        // Non-member is rejected from joining voice channel
        let non_member_join = groups_voice_join(ch.clone(), member.clone());
        assert!(non_member_join.is_err());
        assert!(non_member_join.unwrap_err().contains("not a group member"));

        // Join group as member
        groups_join("g8".to_string(), member.clone(), None).unwrap();
        assert!(groups_voice_join(ch.clone(), member.clone()).unwrap());

        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_voice_join(ch.clone(), owner.clone()).unwrap());
        let presence = groups_voice_presence(ch.clone()).unwrap();
        assert!(presence.contains(&format!("\"pubkey\":\"{}\"", member)));
        assert!(presence.contains(&format!("\"pubkey\":\"{}\"", owner)));

        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_voice_leave(ch.clone(), member.clone()).unwrap());
        let presence = groups_voice_presence(ch.clone()).unwrap();
        assert!(!presence.contains(&format!("\"pubkey\":\"{}\"", member)));

        // Ban member and verify voice join is rejected
        super::super::db::with_db_result(|db| {
            soshal_db_core::repos::banned_member::BannedMemberRepo::new(db).insert(
                &soshal_db_core::repos::banned_member::BannedMemberRow {
                    group_id: "g8".to_string(),
                    pubkey: member.clone(),
                    banned_by: owner.clone(),
                    reason: "disruptive".to_string(),
                    banned_at: soshal_common_core::format::now_secs(),
                },
            )
        })
        .unwrap();

        super::super::signer::signer_unlock(member_keys.secret_key().to_secret_hex()).unwrap();
        let banned_join = groups_voice_join(ch.clone(), member.clone());
        assert!(banned_join.is_err());
        assert!(banned_join.unwrap_err().contains("banned from this group"));

        assert!(groups_voice_channels_delete(ch.clone(), member).is_err());
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_voice_channels_delete(ch.clone(), owner).unwrap());
        assert_eq!(groups_voice_channels_list("g8".to_string()).unwrap(), "[]");
        assert_eq!(groups_voice_presence(ch).unwrap(), "[]");
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_admin_ops_on_missing_rows_error() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("missing_rows");
        let owner = "a".repeat(64);
        assert!(groups_rooms_delete("nope".to_string(), owner.clone()).is_err());
        assert!(groups_threads_delete("nope".to_string(), owner.clone()).is_err());
        assert!(groups_threads_pin("nope".to_string(), true, owner.clone()).is_err());
        assert!(groups_voice_channels_delete("nope".to_string(), owner).is_err());
    }

    #[test]
    fn test_post_message_locked_signer_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("msg_locked");
        super::super::signer::signer_lock().unwrap();
        assert!(groups_post_message("g5".to_string(), String::new(), "hi".to_string()).is_err());
    }

    #[test]
    fn test_private_group_password_flow() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("private_pwd");
        let owner = "a".repeat(64);
        let keys = soshal_nostr_core::keys::generate_keys();
        let member = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        insert_user(&owner);
        insert_user(&member);

        // Missing password for private group fails
        assert!(groups_create(
            "priv1".to_string(),
            "Secret Society".to_string(),
            "Top secret".to_string(),
            "".to_string(),
            owner.clone(),
            true,
            None,
        )
        .is_err());

        // Create private group with password
        let created_json = groups_create(
            "priv1".to_string(),
            "Secret Society".to_string(),
            "Top secret".to_string(),
            "".to_string(),
            owner,
            true,
            Some("OpenSesame123".to_string()),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&created_json).unwrap();
        assert_eq!(v["is_private"], true);

        // Joining without password fails
        let no_pwd_err = groups_join("priv1".to_string(), member.clone(), None);
        assert!(no_pwd_err.is_err());
        assert!(no_pwd_err.unwrap_err().contains("password required"));

        // Joining with wrong password fails
        let wrong_pwd_err = groups_join(
            "priv1".to_string(),
            member.clone(),
            Some("wrongpwd".to_string()),
        );
        assert!(wrong_pwd_err.is_err());
        assert!(wrong_pwd_err
            .unwrap_err()
            .contains("incorrect community password"));

        // Joining with correct password succeeds
        assert!(groups_join(
            "priv1".to_string(),
            member.clone(),
            Some("OpenSesame123".to_string()),
        )
        .unwrap());

        let members = groups_get_members("priv1".to_string()).unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&member));
    }

    #[test]
    fn test_set_password_and_verify() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("set_pwd");
        let owner_keys = soshal_nostr_core::keys::generate_keys();
        let owner = owner_keys.public_key().to_hex();
        let non_owner = "b".repeat(64);
        insert_user(&owner);
        insert_user(&non_owner);
        create_group("pub_grp", &owner);

        let info: serde_json::Value =
            serde_json::from_str(&groups_get_group_info("pub_grp".to_string()).unwrap()).unwrap();
        assert_eq!(info["is_private"], false);

        // Non-owner cannot set password
        assert!(groups_set_password(
            "pub_grp".to_string(),
            Some("NewSecret99".to_string()),
            non_owner,
        )
        .is_err());

        // Owner sets password -> becomes private
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(groups_set_password(
            "pub_grp".to_string(),
            Some("NewSecret99".to_string()),
            owner.clone(),
        )
        .unwrap());

        let info2: serde_json::Value =
            serde_json::from_str(&groups_get_group_info("pub_grp".to_string()).unwrap()).unwrap();
        assert_eq!(info2["is_private"], true);

        // Verify password
        assert!(groups_verify_password("pub_grp".to_string(), "NewSecret99".to_string()).unwrap());
        assert!(!groups_verify_password("pub_grp".to_string(), "wrong".to_string()).unwrap());

        // Owner clears password -> becomes open
        assert!(groups_set_password("pub_grp".to_string(), None, owner).unwrap());
        let info3: serde_json::Value =
            serde_json::from_str(&groups_get_group_info("pub_grp".to_string()).unwrap()).unwrap();
        assert_eq!(info3["is_private"], false);
        let _ = super::super::signer::signer_lock();
    }

    /// Helper to reset the ATTEMPTS static for a specific group between tests.
    fn reset_attempts_for(group_id: &str) {
        if let Ok(mut m) = ATTEMPTS.lock() {
            m.remove(group_id);
        }
    }

    #[test]
    fn test_verify_passwordless_group_does_not_burn_attempts() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("verify_no_pwd");
        let owner = "a".repeat(64);
        create_group("npwd", &owner);
        reset_attempts_for("npwd");

        // Verify on a passwordless group fails but does NOT burn attempts.
        let err = groups_verify_password("npwd".to_string(), "anything".to_string()).unwrap_err();
        assert!(err.contains("no password set"), "err: {err}");

        // 5 more calls should still not lock out — counter was never incremented.
        for _ in 0..5 {
            let e = groups_verify_password("npwd".to_string(), "x".to_string()).unwrap_err();
            assert!(e.contains("no password set"), "err: {e}");
        }
        // Verify no lockout error appeared (would contain "too many").
        let final_err = groups_verify_password("npwd".to_string(), "x".to_string()).unwrap_err();
        assert!(
            final_err.contains("no password set"),
            "unexpected lockout: {final_err}"
        );
    }

    #[test]
    fn test_verify_success_resets_counter() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("verify_success_reset");
        let owner = "a".repeat(64);
        insert_user(&owner);
        groups_create(
            "pwdok".to_string(),
            "G".to_string(),
            String::new(),
            String::new(),
            owner,
            true,
            Some("Secret123".to_string()),
        )
        .unwrap();
        reset_attempts_for("pwdok");

        // 4 wrong attempts (out of 5 max).
        for _ in 0..4 {
            assert!(!groups_verify_password("pwdok".to_string(), "wrong".to_string()).unwrap());
        }
        // Correct password succeeds and resets counter.
        assert!(groups_verify_password("pwdok".to_string(), "Secret123".to_string()).unwrap());
        // Can do another 4 wrong attempts without lockout (counter reset to 0).
        for _ in 0..4 {
            assert!(!groups_verify_password("pwdok".to_string(), "bad".to_string()).unwrap());
        }
        // Still not locked out.
        let r = groups_verify_password("pwdok".to_string(), "nope".to_string());
        assert!(!r.unwrap(), "should be wrong pw, not locked out");
    }

    #[test]
    fn test_verify_boundary_window_reset() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _db = TestDb::init("verify_boundary");
        let owner = "a".repeat(64);
        insert_user(&owner);
        groups_create(
            "bndry".to_string(),
            "G".to_string(),
            String::new(),
            String::new(),
            owner,
            true,
            Some("Pass1234".to_string()),
        )
        .unwrap();
        reset_attempts_for("bndry");

        // Exhaust all 5 attempts with wrong passwords.
        for _ in 0..5 {
            assert!(!groups_verify_password("bndry".to_string(), "wrong".to_string()).unwrap());
        }
        // Now locked out.
        let locked_err =
            groups_verify_password("bndry".to_string(), "wrong".to_string()).unwrap_err();
        assert!(locked_err.contains("too many"), "err: {locked_err}");

        // Manually set the window start to exactly WINDOW_SECS ago.
        let now = soshal_common_core::format::now_secs();
        {
            let mut m = crate::ffi::util::lock(&ATTEMPTS);
            if let Some(entry) = m.get_mut("bndry") {
                // Set timestamp so that entry.1 + WINDOW_SECS == now (exact boundary).
                entry.1 = now - WINDOW_SECS;
            }
        }
        // The <= check should now reset the window at the exact boundary.
        // Should return Ok (no lockout), even though the password is wrong.
        let r = groups_verify_password("bndry".to_string(), "wrong".to_string());
        assert!(
            r.is_ok(),
            "window should have reset at exact boundary, got: {r:?}"
        );
        assert!(!r.unwrap(), "password is wrong as expected");
    }

    #[test]
    fn test_shared_key_read_heals_legacy_plaintext() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("heal-key");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        // Plant a legacy plaintext row exactly as a pre-seal writer would.
        let raw_hex = "a1b2c3d4e5f67890123456789abcdef0123456789abcdef0123456789abcdef0";
        {
            crate::ffi::db::with_db_result(|db| {
                GroupRepo::new(db).set_shared_key("gheal", raw_hex)?;
                Ok(())
            })
            .unwrap();
        }
        // First read returns the raw key AND heals the row in place.
        let key = crate::ffi::db::with_db_result(|db| shared_key_for_group(db, "gheal")).unwrap();
        assert_eq!(key.as_deref(), Some(raw_hex));
        let stored =
            crate::ffi::db::with_db_result(|db| GroupRepo::new(db).get_shared_key("gheal"))
                .unwrap()
                .unwrap();
        assert!(
            stored.starts_with("seal1:"),
            "legacy key not healed into seal1 envelope: {stored}"
        );
        assert!(
            !stored.contains(raw_hex),
            "raw key leaked into sealed value: {stored}"
        );
        // A second read still resolves the same key through the seal path.
        let key2 = crate::ffi::db::with_db_result(|db| shared_key_for_group(db, "gheal")).unwrap();
        assert_eq!(key2.as_deref(), Some(raw_hex));
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_room_messages_reactions() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _db = TestDb::init("room-react");

        let group_id = "g_room_rx".to_string();
        let room_id = "room_abc".to_string();
        let msg_id = "msg_123".to_string();
        let keys_alice = soshal_nostr_core::keys::generate_keys();
        let alice = keys_alice.public_key().to_hex();
        let keys_bob = soshal_nostr_core::keys::generate_keys();
        let bob = keys_bob.public_key().to_hex();

        create_group(&group_id, &alice);
        insert_user(&bob);
        crate::ffi::db::with_db_result(|db| {
            GroupRepo::new(db).add_member(&group_id, &bob, "member", 0)
        })
        .unwrap();

        crate::ffi::signer::signer_unlock(keys_alice.secret_key().to_secret_hex()).unwrap();

        // Reacting with invalid emoji fails
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            alice.clone(),
            String::new()
        )
        .is_err());
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            alice.clone(),
            "x".repeat(20)
        )
        .is_err());

        // Alice reacts 👍
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            alice.clone(),
            "👍".to_string()
        )
        .unwrap());

        // Identity mismatch check: Alice is active signer, so reacting as Bob fails
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            bob.clone(),
            "👍".to_string()
        )
        .is_err());

        // Bob unlocks and reacts 👍
        crate::ffi::signer::signer_unlock(keys_bob.secret_key().to_secret_hex()).unwrap();
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            bob.clone(),
            "👍".to_string()
        )
        .unwrap());

        // Alice unlocks and reacts ❤️
        crate::ffi::signer::signer_unlock(keys_alice.secret_key().to_secret_hex()).unwrap();
        assert!(groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            alice.clone(),
            "❤️".to_string()
        )
        .unwrap());

        // Summary for Alice
        let summary_alice =
            groups_rooms_reactions(group_id.clone(), room_id.clone(), alice.clone()).unwrap();
        assert!(summary_alice.contains("\"emoji\":\"👍\""));
        assert!(summary_alice.contains("\"count\":2"));
        assert!(summary_alice.contains("\"reacted\":true"));
        assert!(summary_alice.contains("\"emoji\":\"❤️\""));

        // Summary for Bob
        let summary_bob =
            groups_rooms_reactions(group_id.clone(), room_id.clone(), bob.clone()).unwrap();
        assert!(summary_bob.contains("\"count\":2"));

        // Alice toggles 👍 off
        assert!(!groups_rooms_react(
            group_id.clone(),
            room_id.clone(),
            msg_id.clone(),
            alice.clone(),
            "👍".to_string()
        )
        .unwrap());

        let summary_after = groups_rooms_reactions(group_id, room_id, alice).unwrap();
        assert!(summary_after.contains("\"emoji\":\"👍\""));
        assert!(summary_after.contains("\"count\":1"));
        assert!(summary_after.contains("\"reacted\":false"));

        crate::ffi::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_groups_create_validation_and_casing() {
        let _lock = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _signer_lock = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("groups_validation");

        // Empty group_id rejected
        assert!(groups_create(
            "".into(),
            "name".into(),
            "".into(),
            "".into(),
            "pk".into(),
            false,
            None
        )
        .is_err());
        // Empty name rejected
        assert!(groups_create(
            "gid".into(),
            "  ".into(),
            "".into(),
            "".into(),
            "pk".into(),
            false,
            None
        )
        .is_err());
        // Invalid picture url rejected
        assert!(groups_create(
            "gid".into(),
            "name".into(),
            "".into(),
            "http://127.0.0.1/evil.png".into(),
            "pk".into(),
            false,
            None
        )
        .is_err());
        // Valid group created
        insert_user("Alice_PK");
        let res = groups_create(
            "g_val".into(),
            "Group Name".into(),
            "about".into(),
            "https://example.com/pic.png".into(),
            "Alice_PK".into(),
            false,
            None,
        )
        .unwrap();
        let info: GroupInfo = serde_json::from_str(&res).unwrap();
        assert_eq!(info.id, "g_val");

        // row_to_group case-insensitive viewer matching
        let row = soshal_db_core::repos::group::GroupRow {
            id: "g_case".into(),
            name: "Case Test".into(),
            about: None,
            picture: None,
            pubkey: "aabbcc".into(),
            created_at: 100,
            updated_at: 100,
            access_type: "open".into(),
            relay: None,
            sync_status: "local".into(),
            password_hash: None,
        };
        let info = row_to_group(&row, Some("AABBCC"), false);
        assert!(info.is_member);
        assert_eq!(info.role, "owner");
    }
}
