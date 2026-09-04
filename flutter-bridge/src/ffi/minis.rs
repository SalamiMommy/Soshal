//! Minis FFI module
//! Minis, Musicloud, custom profiles

use flutter_rust_bridge::frb;
use soshal_minis_core::events as minis_events;

fn add_tag(builder: nostr::event::EventBuilder, tag: Vec<String>) -> nostr::event::EventBuilder {
    match nostr::event::Tag::parse(tag) {
        Ok(t) => builder.tag(t),
        Err(_) => builder,
    }
}

/// Fetch known minis from the local registry: kind-31020 rows ingested by
/// the sync engine (subscription added in sync-core). Newest first. Returns
/// a JSON array of mini entries (id, pubkey, videoUrl, blobHash, mediaSize,
/// textOverlay, thumbnail, audience, createdAt).
#[frb(sync, serialize)]
pub fn minis_fetch() -> Result<String, String> {
    let json = super::db::db_query_raw(
        "SELECT id, pubkey, content, created_at, tags_json FROM posts \
         WHERE kind = 31020 AND is_deleted = 0 \
         ORDER BY created_at DESC LIMIT 200"
            .to_string(),
    )?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let tags: Vec<Vec<String>> =
            serde_json::from_str(row["tags_json"].as_str().unwrap_or("[]")).unwrap_or_default();
        let ev = soshal_nostr_core::models::NostrEvent {
            id: row["id"].as_str().unwrap_or_default().to_string(),
            pubkey: row["pubkey"].as_str().unwrap_or_default().to_string(),
            content: row["content"].as_str().unwrap_or_default().to_string(),
            tags,
            created_at: row["created_at"].as_f64().unwrap_or(0.0),
            kind: 31020,
        };
        if let Some(mapped) = minis_events::mini_from_event(&ev) {
            out.push(mapped);
        }
    }
    super::util::json_ok(out)
}

/// Publish a mini video (kind 31020). `media_source` is a local video file
/// path or an https media URL; the bytes are uploaded to the local chunk
/// store (CAS) and the event carries the feed `["media", ...]` blob tag so
/// other devices fetch it from the publisher's or any peer's cache. When
/// the source is a URL it is kept in the `url` tag as a fallback. Returns
/// the event id.
#[frb(serialize)]
pub async fn minis_publish(
    media_source: String,
    text_overlay: Option<String>,
    thumbnail: Option<String>,
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
    let url_tag = if is_url {
        media_source.clone()
    } else {
        format!("blob://{blob_hash}")
    };
    let aud = audience.unwrap_or_else(|| "public".into());
    let content = text_overlay.unwrap_or_default();
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(31020), content.clone());
    builder = add_tag(builder, vec!["url".to_string(), url_tag]);
    builder = add_tag(
        builder,
        vec![
            "media".to_string(),
            "video".to_string(),
            format!("blob://{blob_hash}"),
            blob_hash,
            media_size.to_string(),
        ],
    );
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
    let signed = super::signer::sign_builder(builder)?;
    let event: serde_json::Value =
        serde_json::from_str(&signed).map_err(|e| format!("parse signed event: {e}"))?;
    let id = event["id"].as_str().unwrap_or_default().to_string();
    let author_pubkey = event["pubkey"].as_str().unwrap_or_default().to_string();
    let tags_json = serde_json::to_string(&event["tags"]).unwrap_or_else(|_| "[]".to_string());
    let _ = super::db::upsert_post_row(
        id.clone(),
        author_pubkey,
        content,
        31020,
        soshal_common_core::format::now_secs(),
        tags_json,
        None,
    );
    let _ = super::network::network_publish_event(signed).await?;
    Ok(id)
}

fn wasm_stub_err(wasm_bytes_hex: &str) -> String {
    if hex::decode(wasm_bytes_hex).is_err() {
        return "invalid wasm hex".to_string();
    }
    "wasm runtime unavailable: WASI component host on roadmap, runtime simulated".to_string()
}

/// Execute a WASI 0.2 Wasm content filter component plugin (runtime itself
/// still simulated in minis-core; wasm bytes must be valid hex).
#[frb(sync, serialize)]
pub fn minis_wasm_execute_filter(
    _plugin_id: String,
    _text: String,
    wasm_bytes_hex: String,
) -> Result<String, String> {
    Err(wasm_stub_err(&wasm_bytes_hex))
}

/// Execute a WASI 0.2 Wasm feed ranker component plugin (runtime itself still
/// simulated in minis-core; wasm bytes must be valid hex).
#[frb(sync, serialize)]
pub fn minis_wasm_rank_feed(
    _plugin_id: String,
    _posts_json: Vec<String>,
    wasm_bytes_hex: String,
) -> Result<Vec<String>, String> {
    Err(wasm_stub_err(&wasm_bytes_hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_returns_empty_list() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = format!(
            "{}/soshal_minis_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            "fetch"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        let json = minis_fetch().unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
        assert!(rows.is_empty());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn fetch_returns_urls_from_mini_rows() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = format!(
            "{}/soshal_minis_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            "rows"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        assert!(super::super::db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('m1','pk','mini one',31020,100,'[[\"url\",\"https://mini.example/a\"],[\"image\",\"https://img.example/a.png\"]]','pending',0), \
                    ('m2','pk','mini two',31020,200,'[[\"url\",\"https://mini.example/b\"]]','pending',0), \
                    ('n1','pk','not a mini',1,300,'[]','pending',0)"
                .to_string()
        )
        .is_ok());
        let json = minis_fetch().unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["videoUrl"], "https://mini.example/b");
        assert_eq!(rows[0]["textOverlay"], "mini two");
        assert_eq!(rows[1]["videoUrl"], "https://mini.example/a");
        assert_eq!(rows[1]["thumbnail"], "https://img.example/a.png");
        assert_eq!(rows[1]["blobHash"], "");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn wasm_filter_returns_unavailable_error() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = minis_wasm_execute_filter(
            "f1".to_string(),
            "hello world".to_string(),
            "0061736d".to_string(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "wasm runtime unavailable: WASI component host on roadmap, runtime simulated"
        );
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn wasm_filter_rejects_invalid_hex() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err =
            minis_wasm_execute_filter("f1".to_string(), "hello".to_string(), "zz".to_string())
                .unwrap_err();
        assert_eq!(err, "invalid wasm hex");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn wasm_rank_returns_unavailable_error() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = minis_wasm_rank_feed(
            "r1".to_string(),
            vec!["a".to_string(), "ccc".to_string(), "bb".to_string()],
            "0061736d".to_string(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "wasm runtime unavailable: WASI component host on roadmap, runtime simulated"
        );
        super::super::signer::signer_lock().unwrap();
    }
}
