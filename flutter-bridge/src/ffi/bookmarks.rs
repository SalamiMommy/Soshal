//! Bookmarks FFI module
//!
//! Thin wrapper over db-core's `BookmarkRepo` (local bookmark table).

use flutter_rust_bridge::frb;

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
    super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).delete(&id)?;
        Ok(true)
    })
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
