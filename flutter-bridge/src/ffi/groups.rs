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
        is_private: row.access_type == "private" || row.password_hash.is_some(),
    }
}

fn member_count(group_id: &str) -> i32 {
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let counts = repo.member_count_many(&[group_id.to_string()])?;
        Ok(counts.get(group_id).copied().unwrap_or(0))
    })
    .unwrap_or(0) as i32
}

/// Fetch all groups the user belongs to (DB-backed).
#[frb(sync, serialize)]
pub fn groups_fetch_groups(user_pubkey: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let rows = repo.get_user_groups(&user_pubkey)?;
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let counts = repo.member_count_many(&ids)?;
        let groups: Vec<GroupInfo> = rows
            .iter()
            .map(|r| {
                let mut g = row_to_group(r, Some(&user_pubkey));
                g.members = counts.get(&r.id).copied().unwrap_or(0) as i32;
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
        let key = GroupRepo::new(db).get_shared_key(&group_id)?;
        Ok(match key {
            Some(k) => {
                let k = match k.strip_prefix("seal1:") {
                    Some(sealed) => {
                        let plain = soshal_crypto_core::at_rest::open_at_rest(
                            &super::signer::signer_at_rest_key()
                                .map_err(soshal_db_core::error::DbError::Migration)?,
                            sealed,
                        )
                        .map_err(soshal_db_core::error::DbError::Migration)?;
                        hex::encode(plain)
                    }
                    None => k,
                };
                group_message_envelope(&content, Some(&k))
                    .map_err(soshal_db_core::error::DbError::Migration)?
            }
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
        if !group_repo.is_member(&group_id, &sender)? {
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
    let owner = super::db::with_db_result(|db| {
        GroupRepo::new(db)
            .get_by_id(&group_id)?
            .map(|r| r.pubkey)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    if owner != admin_pubkey {
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
    let owner = super::db::with_db_result(|db| {
        GroupRepo::new(db)
            .get_by_id(&group_id)?
            .map(|r| r.pubkey)
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)
    })?;
    if owner != admin_pubkey {
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

/// List group threads, pinned-first, sorted by recency or hot engagement
/// (JSON rows with reply + reaction counts).
#[frb(sync, serialize)]
pub fn groups_threads_list(group_id: String, sort: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
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
    let now = soshal_common_core::format::now_secs();
    let id = format!("thr_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::thread::GroupThreadRow {
        id: id.clone(),
        group_id,
        title,
        body,
        author,
        created_at: now,
        is_pinned: false,
        reply_count: 0,
        reaction_count: 0,
    };
    super::db::with_db_result(|db| {
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
    let now = soshal_common_core::format::now_secs();
    let id = format!("rpl_{now}_{:x}", rand::random::<u32>());
    let row = soshal_db_core::repos::thread::GroupThreadReplyRow {
        id: id.clone(),
        thread_id,
        parent_id,
        author,
        content,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::thread::GroupThreadRepo::new(db);
        repo.add_reply(&row)?;
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
    if emoji.is_empty() || emoji.chars().count() > 16 {
        return Err("invalid reaction emoji".into());
    }
    super::db::with_db_result(|db| {
        soshal_db_core::repos::thread::GroupThreadRepo::new(db)
            .toggle_reaction(&thread_id, &reply_id, &pubkey, &emoji)
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
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).delete_channel(&channel_id)?;
        Ok(true)
    })
}

/// Mark local presence in a voice channel (audio transport is a roadmap
/// surface; this only records intent).
#[frb(sync, serialize)]
pub fn groups_voice_join(channel_id: String, pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).join(&channel_id, &pubkey)?;
        Ok(true)
    })
}

/// Clear local presence from a voice channel.
#[frb(sync, serialize)]
pub fn groups_voice_leave(channel_id: String, pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::voice::GroupVoiceRepo::new(db).leave(&channel_id, &pubkey)?;
        Ok(true)
    })
}

/// Members currently present in a voice channel (JSON rows).
#[frb(sync, serialize)]
pub fn groups_voice_presence(channel_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::voice::GroupVoiceRepo::new(db).presence(&channel_id)?;
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
    groups_get_group_info(group_id)
}

/// Change or remove group password and update private/open access type (owner only).
#[frb(sync, serialize)]
pub fn groups_set_password(
    group_id: String,
    new_password: Option<String>,
    actor_pubkey: String,
) -> Result<bool, String> {
    require_owner(&group_id, &actor_pubkey)?;
    let (access_type, password_hash) = match new_password {
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

/// Test whether candidate password matches the group's password hash.
/// Rate-limited per group (5 attempts / 30 s) so the FFI surface cannot be
/// used as an offline-style guess oracle.
#[frb(sync, serialize)]
pub fn groups_verify_password(group_id: String, password: String) -> Result<bool, String> {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    static ATTEMPTS: LazyLock<Mutex<HashMap<String, (u32, i64)>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    const MAX_ATTEMPTS: u32 = 5;
    const WINDOW_SECS: i64 = 30;

    let now = soshal_common_core::format::now_secs();
    let mut attempts = ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = attempts.entry(group_id.clone()).or_insert((0, now));
    if entry.1 + WINDOW_SECS < now {
        *entry = (0, now);
    }
    if entry.0 >= MAX_ATTEMPTS {
        return Err(format!(
            "too many password attempts for this group; retry in {}s",
            entry.1 + WINDOW_SECS - now
        ));
    }
    entry.0 += 1;
    drop(attempts);

    super::db::with_db_result(|db| {
        let repo = GroupRepo::new(db);
        let group = repo
            .get_by_id(&group_id)?
            .ok_or_else(|| soshal_db_core::error::DbError::NotFound)?;
        let stored = group.password_hash.as_deref().unwrap_or("");
        if stored.is_empty() {
            return Ok(true);
        }
        Ok(soshal_groups_core::access::verify_community_password(
            password.trim(),
            stored,
        ))
    })
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
        super::super::db::db_execute_raw_test(format!(
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
            false,
            None,
        )
        .unwrap();
    }

    #[test]
    fn test_create_group_and_fetch() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("create");
        let owner = "a".repeat(64);

        assert_eq!(groups_fetch_groups(owner.clone()).unwrap(), "[]");

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

        let list = groups_fetch_groups(owner).unwrap();
        assert!(list.contains("\"g1\""));
        assert!(list.contains("a test group"));

        let detail = groups_get_group_info("g1".to_string()).unwrap();
        assert!(detail.contains("https://example.com/pic.png"));
    }

    #[test]
    fn test_get_group_info_missing_errors() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("missing");
        assert!(groups_get_group_info("nope".to_string()).is_err());
    }

    #[test]
    fn test_join_leave_and_members() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("role_gate");
        let owner = "a".repeat(64);
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
        assert!(groups_remove_member("g3".to_string(), member, owner).unwrap());

        let members = groups_get_members("g3".to_string()).unwrap();
        assert_eq!(members.len(), 1);
    }

    #[test]
    fn test_role_ops_on_missing_group_errors() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("no_msgs");
        assert_eq!(
            groups_fetch_messages("g5".to_string(), String::new(), 20, 0).unwrap(),
            "[]"
        );
    }

    #[test]
    fn test_post_message_and_fetch_messages() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("rooms");
        let owner = "a".repeat(64);
        create_group("g6", &owner);
        super::super::signer::signer_unlock(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        )
        .unwrap();
        // The message sender (active signer) must be a member: post-message
        // now enforces membership.
        let sender_pk = super::super::signer::signer_pubkey().unwrap();
        groups_join("g6".to_string(), sender_pk, None).unwrap();

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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("threads");
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        create_group("g7", &owner);
        insert_user(&member);

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
        assert!(groups_threads_pin(thread_id.clone(), true, owner.clone()).unwrap());
        let pinned = groups_threads_list("g7".to_string(), "newest".to_string()).unwrap();
        assert!(pinned.contains("\"is_pinned\":true"));

        assert!(groups_threads_delete(thread_id.clone(), owner).unwrap());
        assert_eq!(groups_threads_replies(thread_id).unwrap(), "[]");
        assert_eq!(
            groups_threads_list("g7".to_string(), "newest".to_string()).unwrap(),
            "[]"
        );
    }

    #[test]
    fn test_thread_reactions_and_popular_sort() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("thread_reacts");
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        let other = "c".repeat(64);
        create_group("g9", &owner);
        insert_user(&member);
        insert_user(&other);

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

        assert!(
            groups_threads_react(old.clone(), String::new(), member.clone(), "👍".to_string(),)
                .unwrap()
        );
        assert!(
            groups_threads_react(old.clone(), String::new(), other.clone(), "👍".to_string(),)
                .unwrap()
        );
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
        assert!(groups_threads_react(old.clone(), rpl.clone(), other, "🔥".to_string(),).unwrap());
        let summary = groups_threads_reactions(old.clone(), member.clone()).unwrap();
        assert!(summary.contains(&format!("\"reply_id\":\"{}\"", rpl)));
        assert!(summary.contains("\"emoji\":\"🔥\""));

        // Empty/oversized emoji rejected.
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
    }

    #[test]
    fn test_voice_channels_and_presence() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("voice");
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        create_group("g8", &owner);
        insert_user(&member);

        assert_eq!(groups_voice_channels_list("g8".to_string()).unwrap(), "[]");
        let denied =
            groups_voice_channels_create("g8".to_string(), "Lounge".to_string(), member.clone());
        assert!(denied.unwrap_err().contains("only the group owner"));
        let ch =
            groups_voice_channels_create("g8".to_string(), "Lounge".to_string(), owner.clone())
                .unwrap();
        assert!(ch.starts_with("vc_"));

        assert!(groups_voice_join(ch.clone(), member.clone()).unwrap());
        assert!(groups_voice_join(ch.clone(), owner.clone()).unwrap());
        let presence = groups_voice_presence(ch.clone()).unwrap();
        assert!(presence.contains(&format!("\"pubkey\":\"{}\"", member)));
        assert!(presence.contains(&format!("\"pubkey\":\"{}\"", owner)));

        assert!(groups_voice_leave(ch.clone(), member.clone()).unwrap());
        let presence = groups_voice_presence(ch.clone()).unwrap();
        assert!(!presence.contains(&format!("\"pubkey\":\"{}\"", member)));

        assert!(groups_voice_channels_delete(ch.clone(), member).is_err());
        assert!(groups_voice_channels_delete(ch.clone(), owner).unwrap());
        assert_eq!(groups_voice_channels_list("g8".to_string()).unwrap(), "[]");
        assert_eq!(groups_voice_presence(ch).unwrap(), "[]");
    }

    #[test]
    fn test_admin_ops_on_missing_rows_error() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("missing_rows");
        let owner = "a".repeat(64);
        assert!(groups_rooms_delete("nope".to_string(), owner.clone()).is_err());
        assert!(groups_threads_delete("nope".to_string(), owner.clone()).is_err());
        assert!(groups_threads_pin("nope".to_string(), true, owner.clone()).is_err());
        assert!(groups_voice_channels_delete("nope".to_string(), owner).is_err());
    }

    #[test]
    fn test_post_message_locked_signer_errors() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("msg_locked");
        super::super::signer::signer_lock().unwrap();
        assert!(groups_post_message("g5".to_string(), String::new(), "hi".to_string()).is_err());
    }

    #[test]
    fn test_private_group_password_flow() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _db = TestDb::init("set_pwd");
        let owner = "a".repeat(64);
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
    }
}
