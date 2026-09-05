//! Musicloud FFI module
//!
//! Music publishing/sharing on nostr: kind-31022 tracks (addressable,
//! `d` = soshal_music_<ts>), kind-1 text-note shares, comments via the
//! minis address scheme. Fetch results are signature-verified and mapped
//! through minis-core.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::saved::{
    MusicloudPlaylistRepo, PlaylistTrackRow, SavedContentRepo, SavedContentRow,
};
use soshal_media_core::cas::ChunkStore;
use soshal_minis_core::events as minis_events;

fn add_tag(builder: nostr::event::EventBuilder, tag: Vec<String>) -> nostr::event::EventBuilder {
    match nostr::event::Tag::parse(tag) {
        Ok(t) => builder.tag(t),
        Err(_) => builder,
    }
}

/// Publish a track (kind 31022). `media_source` is a local audio file path
/// or an https media URL; the bytes are uploaded to the local chunk store
/// (CAS) and the event carries the feed `["media", ...]` blob tag so other
/// devices can fetch it from the publisher's or any peer's cache. When the
/// source is a URL it is kept in the `url` tag as a fallback for devices
/// that cannot reach a blob holder. Returns the event id.
#[frb(serialize)]
pub async fn music_publish(
    media_source: String,
    title: Option<String>,
    thumbnail: Option<String>,
    hashtags: Vec<String>,
    audience: Option<String>,
) -> Result<String, String> {
    if media_source.is_empty() {
        return Err("media_source must be a local file path or https URL".into());
    }
    let is_url = soshal_media_core::source::is_url_source(&media_source);
    if is_url {
        if !soshal_content_core::url::is_valid_media_url(&media_source) {
            return Err("media_source must be a valid https media URL".into());
        }
    } else if std::fs::metadata(&media_source).is_err() {
        return Err("media file not found".into());
    }
    let manifest_json = super::media::media_upload_blob_file(media_source.clone()).await?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_json).map_err(|e| format!("parse blob manifest: {e}"))?;
    let blob_hash = manifest["blob_hash"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let media_size: u64 = manifest["total_size"].as_u64().unwrap_or(0);
    if blob_hash.len() != 64 {
        return Err("blob upload produced an invalid hash".into());
    }
    // url tag = original source when remote (peer fallback), else blob ref.
    let url_tag = if is_url {
        media_source.clone()
    } else {
        format!("blob://{blob_hash}")
    };
    let aud = audience.unwrap_or_else(|| "public".into());
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31022), String::new());
    builder = add_tag(
        builder,
        vec![
            "d".to_string(),
            format!("soshal_music_{}", nostr::types::Timestamp::now().as_secs()),
        ],
    );
    builder = add_tag(builder, vec!["url".to_string(), url_tag]);
    builder = add_tag(
        builder,
        vec![
            "media".to_string(),
            "audio".to_string(),
            format!("blob://{blob_hash}"),
            blob_hash,
            media_size.to_string(),
        ],
    );
    if let Some(t) = title {
        if !t.is_empty() {
            builder = add_tag(builder, vec!["title".to_string(), t]);
        }
    }
    if let Some(th) = thumbnail {
        if !th.is_empty() {
            let thumb_tag = if soshal_media_core::source::is_url_source(&th) {
                if !soshal_content_core::url::is_valid_media_url(&th) {
                    return Err("thumbnail must be a valid https media URL".into());
                }
                th
            } else if std::fs::metadata(&th).is_ok() {
                let t_manifest = super::media::media_upload_blob_file(th.clone()).await?;
                let t_json: serde_json::Value = serde_json::from_str(&t_manifest)
                    .map_err(|e| format!("parse thumbnail manifest: {e}"))?;
                let t_hash = t_json["blob_hash"].as_str().unwrap_or_default().to_string();
                format!("blob://{t_hash}")
            } else {
                return Err("thumbnail file not found".into());
            };
            builder = add_tag(builder, vec!["image".to_string(), thumb_tag]);
        }
    }
    builder = add_tag(builder, vec!["audience".to_string(), aud]);
    for h in hashtags.iter().take(10) {
        let h = h.trim_start_matches('#').to_string();
        if !h.is_empty() {
            builder = add_tag(builder, vec!["t".to_string(), h]);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch tracks (kind 31022), optionally by author. Returns JSON array of
/// musicloud entries, newest first.
#[frb(serialize)]
pub async fn music_fetch(
    limit: u64,
    author: Option<String>,
    audience: String,
) -> Result<String, String> {
    let mut filter = serde_json::json!({
        "kinds": [31022],
        "limit": limit.min(100),
    });
    if let Some(a) = author {
        filter["authors"] = serde_json::json!([a]);
    } else if let Some(a) = super::identity::resolve_audience_authors(&audience)? {
        filter["authors"] = serde_json::json!(a);
    }
    let raw = super::network::network_query_events(filter.to_string()).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    for e in events {
        if e.verify().is_err() {
            continue;
        }
        if let Some(mapped) =
            minis_events::musicloud_from_event(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            out.push(mapped);
        }
    }
    minis_events::sort_by_created_desc(&mut out);
    super::util::json_ok(out)
}

/// Share a track as a kind-1 text note with the standard track tags.
#[frb(serialize)]
pub async fn music_share_to_feed(
    track_id: String,
    track_pubkey: String,
    track_d: String,
    message: String,
    hashtags: Vec<String>,
) -> Result<String, String> {
    let content = message.trim().to_string();
    if content.is_empty() || content.len() > 64000 {
        return Err("message must be 1-64000 chars".into());
    }
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    for tag in [
        vec!["e".to_string(), track_id.clone()],
        vec!["p".to_string(), track_pubkey.clone()],
        vec!["k".to_string(), "31022".to_string()],
        vec!["a".to_string(), format!("31022:{track_pubkey}:{track_d}")],
    ] {
        builder = add_tag(builder, tag);
    }
    for h in hashtags.iter().take(10) {
        let h = h.trim_start_matches('#').to_string();
        if !h.is_empty() {
            builder = add_tag(builder, vec!["t".to_string(), h]);
        }
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Publish a comment on a track (kind 1 with `E`/`a` tags to the track).
#[frb(serialize)]
pub async fn music_comment(
    track_kind: u16,
    track_pubkey: String,
    track_d: String,
    content: String,
) -> Result<String, String> {
    let addr = minis_events::musicloud_comment_addr(track_kind, &track_pubkey, &track_d);
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    for tag in [
        vec!["E".to_string(), addr.clone()],
        vec!["a".to_string(), addr],
        vec!["p".to_string(), track_pubkey],
    ] {
        builder = add_tag(builder, tag);
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

/// Fetch comments (kind 1 with `E` = track address) for a track.
/// Returns JSON array of mini event outputs.
#[frb(serialize)]
pub async fn music_comments(
    track_kind: u16,
    track_pubkey: String,
    track_d: String,
) -> Result<String, String> {
    let addr = minis_events::musicloud_comment_addr(track_kind, &track_pubkey, &track_d);
    let filter = serde_json::json!({
        "kinds": [1],
        "#E": [addr.clone()],
        "limit": 100,
    })
    .to_string();
    let raw = super::network::network_query_events(filter).await?;
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = Vec::new();
    for e in events {
        if e.verify().is_err() {
            continue;
        }
        if let Some(mapped) =
            minis_events::mini_event_out(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            out.push(mapped);
        }
    }
    minis_events::sort_minis_desc(&mut out);
    super::util::json_ok(out)
}

/// Persist a track (kind-31022) from a musicloud-shaped JSON object (the same
/// shape `music_fetch` returns: id, pubkey, audioUrl, blobHash, mediaSize,
/// title, thumbnail, hashtags, d, audience, createdAt) into `saved_content`.
/// Returns true when the audio blob is already in the local chunk store
/// (host-ready). Materialize remote bytes into the CAS before saving if
/// re-hosting is intended.
#[frb(sync, serialize)]
pub fn music_save(track_json: String) -> Result<bool, String> {
    let t: serde_json::Value =
        serde_json::from_str(&track_json).map_err(|e| format!("parse track json: {e}"))?;
    let id = t["id"].as_str().unwrap_or_default().to_string();
    if id.is_empty() {
        return Err("track id missing".into());
    }
    let blob_hash = t["blobHash"].as_str().unwrap_or_default().to_string();
    let host_ready = if blob_hash.len() == 64 {
        ChunkStore::new(ChunkStore::default_root()).contains(&blob_hash)
    } else {
        false
    };
    let hashtags = t["hashtags"].as_array().cloned().unwrap_or_default();
    super::db::with_db_result(|db| {
        SavedContentRepo::new(db).upsert(&SavedContentRow {
            kind: 31022,
            id: id.clone(),
            pubkey: t["pubkey"].as_str().unwrap_or_default().to_string(),
            d: t["d"].as_str().unwrap_or_default().to_string(),
            media_type: "audio".to_string(),
            media_url: t["audioUrl"].as_str().unwrap_or_default().to_string(),
            text_overlay: String::new(),
            title: t["title"].as_str().unwrap_or_default().to_string(),
            thumbnail: t["thumbnail"].as_str().unwrap_or_default().to_string(),
            blob_hash,
            media_size: t["mediaSize"].as_i64().unwrap_or(0),
            audience: t["audience"].as_str().unwrap_or("public").to_string(),
            hashtags: serde_json::to_string(&hashtags).unwrap_or_else(|_| "[]".to_string()),
            host_ready,
            created_at: t["createdAt"].as_i64().unwrap_or(0),
            saved_at: soshal_common_core::format::now_secs(),
        })
    })?;
    Ok(host_ready)
}

/// Remove a saved track (kind-31022) from `saved_content`.
#[frb(sync, serialize)]
pub fn music_unsave(track_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| SavedContentRepo::new(db).delete(31022, &track_id))?;
    Ok(true)
}

/// Return saved tracks as a JSON array of musicloud-shaped entries: id,
/// pubkey, audioUrl, blobHash, mediaSize, title, thumbnail, hashtags, d,
/// audience, createdAt — plus hostReady and savedAt. Newest saved first.
#[frb(sync, serialize)]
pub fn music_saved() -> Result<String, String> {
    let rows = super::db::with_db_result(|db| SavedContentRepo::new(db).list(31022, 200))?;
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            let hashtags: Vec<serde_json::Value> =
                serde_json::from_str(&r.hashtags).unwrap_or_default();
            serde_json::json!({
                "id": r.id, "pubkey": r.pubkey, "audioUrl": r.media_url,
                "blobHash": r.blob_hash, "mediaSize": r.media_size,
                "title": r.title, "thumbnail": r.thumbnail, "hashtags": hashtags,
                "d": r.d, "audience": r.audience, "createdAt": r.created_at,
                "hostReady": r.host_ready, "savedAt": r.saved_at,
            })
        })
        .collect();
    super::util::json_ok(out)
}

/// Create a playlist for the active account. Returns the new playlist id.
#[frb(sync, serialize)]
pub fn music_playlist_create(title: String, is_private: bool) -> Result<String, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("playlist title must not be empty".into());
    }
    let id = super::util::uuid_like();
    let pubkey = super::db::active_pubkey().unwrap_or_default();
    super::db::with_db_result(|db| {
        MusicloudPlaylistRepo::new(db).create(&id, &pubkey, &title, is_private)
    })?;
    Ok(id)
}

/// List the active account's playlists as JSON: id, pubkey, title, isPrivate,
/// createdAt, trackCount. Newest first.
#[frb(sync, serialize)]
pub fn music_playlist_list() -> Result<String, String> {
    let pubkey = super::db::active_pubkey().unwrap_or_default();
    let rows = super::db::with_db_result(|db| MusicloudPlaylistRepo::new(db).list(&pubkey, 200))?;
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "pubkey": p.pubkey, "title": p.title,
                "isPrivate": p.is_private, "createdAt": p.created_at,
                "trackCount": p.track_count,
            })
        })
        .collect();
    super::util::json_ok(out)
}

