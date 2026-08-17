//! Bookmarks FFI module
//!
//! Thin wrapper over db-core's `BookmarkRepo` (local bookmark table).

use flutter_rust_bridge::frb;
use nostr::event::{EventBuilder, Kind, Tag};

/// Save a bookmark for an event. Returns the bookmark id (event id).
#[frb(sync, serialize)]
pub fn bookmarks_save(pubkey: String, event_id: String) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::bookmark::BookmarkRow {
        id: format!("bm:{event_id}"),
        pubkey,
        event_id,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    publish_bookmark_list(&row.pubkey);
    Ok(row.id).into()
}

/// List bookmarks for a pubkey, newest first. Returns JSON array of
/// `{id, pubkey, event_id, created_at}`.
#[frb(sync, serialize)]
pub fn bookmarks_list(pubkey: String, limit: i64, offset: i64) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::bookmark::BookmarkRepo::new(db)
            .get_user_bookmarks(&pubkey, limit, offset)?;
        Ok(rows)
    })
    .and_then(super::util::json_ok)
}

/// Delete a bookmark by id. Returns true if a row was removed.
#[frb(sync, serialize)]
pub fn bookmarks_delete(id: String) -> Result<bool, String> {
    let pubkey = super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).get_by_id(&id)
    })?
    .map(|r| r.pubkey)
    .unwrap_or_default();
    super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).delete(&id)?;
        Ok(true)
    })?;
    if !pubkey.is_empty() {
        publish_bookmark_list(&pubkey);
    }
    Ok(true)
}

/// Resolve a bookmarked event from the local DB cache.
/// Returns JSON of PostRow or empty string if not found.
#[frb(sync, serialize)]
pub fn bookmarks_resolve_post(event_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let row = soshal_db_core::repos::post::PostRepo::new(db).get_by_id(&event_id)?;
        Ok(row)
    })
    .and_then(|r| match r {
        Some(row) => super::util::json_ok(row),
        None => Ok(String::new()),
    })
}

/// Resolve many bookmarked events in one call. `ids_json` is a JSON array of
/// event ids; returns a JSON map `{"<id>": PostRow, ...}` (missing rows are
/// absent). Replaces N sequential per-bookmark FFI round-trips.
#[frb(sync, serialize)]
pub fn bookmarks_resolve_posts(ids_json: String) -> Result<String, String> {
    let ids: Vec<String> =
        serde_json::from_str(&ids_json).map_err(|e| format!("invalid ids JSON: {e}"))?;
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::post::PostRepo::new(db);
        let rows = repo.get_by_ids(&ids)?;
        let mut out = serde_json::Map::with_capacity(rows.len());
        for row in rows {
            if let Ok(value) = serde_json::to_value(&row) {
                out.insert(row.id, value);
            }
        }
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "{}".into()))
    })
}

/// Best-effort publish of the user's bookmark list (NIP-51 kind 10003) with
/// one `e` tag per saved bookmark. Signer-locked or unreachable relays leave
/// bookmarks local-only (DB write already succeeded).
fn publish_bookmark_list(pubkey: &str) {
    if super::signer::signer_pubkey().is_err() {
        return;
    }
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).get_user_bookmarks(pubkey, 1000, 0)
    })
    .unwrap_or_default();
    let mut builder = EventBuilder::new(Kind::from_u16(10003), "");
    for r in &rows {
        if let Ok(tag) = Tag::parse(vec!["e".to_string(), r.event_id.clone()]) {
            builder = builder.tag(tag);
        }
    }
    if let Ok(signed) = super::signer::sign_builder(builder) {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            let _ = rt.block_on(super::network::network_publish_event(signed));
        }
    }
}
