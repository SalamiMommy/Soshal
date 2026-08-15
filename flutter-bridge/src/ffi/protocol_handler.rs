//! Protocol handler for app:// scheme
//!
//! Enables Flutter WebView to access media and other resources via custom URI scheme.
//! Examples:
//!   - app://media/post-123.jpg (fetch and stream image)
//!   - `app://avatar/<pubkey>` (fetch user avatar)
//!   - app://relay/status (get relay pool status)

use flutter_rust_bridge::frb;
use soshal_common_core::url::is_valid_media_url;
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
    // If it looks like a hash, resolve to full URL first
    let url = if url_or_hash.contains("://") {
        url_or_hash.to_string()
    } else {
        // Could be a hash - look up in metadata
        format!("https://blossom.example.com/blob/{}", url_or_hash)
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

    let client = soshal_media_core::blossom::BlossomClient::new(&server);
    match client.download(&hash).await {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Fetch failed: {}", e)),
    }
}

/// Load media from local cache directory
fn protocol_load_from_cache(filename: &str) -> Result<Vec<u8>, String> {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("soshal_flutter_cache");

    let file_path = cache_dir.join(filename);

    // Prevent directory traversal
    if !file_path.starts_with(&cache_dir) {
        return Err("Path traversal detected".to_string());
    }

    match fs::read(&file_path) {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Cache read failed: {}", e)),
    }
}

/// Get metadata for media resource
fn protocol_metadata_media(path: &str) -> Result<ProtocolResponse, String> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if parts.len() < 2 {
        return Err("Invalid media path".to_string());
    }

    match parts[0] {
        "local" => {
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("soshal_flutter_cache");

            let file_path = cache_dir.join(parts[1]);

            match fs::metadata(&file_path) {
                Ok(meta) => {
                    let mime_type = infer_mime_from_path(parts[1]);
                    Ok(ProtocolResponse {
                        content_type: mime_type,
                        content_length: meta.len(),
                        cache_control: "public, max-age=86400".to_string(),
                        etag: format!("{:x}", meta.len()), // Simple ETag
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

    // Fallback: minimal 1x1 transparent PNG.
    Ok(PLACEHOLDER_PNG.to_vec())
}

const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// Fetch avatar bytes over HTTPS with a 5 MiB cap. `is_valid_media_url`
/// rejects private/loopback hosts (SSRF guard).
async fn fetch_avatar_bytes(url: &str) -> Result<Vec<u8>, String> {
    if !is_valid_media_url(url) {
        return Err("Invalid avatar URL".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("avatar fetch: {e}"))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("avatar read: {e}"))?;
    if bytes.len() > 5 * 1024 * 1024 {
        return Err("avatar too large".to_string());
    }
    Ok(bytes.to_vec())
}

/// Get metadata for avatar
fn protocol_metadata_avatar(_path: &str) -> Result<ProtocolResponse, String> {
    Ok(ProtocolResponse {
        content_type: "image/png".to_string(),
        content_length: PLACEHOLDER_PNG.len() as u64,
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
    if !file_path.starts_with(&cache_dir) {
        return Err("Path traversal detected".to_string());
    }

    protocol_load_from_cache(path)
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