/// Rename a playlist owned by the active account.
#[frb(sync, serialize)]
pub fn music_playlist_rename(playlist_id: String, title: String) -> Result<bool, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("playlist title must not be empty".into());
    }
    super::db::with_db_result(|db| MusicloudPlaylistRepo::new(db).rename(&playlist_id, &title))?;
    Ok(true)
}

/// Delete a playlist owned by the active account and its track rows.
#[frb(sync, serialize)]
pub fn music_playlist_delete(playlist_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| MusicloudPlaylistRepo::new(db).delete(&playlist_id))?;
    Ok(true)
}

/// Add a track (musicloud-shaped JSON, see `music_save`) to a playlist.
/// Tracks are de-duplicated by track id within a playlist.
#[frb(sync, serialize)]
pub fn music_playlist_add_track(playlist_id: String, track_json: String) -> Result<bool, String> {
    let t: serde_json::Value =
        serde_json::from_str(&track_json).map_err(|e| format!("parse track json: {e}"))?;
    let track_id = t["id"].as_str().unwrap_or_default().to_string();
    if track_id.is_empty() {
        return Err("track id missing".into());
    }
    let hashtags = t["hashtags"].as_array().cloned().unwrap_or_default();
    super::db::with_db_result(|db| {
        MusicloudPlaylistRepo::new(db).add_track(&PlaylistTrackRow {
            playlist_id: playlist_id.clone(),
            track_id: track_id.clone(),
            pubkey: t["pubkey"].as_str().unwrap_or_default().to_string(),
            d: t["d"].as_str().unwrap_or_default().to_string(),
            title: t["title"].as_str().unwrap_or_default().to_string(),
            thumbnail: t["thumbnail"].as_str().unwrap_or_default().to_string(),
            audio_url: t["audioUrl"].as_str().unwrap_or_default().to_string(),
            blob_hash: t["blobHash"].as_str().unwrap_or_default().to_string(),
            media_size: t["mediaSize"].as_i64().unwrap_or(0),
            audience: t["audience"].as_str().unwrap_or("public").to_string(),
            hashtags: serde_json::to_string(&hashtags).unwrap_or_else(|_| "[]".to_string()),
            created_at: t["createdAt"].as_i64().unwrap_or(0),
            position: 0,
            added_at: soshal_common_core::format::now_secs(),
        })
    })?;
    Ok(true)
}

