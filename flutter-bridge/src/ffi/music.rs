//! Musicloud FFI module
//!
//! Music publishing/sharing on nostr: kind-31022 tracks (addressable,
//! `d` = soshal_music_<ts>), kind-1 text-note shares, comments via the
//! minis address scheme. Fetch results are signature-verified and mapped
//! through minis-core.

use flutter_rust_bridge::frb;
use serde::Serialize;
use soshal_db_core::repos::musicloud::{
    MusicloudCommentRepo, MusicloudCommentRow, MusicloudRepo, MusicloudRow,
};
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
    super::signer::signer_pubkey()?;
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
    let _ = super::network::network_publish_event(signed.clone()).await?;
    // Mirror the published track into the local musiclouds table so
    // `music_fetch` returns it immediately (relay echo lag) and across app
    // restarts (persistent). Best-effort: a failed insert must not fail the
    // publish — the track still reaches the network.
    if let Ok(e) = nostr::event::Event::from_json(&signed) {
        if let Some(track) =
            minis_events::musicloud_from_event(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            persist_published_track(&track);
        }
    }
    Ok(id)
}

/// Map a musicloud track JSON value (`musicloud_from_event` shape) to a
/// local `musiclouds` row and upsert it. Best-effort — errors are logged.
fn persist_published_track(track: &serde_json::Value) {
    let empty = |k: &str| track[k].as_str().unwrap_or_default().to_string();
    let hashtags = match track["hashtags"].as_array() {
        Some(a) => serde_json::to_string(a).unwrap_or_else(|_| "[]".to_string()),
        None => "[]".to_string(),
    };
    let title = empty("title");
    let thumbnail = empty("thumbnail");
    let audience = empty("audience");
    let row = MusicloudRow {
        id: empty("id"),
        pubkey: empty("pubkey"),
        audio_url: empty("audioUrl"),
        title: if title.is_empty() { None } else { Some(title) },
        duration: None,
        text_overlay: None,
        thumbnail: if thumbnail.is_empty() {
            None
        } else {
            Some(thumbnail)
        },
        likes: 0,
        liked: false,
        bookmarked: false,
        audience: if audience.is_empty() {
            "public".to_string()
        } else {
            audience
        },
        blob_hash: empty("blobHash"),
        media_size: track["mediaSize"].as_i64().unwrap_or(0),
        hashtags,
        d: empty("d"),
        created_at: track["createdAt"].as_i64().unwrap_or(0),
    };
    if let Err(e) = super::db::with_db_string(|db| {
        MusicloudRepo::new(db)
            .upsert(&row)
            .map_err(|e| e.to_string())?;
        Ok(())
    }) {
        eprintln!("musicloud persist own track: {e}");
    }
}

/// Maps a local `musiclouds` row back to the musicloud track JSON shape
/// (`musicloud_from_event` / `MusicTrack.fromJson`: id, pubkey, audioUrl,
/// blobHash, mediaSize, title, thumbnail, hashtags, d, audience, createdAt).
fn musicloud_row_to_json(r: &MusicloudRow) -> serde_json::Value {
    let hashtags: serde_json::Value =
        serde_json::from_str(&r.hashtags).unwrap_or_else(|_| serde_json::json!([]));
    serde_json::json!({
        "id": r.id.as_str(),
        "pubkey": r.pubkey.as_str(),
        "audioUrl": r.audio_url.as_str(),
        "blobHash": r.blob_hash.as_str(),
        "mediaSize": r.media_size,
        "title": r.title.clone().unwrap_or_default(),
        "thumbnail": r.thumbnail.clone().unwrap_or_default(),
        "hashtags": hashtags,
        "d": r.d.as_str(),
        "audience": r.audience.as_str(),
        "createdAt": r.created_at,
    })
}

