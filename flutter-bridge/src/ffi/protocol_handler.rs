//! Protocol handler for app:// scheme
//!
//! Enables Flutter WebView to access media and other resources via custom URI scheme.
//! Examples:
//!   - app://media/post-123.jpg (fetch and stream image)
//!   - `app://avatar/<pubkey>` (fetch user avatar)
//!   - app://relay/status (get relay pool status)

use flutter_rust_bridge::frb;
use soshal_common_core::url::{is_private_ip_str, is_valid_media_url};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Protocol response metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProtocolResponse {
    pub content_type: String,
    pub content_length: u64,
    pub cache_control: String,
    pub etag: String,
}

/// Handle app:// protocol requests
#[frb(serialize)]
pub async fn protocol_handle_request(
    scheme: String,
    host: String,
    path: String,
) -> Result<Vec<u8>, String> {
    match scheme.as_str() {
        "app" => match host.as_str() {
            "media" => protocol_handle_media(&path).await,
            "avatar" => protocol_handle_avatar(&path).await,
            "relay" => protocol_handle_relay(&path).await,
            "cache" => protocol_handle_cache(&path).await,
            _ => Err(format!("Unknown app:// host: {}", host)).into(),
        },
        _ => Err(format!("Unsupported scheme: {}", scheme)).into(),
    }
}

/// Get response metadata for a resource
#[frb(serialize)]
pub async fn protocol_get_metadata(
    scheme: String,
    host: String,
    path: String,
) -> Result<String, String> {
    match scheme.as_str() {
        "app" => match host.as_str() {
            "media" => protocol_metadata_media(&path).and_then(super::util::json_ok),
            "avatar" => protocol_metadata_avatar(&path).and_then(super::util::json_ok),
            _ => Err("Unknown app:// host".to_string()).into(),
        },
        _ => Err(format!("Unsupported scheme: {}", scheme)).into(),
    }
}

/// Handle media:// requests (images, videos)
async fn protocol_handle_media(path: &str) -> Result<Vec<u8>, String> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if parts.is_empty() {
        return Err("Invalid media path".to_string());
    }

    match parts[0] {
        "blossom" => {
            // Fetch from Blossom server (parts[1] = URL or hash)
            if parts.len() < 2 {
                return Err("Missing blossom URL".to_string());
            }
            protocol_fetch_from_blossom(parts[1]).await
        }
        "local" => {
            // Load from local cache (parts[1] = filename)
            if parts.len() < 2 {
                return Err("Missing local filename".to_string());
            }
            protocol_load_from_cache(parts[1])
        }
        _ => Err("Unknown media source".to_string()),
    }
}

/// Fetch media from Blossom server
async fn protocol_fetch_from_blossom(url_or_hash: &str) -> Result<Vec<u8>, String> {
    // L1 fix: if the caller supplied a bare hash (no scheme), return a clear
    // error instead of constructing a dead-end URL pointing at a hardcoded
    // example domain. The caller must supply a complete, absolute URL.
    let url = if url_or_hash.contains("://") {
        url_or_hash.to_string()
    } else {
        return Err(
            "Blossom blob hash without a server URL is not supported via this path; \
             supply a full https:// URL"
                .to_string(),
        );
    };

    if !is_valid_media_url(&url) {
        return Err("Invalid media URL".to_string());
    }

    let (server, hash) = match url.find("://") {
        Some(scheme_end) => {
            let rest = &url[scheme_end + 3..];
            match rest.find('/') {
                Some(i) => (
                    url[..scheme_end + 3 + i].to_string(),
                    rest[i + 1..].to_string(),
                ),
                None => return Err("media URL must point to a blob path".to_string()),
            }
        }
        None => return Err("media URL must be absolute".to_string()),
    };

    let client = soshal_media_core::blossom::BlossomClient::new_pinned_resolve(&server).await?;
    match client.download(&hash).await {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Fetch failed: {}", e)),
    }
}

/// Load media from local cache directory.
///
/// # Security (M1 fix — TOCTOU)
/// The original code used `canonicalize(cache_dir.join(filename))`, which
/// requires the file to exist at canonicalize time. Between the check and the
/// subsequent `fs::read`, a local attacker could swap the file for a symlink
/// pointing outside the cache dir (classic TOCTOU / symlink-race).
///
/// The fix: validate that `filename` contains no path separators or
/// dot-traversal sequences (so no symlink race is possible via the filename
/// itself), canonicalize only the **parent** cache directory (which always
/// exists), then join the validated filename onto the canonical parent.
/// The resulting path cannot escape the cache dir regardless of what happens
/// to the file between the check and the `read` call.
fn protocol_load_from_cache(filename: &str) -> Result<Vec<u8>, String> {
    // Reject filenames with path separators or dot-traversal sequences.
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("invalid filename: path separators and '..' are not allowed".to_string());
    }
    // Reject empty filename.
    if filename.is_empty() {
        return Err("filename must not be empty".to_string());
    }

    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("soshal_flutter_cache");

    // Canonicalize the cache *directory* (not the full path) — the directory
    // always exists once created; the file need not.
    let canon_cache = cache_dir
        .canonicalize()
        .map_err(|e| format!("Cache dir: {}", e))?;

    // Join the already-validated filename component onto the canonical parent.
    // No separator in `filename` means this cannot escape `canon_cache`.
    let file_path = canon_cache.join(filename);

    // Open with O_NOFOLLOW so that a symlink placed at the target path between
    // the filename validation above and the actual open is rejected by the
    // kernel — same TOCTOU hardening as `media.rs::open_allowed_read()`.
    #[cfg(unix)]
    let data = {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&file_path)
            .map_err(|e| format!("Cache read failed: {e}"))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| format!("Cache read failed: {e}"))?;
        buf
    };
    #[cfg(not(unix))]
    let data = fs::read(&file_path).map_err(|e| format!("Cache read failed: {e}"))?;
    Ok(data)
}