/// Remove a track from a playlist.
#[frb(sync, serialize)]
pub fn music_playlist_remove_track(playlist_id: String, track_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        MusicloudPlaylistRepo::new(db).remove_track(&playlist_id, &track_id)
    })?;
    Ok(true)
}

/// Return a playlist's tracks ordered by insertion position as a JSON array
/// of musicloud-shaped entries (id, pubkey, audioUrl, blobHash, mediaSize,
/// title, thumbnail, hashtags, d, audience, createdAt).
#[frb(sync, serialize)]
pub fn music_playlist_tracks(playlist_id: String) -> Result<String, String> {
    let rows =
        super::db::with_db_result(|db| MusicloudPlaylistRepo::new(db).tracks(&playlist_id, 500))?;
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            let hashtags: Vec<serde_json::Value> =
                serde_json::from_str(&r.hashtags).unwrap_or_default();
            serde_json::json!({
                "id": r.track_id, "pubkey": r.pubkey, "audioUrl": r.audio_url,
                "blobHash": r.blob_hash, "mediaSize": r.media_size,
                "title": r.title, "thumbnail": r.thumbnail, "hashtags": hashtags,
                "d": r.d, "audience": r.audience, "createdAt": r.created_at,
            })
        })
        .collect();
    super::util::json_ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "pubkey": "pk",
            "audioUrl": format!("https://example.com/{id}.mp3"),
            "blobHash": "",
            "mediaSize": 12345,
            "title": "Song",
            "thumbnail": "https://example.com/{id}.png",
            "hashtags": ["a"],
            "d": "soshal_music_100",
            "audience": "public",
            "createdAt": 100,
        })
    }

    fn init_db(label: &str) -> String {
        let path = format!(
            "{}/soshal_music_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            label
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        path
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn save_unsave_saved_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = init_db("saved");
        let empty =
            serde_json::from_str::<Vec<serde_json::Value>>(&music_saved().unwrap()).unwrap();
        assert!(empty.is_empty());
        let json = track_json("t1");
        assert_eq!(music_save(json.to_string()).unwrap(), false);
        let saved: Vec<serde_json::Value> = serde_json::from_str(&music_saved().unwrap()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["title"], "Song");
        assert_eq!(saved[0]["hostReady"], false);
        assert!(saved[0]["savedAt"].as_i64().unwrap() > 0);
        assert_eq!(music_save(json.to_string()).unwrap(), false);
        assert_eq!(music_unsave("t1".to_string()).unwrap(), true);
        let after: Vec<serde_json::Value> = serde_json::from_str(&music_saved().unwrap()).unwrap();
        assert!(after.is_empty());
        cleanup(&path);
    }

    #[test]
    fn playlist_lifecycle() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = init_db("playlist");
        let pl_id = music_playlist_create("Road Trip".to_string(), true).unwrap();
        assert!(!pl_id.is_empty());
        assert!(music_playlist_create("  ".to_string(), true)
            .unwrap_err()
            .contains("title"));
        let list: Vec<serde_json::Value> =
            serde_json::from_str(&music_playlist_list().unwrap()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["title"], "Road Trip");
        assert_eq!(list[0]["trackCount"], 0);
        let json = track_json("t1");
        assert!(music_playlist_add_track(pl_id.clone(), json.to_string()).unwrap());
        assert!(music_playlist_add_track(pl_id.clone(), json.to_string()).unwrap());
        let list: Vec<serde_json::Value> =
            serde_json::from_str(&music_playlist_list().unwrap()).unwrap();
        assert_eq!(list[0]["trackCount"], 1);
        let tracks: Vec<serde_json::Value> =
            serde_json::from_str(&music_playlist_tracks(pl_id.clone()).unwrap()).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["id"], "t1");
        assert!(music_playlist_rename(pl_id.clone(), "Faves".to_string()).unwrap());
        assert!(music_playlist_delete(pl_id.clone()).unwrap());
        let after: Vec<serde_json::Value> =
            serde_json::from_str(&music_playlist_list().unwrap()).unwrap();
        assert!(after.is_empty());
        cleanup(&path);
    }
}