/// Merges local own tracks with relay-fetched tracks: dedup by event id
/// (local wins — it must appear even while the relay echo lags), newest
/// first, capped at [limit].
fn merge_tracks(
    mut local: Vec<serde_json::Value>,
    relay: Vec<serde_json::Value>,
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut seen: std::collections::HashSet<String> = local
        .iter()
        .filter_map(|t| t["id"].as_str().map(str::to_ascii_lowercase))
        .collect();
    for t in relay {
        let id = t["id"].as_str().unwrap_or("").to_ascii_lowercase();
        if !id.is_empty() && seen.insert(id) {
            local.push(t);
        }
    }
    minis_events::sort_by_created_desc(&mut local);
    local.truncate(limit);
    local
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
    if let Some(a) = author.clone() {
        filter["authors"] = serde_json::json!([a]);
    } else if let Some(a) = super::identity::resolve_audience_authors(&audience)? {
        filter["authors"] = serde_json::json!(a);
    }
    // Local mirror of the active account's own published tracks (fed by
    // `music_publish`): they must appear instantly and survive restarts even
    // before the relay echoes them back. Offline (no relay client / query
    // failure) this is the fallback, so the Browse tab never blanks.
    let local = match super::signer::signer_pubkey() {
        Ok(my_pk) => super::db::with_db_string(|db| {
            let rows = MusicloudRepo::new(db)
                .list_by_author(&my_pk, limit.min(100) as i64)
                .map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for r in rows {
                if !audience.is_empty() && r.audience != audience {
                    continue;
                }
                if let Some(a) = &author {
                    if !a.eq_ignore_ascii_case(&r.pubkey) {
                        continue;
                    }
                }
                out.push(musicloud_row_to_json(&r));
            }
            Ok(out)
        })
        .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let raw = match super::network::network_query_events(filter.to_string()).await {
        Ok(raw) => raw,
        Err(e) => {
            if local.is_empty() {
                return Err(e);
            }
            return super::util::json_ok(local);
        }
    };
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut relay_tracks = Vec::new();
    for e in events {
        if !soshal_nostr_core::models::verify_event(&e) {
            continue;
        }
        if let Some(mapped) =
            minis_events::musicloud_from_event(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            relay_tracks.push(mapped);
        }
    }
    let merged = merge_tracks(local, relay_tracks, limit.min(100) as usize);
    super::util::json_ok(merged)
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
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content.clone());
    for tag in [
        vec!["E".to_string(), addr.clone()],
        vec!["a".to_string(), addr.clone()],
        vec!["p".to_string(), track_pubkey],
    ] {
        builder = add_tag(builder, tag);
    }
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let _ = super::network::network_publish_event(signed).await?;
    // Mirror the comment into the local `musicloud_comments` table (keyed by
    // the track address) so the comment thread shows it immediately instead of
    // waiting for the relay echo. Best-effort.
    let pubkey = event["pubkey"].as_str().unwrap_or_default().to_string();
    let created_at = event["created_at"]
        .as_i64()
        .unwrap_or_else(soshal_common_core::format::now_secs);
    if let Err(e) = super::db::with_db_string(|db| {
        MusicloudCommentRepo::new(db)
            .insert(&MusicloudCommentRow {
                id: id.clone(),
                track_id: addr,
                pubkey,
                content,
                created_at,
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }) {
        eprintln!("musicloud persist own comment: {e}");
    }
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
    // Local mirror of this device's own comments (fed by `music_comment`), so
    // the thread shows them immediately; fallback when the relay query fails.
    let local: Vec<minis_events::MiniEventOut> = super::db::with_db_string(|db| {
        let rows = MusicloudCommentRepo::new(db)
            .list_by_track(&addr, 100)
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|c| minis_events::MiniEventOut {
                id: c.id.clone(),
                pubkey: c.pubkey.clone(),
                video_url: String::new(),
                blob_hash: String::new(),
                media_size: 0,
                text_overlay: c.content.clone(),
                thumbnail: String::new(),
                audience: String::new(),
                created_at: c.created_at as u64,
            })
            .collect::<Vec<_>>())
    })
    .unwrap_or_default();
    let raw = match super::network::network_query_events(filter).await {
        Ok(raw) => raw,
        Err(e) => {
            if local.is_empty() {
                return Err(e);
            }
            let mut out = local;
            minis_events::sort_minis_desc(&mut out);
            return super::util::json_ok(out);
        }
    };
    let events: Vec<nostr::event::Event> =
        serde_json::from_str(&raw).map_err(|e| format!("parse query result: {e}"))?;
    let mut out = local;
    let mut seen: std::collections::HashSet<String> = out.iter().map(|m| m.id.clone()).collect();
    for e in events {
        if !soshal_nostr_core::models::verify_event(&e) {
            continue;
        }
        if let Some(mapped) =
            minis_events::mini_event_out(&soshal_nostr_core::models::NostrEvent::from(&e))
        {
            if seen.insert(mapped.id.clone()) {
                out.push(mapped);
            }
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
/// Save a track into `saved_content` for the active account.
#[frb(sync, serialize)]
pub fn music_save(track_json: String) -> Result<bool, String> {
    let _my_pk = super::signer::signer_pubkey()?;
    let t: serde_json::Value =
        serde_json::from_str(&track_json).map_err(|e| format!("parse track json: {e}"))?;
    let id = t["id"].as_str().unwrap_or_default().trim().to_string();
    if id.is_empty() || id.len() > 128 {
        return Err("track id missing or invalid".into());
    }
    let blob_hash = t["blobHash"].as_str().unwrap_or_default().to_string();
    let host_ready = if blob_hash.len() == 64 {
        ChunkStore::new(ChunkStore::default_root()).contains(&blob_hash)
    } else {
        false
    };
    let hashtags = t["hashtags"].as_array().cloned().unwrap_or_default();
    let pubkey = t["pubkey"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    super::db::with_db_result(|db| {
        SavedContentRepo::new(db).upsert(&SavedContentRow {
            kind: 31022,
            id: id.clone(),
            pubkey,
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
    let _my_pk = super::signer::signer_pubkey()?;
    let track_id = track_id.trim();
    if track_id.is_empty() || track_id.len() > 128 {
        return Err("track_id must be between 1 and 128 chars".into());
    }
    super::db::with_db_result(|db| SavedContentRepo::new(db).delete(31022, track_id))?;
    Ok(true)
}

/// Return saved tracks as a JSON array of musicloud-shaped entries: id,
#[derive(Serialize)]
struct SavedTrackDto<'a> {
    id: &'a str,
    pubkey: &'a str,
    #[serde(rename = "audioUrl")]
    audio_url: &'a str,
    #[serde(rename = "blobHash")]
    blob_hash: &'a str,
    #[serde(rename = "mediaSize")]
    media_size: i64,
    title: &'a str,
    thumbnail: &'a str,
    hashtags: serde_json::Value,
    d: &'a str,
    audience: &'a str,
    #[serde(rename = "createdAt")]
    created_at: i64,
    #[serde(rename = "hostReady")]
    host_ready: bool,
    #[serde(rename = "savedAt")]
    saved_at: i64,
}

/// List saved musicloud tracks for the active account as JSON: id,
/// pubkey, audioUrl, blobHash, mediaSize, title, thumbnail, hashtags, d,
/// audience, createdAt — plus hostReady and savedAt. Newest saved first.
#[frb(sync, serialize)]
pub fn music_saved() -> Result<String, String> {
    let _my_pk = super::signer::signer_pubkey()?;
    let rows = super::db::with_db_result(|db| SavedContentRepo::new(db).list(31022, 200))?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let hashtags: serde_json::Value =
            serde_json::from_str(&r.hashtags).unwrap_or_else(|_| serde_json::json!([]));
        out.push(SavedTrackDto {
            id: &r.id,
            pubkey: &r.pubkey,
            audio_url: &r.media_url,
            blob_hash: &r.blob_hash,
            media_size: r.media_size,
            title: &r.title,
            thumbnail: &r.thumbnail,
            hashtags,
            d: &r.d,
            audience: &r.audience,
            created_at: r.created_at,
            host_ready: r.host_ready,
            saved_at: r.saved_at,
        });
    }
    serde_json::to_string(&out).map_err(|e| format!("serialize saved tracks: {e}"))
}

fn require_playlist_owner(
    db: &soshal_db_core::Database,
    playlist_id: &str,
) -> Result<String, String> {
    let my_pk = super::signer::signer_pubkey()?;
    let playlist = MusicloudPlaylistRepo::new(db)
        .get(playlist_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "playlist not found".to_string())?;
    if !playlist.pubkey.eq_ignore_ascii_case(&my_pk) {
        return Err("only playlist owner can modify playlist".to_string());
    }
    Ok(my_pk)
}

/// Create a playlist for the active account. Returns the new playlist id.
#[frb(sync, serialize)]
pub fn music_playlist_create(title: String, is_private: bool) -> Result<String, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("playlist title must not be empty".into());
    }
    let pubkey = super::signer::signer_pubkey()?;
    let id = super::util::uuid_like();
    super::db::with_db_result(|db| {
        MusicloudPlaylistRepo::new(db).create(&id, &pubkey, &title, is_private)
    })?;
    Ok(id)
}

#[derive(Serialize)]
struct PlaylistSummaryDto<'a> {
    id: &'a str,
    pubkey: &'a str,
    title: &'a str,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "createdAt")]
    created_at: i64,
    #[serde(rename = "trackCount")]
    track_count: i64,
}

/// List the active account's playlists as JSON: id, pubkey, title, isPrivate,
/// createdAt, trackCount. Newest first.
#[frb(sync, serialize)]
pub fn music_playlist_list() -> Result<String, String> {
    let pubkey = super::signer::signer_pubkey()?;
    let rows = super::db::with_db_result(|db| MusicloudPlaylistRepo::new(db).list(&pubkey, 200))?;
    let mut out = Vec::with_capacity(rows.len());
    for p in &rows {
        out.push(PlaylistSummaryDto {
            id: &p.id,
            pubkey: &p.pubkey,
            title: &p.title,
            is_private: p.is_private,
            created_at: p.created_at,
            track_count: p.track_count,
        });
    }
    serde_json::to_string(&out).map_err(|e| format!("serialize playlists: {e}"))
}

/// Rename a playlist owned by the active account.
#[frb(sync, serialize)]
pub fn music_playlist_rename(playlist_id: String, title: String) -> Result<bool, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("playlist title must not be empty".into());
    }
    super::db::with_db_string(|db| {
        require_playlist_owner(db, &playlist_id)?;
        MusicloudPlaylistRepo::new(db)
            .rename(&playlist_id, &title)
            .map_err(|e| e.to_string())
    })?;
    Ok(true)
}

/// Delete a playlist owned by the active account and its track rows.
#[frb(sync, serialize)]
pub fn music_playlist_delete(playlist_id: String) -> Result<bool, String> {
    super::db::with_db_string(|db| {
        require_playlist_owner(db, &playlist_id)?;
        MusicloudPlaylistRepo::new(db)
            .delete(&playlist_id)
            .map_err(|e| e.to_string())
    })?;
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
    super::db::with_db_string(|db| {
        require_playlist_owner(db, &playlist_id)?;
        MusicloudPlaylistRepo::new(db)
            .add_track(&PlaylistTrackRow {
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
            .map_err(|e| e.to_string())
    })?;
    Ok(true)
}

/// Remove a track from a playlist.
#[frb(sync, serialize)]
pub fn music_playlist_remove_track(playlist_id: String, track_id: String) -> Result<bool, String> {
    super::db::with_db_string(|db| {
        require_playlist_owner(db, &playlist_id)?;
        MusicloudPlaylistRepo::new(db)
            .remove_track(&playlist_id, &track_id)
            .map_err(|e| e.to_string())
    })?;
    Ok(true)
}

#[derive(Serialize)]
struct PlaylistTrackDto<'a> {
    id: &'a str,
    pubkey: &'a str,
    #[serde(rename = "audioUrl")]
    audio_url: &'a str,
    #[serde(rename = "blobHash")]
    blob_hash: &'a str,
    #[serde(rename = "mediaSize")]
    media_size: i64,
    title: &'a str,
    thumbnail: &'a str,
    hashtags: serde_json::Value,
    d: &'a str,
    audience: &'a str,
    #[serde(rename = "createdAt")]
    created_at: i64,
}

/// Return a playlist's tracks ordered by insertion position as a JSON array
/// of musicloud-shaped entries (id, pubkey, audioUrl, blobHash, mediaSize,
/// title, thumbnail, hashtags, d, audience, createdAt).
#[frb(sync, serialize)]
pub fn music_playlist_tracks(playlist_id: String) -> Result<String, String> {
    let caller = super::signer::signer_pubkey().ok();
    super::db::with_db_string(|db| {
        let repo = MusicloudPlaylistRepo::new(db);
        if let Some(p) = repo.get(&playlist_id).map_err(|e| e.to_string())? {
            if p.is_private && caller.as_deref() != Some(p.pubkey.as_str()) {
                return Err("playlist is private".into());
            }
        } else {
            return Err("playlist not found".into());
        }
        let rows = repo.tracks(&playlist_id, 500).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let hashtags: serde_json::Value =
                serde_json::from_str(&r.hashtags).unwrap_or_else(|_| serde_json::json!([]));
            out.push(PlaylistTrackDto {
                id: &r.track_id,
                pubkey: &r.pubkey,
                audio_url: &r.audio_url,
                blob_hash: &r.blob_hash,
                media_size: r.media_size,
                title: &r.title,
                thumbnail: &r.thumbnail,
                hashtags,
                d: &r.d,
                audience: &r.audience,
                created_at: r.created_at,
            });
        }
        serde_json::to_string(&out).map_err(|e| format!("serialize playlist tracks: {e}"))
    })
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
    fn merge_tracks_local_first_dedup_cap() {
        let mut a = track_json("a1");
        a["createdAt"] = serde_json::json!(200);
        let mut b = track_json("b2");
        b["createdAt"] = serde_json::json!(100);
        let dup = track_json("a1"); // same id as local
        let merged = merge_tracks(vec![a.clone()], vec![b.clone(), dup], 50);
        assert_eq!(merged.len(), 2, "relay dup of local id must be dropped");
        assert_eq!(merged[0]["id"], "a1", "newest first");
        assert_eq!(merged[1]["id"], "b2");
        let capped = merge_tracks(vec![a.clone()], vec![b], 1);
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0]["id"], "a1");
    }

    #[test]
    fn musicloud_row_to_json_roundtrip() {
        let row = MusicloudRow {
            id: "t1".into(),
            pubkey: "pk1".into(),
            audio_url: "blob://abcd".into(),
            title: Some("Song".into()),
            duration: None,
            text_overlay: None,
            thumbnail: Some("thumb".into()),
            likes: 0,
            liked: false,
            bookmarked: false,
            audience: "public".into(),
            blob_hash: "abcd".into(),
            media_size: 42,
            hashtags: "[\"a\",\"b\"]".into(),
            d: "soshal_music_1".into(),
            created_at: 9,
        };
        let v = musicloud_row_to_json(&row);
        assert_eq!(v["id"], "t1");
        assert_eq!(v["audioUrl"], "blob://abcd");
        assert_eq!(v["blobHash"], "abcd");
        assert_eq!(v["mediaSize"], 42);
        assert_eq!(v["hashtags"].as_array().unwrap().len(), 2);
        assert_eq!(v["d"], "soshal_music_1");
        assert_eq!(v["audience"], "public");
        assert_eq!(v["createdAt"], 9);
        assert_eq!(v["title"], "Song");
    }

    #[test]
    fn persist_track_then_fetch_offline_returns_local() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let path = init_db("musicloud_local");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let my_pk = super::super::signer::signer_pubkey().unwrap();
        let mut t = track_json("tlocal");
        t["pubkey"] = serde_json::json!(my_pk);
        t["createdAt"] = serde_json::json!(500);
        persist_published_track(&t);
        // No relay client in tests → query fails → local-only fallback returns
        // the persisted own track instead of an error.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let json = rt
            .block_on(music_fetch(50, None, "public".to_string()))
            .unwrap();
        let fetched: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0]["id"], "tlocal");
        super::super::signer::signer_lock().unwrap();
        cleanup(&path);
    }

    #[test]
    fn save_unsave_saved_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let path = init_db("saved");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let empty =
            serde_json::from_str::<Vec<serde_json::Value>>(&music_saved().unwrap()).unwrap();
        assert!(empty.is_empty());
        let json = track_json("t1");
        assert!(!music_save(json.to_string()).unwrap());
        let saved: Vec<serde_json::Value> = serde_json::from_str(&music_saved().unwrap()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["title"], "Song");
        assert_eq!(saved[0]["hostReady"], false);
        assert!(saved[0]["savedAt"].as_i64().unwrap() > 0);
        assert!(!music_save(json.to_string()).unwrap());
        assert!(music_unsave("t1".to_string()).unwrap());
        let after: Vec<serde_json::Value> = serde_json::from_str(&music_saved().unwrap()).unwrap();
        assert!(after.is_empty());
        super::super::signer::signer_lock().unwrap();
        cleanup(&path);
    }

    #[test]
    fn playlist_lifecycle() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let path = init_db("playlist");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

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
        super::super::signer::signer_lock().unwrap();
        cleanup(&path);
    }
}