/// Get metadata for media resource
fn protocol_metadata_media(path: &str) -> Result<ProtocolResponse, String> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if parts.len() < 2 {
        return Err("Invalid media path".to_string());
    }

    match parts[0] {
        "local" => {
            let filename = parts[1];
            // Reject filenames with path separators or dot-traversal sequences.
            // Mirrors the validation in protocol_load_from_cache so the metadata
            // path is as hardened as the read path.
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err(
                    "invalid filename: path separators and '..' are not allowed".to_string()
                );
            }
            if filename.is_empty() {
                return Err("filename must not be empty".to_string());
            }
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("soshal_flutter_cache");
            // Canonicalize only the *directory* (not the full path) so that
            // non-existent files don't cause canonicalize to fail with ENOENT.
            // The joined path cannot escape canon_cache because filename has
            // no separator characters.
            let canon_cache = cache_dir
                .canonicalize()
                .map_err(|e| format!("Cache dir: {e}"))?;
            let file_path = canon_cache.join(filename);

            match fs::metadata(&file_path) {
                Ok(meta) => {
                    let mime_type = infer_mime_from_path(filename);
                    // Include mtime in the ETag so stale cached responses are
                    // invalidated after an update (size-only ETags are guessable
                    // and provide no staleness detection).
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    Ok(ProtocolResponse {
                        content_type: mime_type,
                        content_length: meta.len(),
                        cache_control: "public, max-age=86400".to_string(),
                        etag: format!("{:x}-{:x}", meta.len(), mtime),
                    })
                }
                Err(e) => Err(format!("Metadata read failed: {}", e)),
            }
        }
        _ => Err("Unknown media source".to_string()),
    }
}

/// Handle avatar:// requests
async fn protocol_handle_avatar(path: &str) -> Result<Vec<u8>, String> {
    let pubkey = path.trim_start_matches('/');

    if pubkey.is_empty() {
        return Err("Missing pubkey".to_string());
    }

    // Look up the user's `picture` URL from the local profile cache.
    let picture = super::db::with_db_result(|db| {
        Ok(soshal_db_core::repos::user::UserRepo::new(db)
            .get_by_pubkey(pubkey)?
            .and_then(|u| u.picture))
    })
    .ok()
    .flatten();

    if let Some(url) = picture {
        if let Ok(bytes) = fetch_avatar_bytes(&url).await {
            return Ok(bytes);
        }
    }

    // Fallback: deterministic identicon derived from the pubkey.
    Ok(soshal_media_core::identicon::identicon_png(pubkey))
}

/// Fetch avatar bytes over HTTPS with a 5 MiB cap. `is_valid_media_url`
/// rejects private/loopback hostnames (pre-check); resolved addresses are
/// re-checked and pinned against DNS rebinding (SSRF guard).
async fn fetch_avatar_bytes(url: &str) -> Result<Vec<u8>, String> {
    if !is_valid_media_url(url) {
        return Err("Invalid avatar URL".to_string());
    }
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("avatar URL parse: {e}"))?;
    let host = parsed
        .host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| "avatar URL has no host".to_string())?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "avatar URL has no port".to_string())?;
    let mut pinned_addrs: Vec<std::net::SocketAddr> = Vec::new();
    match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(addrs) => {
            for addr in addrs {
                if is_private_ip_str(&addr.ip().to_string()) {
                    return Err("avatar URL resolves to an internal address".to_string());
                }
                pinned_addrs.push(addr);
            }
        }
        Err(_) => return Err("avatar URL does not resolve".to_string()),
    }
    if pinned_addrs.is_empty() {
        return Err("avatar URL does not resolve".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(&host, &pinned_addrs)
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("avatar fetch: {e}"))?;
    let max = 5 * 1024 * 1024;
    let mut bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("avatar read: {e}"))?
    {
        if bytes.len() + chunk.len() > max {
            return Err("avatar too large".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Get metadata for avatar
fn protocol_metadata_avatar(path: &str) -> Result<ProtocolResponse, String> {
    let pubkey = path.trim_start_matches('/');
    let length = soshal_media_core::identicon::identicon_png(pubkey).len() as u64;
    Ok(ProtocolResponse {
        content_type: "image/png".to_string(),
        content_length: length,
        cache_control: "public, max-age=3600".to_string(),
        etag: "avatar".to_string(),
    })
}

/// Handle relay:// status requests (JSON response)
async fn protocol_handle_relay(path: &str) -> Result<Vec<u8>, String> {
    match path {
        "/status" => {
            // Get relay pool status and return as JSON
            match super::network::network_get_relay_status().await {
                Ok(statuses) => match serde_json::to_string(&statuses) {
                    Ok(json) => Ok(json.into_bytes()),
                    Err(e) => Err(format!("Serialization failed: {}", e)),
                },
                Err(e) => Err(e),
            }
        }
        _ => Err("Unknown relay endpoint".to_string()),
    }
}

/// Handle cache:// requests (local file storage)
async fn protocol_handle_cache(path: &str) -> Result<Vec<u8>, String> {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("soshal_flutter_cache");

    let file_path = cache_dir.join(path.trim_start_matches('/'));

    // Prevent directory traversal
    let cache_dir = cache_dir
        .canonicalize()
        .map_err(|e| format!("Cache dir: {}", e))?;
    let file_path = file_path
        .canonicalize()
        .map_err(|e| format!("Cache read failed: {}", e))?;
    if !file_path.starts_with(&cache_dir) {
        return Err("Path traversal detected".to_string());
    }

    protocol_load_from_cache(path.trim_start_matches('/'))
}

/// Infer MIME type from file path
fn infer_mime_from_path(path: &str) -> String {
    let ext = path.split('.').next_back().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_from_blossom_rejects_loopback_and_bad_scheme() {
        // loopback host rejected by SSRF guard
        let err = protocol_fetch_from_blossom("http://127.0.0.1/x")
            .await
            .unwrap_err();
        assert_eq!(err, "Invalid media URL");
        // non-http(s) scheme rejected
        let err = protocol_fetch_from_blossom("ftp://host/x")
            .await
            .unwrap_err();
        assert_eq!(err, "Invalid media URL");
    }

    #[tokio::test]
    async fn test_fetch_from_blossom_requires_blob_path() {
        let err = protocol_fetch_from_blossom("https://host")
            .await
            .unwrap_err();
        assert_eq!(err, "media URL must point to a blob path");
    }

    #[tokio::test]
    async fn test_handle_media_missing_blossom_url() {
        let err = protocol_handle_media("/blossom").await.unwrap_err();
        assert_eq!(err, "Missing blossom URL");
    }

    #[test]
    fn test_metadata_media_path_validation() {
        let err = protocol_metadata_media("/x").unwrap_err();
        assert_eq!(err, "Invalid media path");
        let err = protocol_metadata_media("/blossom/x").unwrap_err();
        assert_eq!(err, "Unknown media source");
    }

    #[tokio::test]
    async fn test_handle_relay_unknown_endpoint() {
        let err = protocol_handle_relay("/nope").await.unwrap_err();
        assert_eq!(err, "Unknown relay endpoint");
    }

    #[test]
    fn test_infer_mime_from_path() {
        let cases = [
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.png", "image/png"),
            ("a.GIF", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.mp4", "video/mp4"),
            ("a.webm", "video/webm"),
            ("a.wav", "audio/wav"),
            ("a.mp3", "audio/mpeg"),
            ("a.json", "application/json"),
            ("a.bin", "application/octet-stream"),
            ("noext", "application/octet-stream"),
        ];
        for (path, want) in cases {
            assert_eq!(infer_mime_from_path(path), want, "path: {path}");
        }
    }

    #[tokio::test]
    async fn test_handle_avatar_missing_pubkey() {
        let err = protocol_handle_avatar("/").await.unwrap_err();
        assert_eq!(err, "Missing pubkey");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_handle_avatar_falls_back_to_identicon_on_invalid_picture_url() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("proto_avatar", "proto");
        let now = soshal_common_core::format::now_secs();
        let row = soshal_db_core::repos::user::UserRow {
            pubkey: "pk".to_string(),
            npub: "npk".to_string(),
            name: None,
            display_name: None,
            about: None,
            picture: Some("http://127.0.0.1/x".to_string()),
            banner: None,
            nip05: None,
            lud16: None,
            created_at: now,
            updated_at: now,
            metadata_json: None,
            contact_pubkeys: "[]".to_string(),
            relay_list: "[]".to_string(),
            follower_count: 0,
        };
        super::super::db::with_db_result(|db| {
            soshal_db_core::repos::user::UserRepo::new(db).upsert(&row)?;
            Ok(true)
        })
        .unwrap();

        // picture is a loopback URL -> fetch fails -> identicon fallback
        let out = protocol_handle_avatar("/pk").await.unwrap();
        assert_eq!(out, soshal_media_core::identicon::identicon_png("pk"));
    }
}
