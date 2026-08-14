//! Media FFI module
//!
//! Blossom uploads/downloads and local media caching. All network I/O is
//! Rust-side; Dart passes file paths and server URLs only.

use flutter_rust_bridge::frb;
use soshal_common_core::url::is_valid_media_url;
use soshal_media_core::cas::ChunkStore;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

lazy_static::lazy_static! {
    static ref MEDIA_SERVER: std::sync::Mutex<Option<soshal_streaming_core::video_server::LocalVideoServer>> =
        std::sync::Mutex::new(None);
}

/// Result type for media operations
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct MediaResult {
    pub url: String,
    pub mime_type: String,
    pub size: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct DecodedImageRgbaDto {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decode raw image bytes/file path into uncompressed 32-bit RGBA pixels on a background worker thread.
#[frb(serialize)]
pub fn media_decode_image_rgba(
    file_path_or_url: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> Result<DecodedImageRgbaDto, String> {
    let bytes =
        fs::read(&file_path_or_url).map_err(|e| format!("Failed to read image file: {e}"))?;

    let frame = soshal_media_core::decoder::decode_to_rgba(&bytes, max_width, max_height)?;
    Ok(DecodedImageRgbaDto {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels,
    })
}

/// Upload media to a Blossom server (reads the local file first).
#[frb(serialize)]
pub async fn media_upload(file_path: String, blossom_server: String) -> Result<String, String> {
    match fs::read(&file_path) {
        Ok(data) => {
            let mime_type = infer_mime_type(&file_path);
            let client = soshal_media_core::blossom::BlossomClient::new(&blossom_server);
            match client.upload(data, &mime_type).await {
                Ok(file) => super::util::json_ok(MediaResult {
                    url: file.url,
                    mime_type,
                    size: file.size,
                }),
                Err(e) => Err(format!("Upload failed: {e}")).into(),
            }
        }
        Err(e) => Err(format!("Failed to read file: {e}")).into(),
    }
}

/// Fetch media from URL and cache locally, returning the cache path.
#[frb(serialize)]
pub async fn media_fetch(url: String, cache_dir: String) -> Result<String, String> {
    if !is_valid_media_url(&url) {
        return Err("Invalid media URL".to_string()).into();
    }
    let (server, hash) = match url.split_once('/') {
        _ => {
            let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
            let rest = &url[scheme_end..];
            match rest.find('/') {
                Some(i) => (url[..scheme_end + i].to_string(), rest[i + 1..].to_string()),
                None => return Err("media URL must point to a blob path".to_string()).into(),
            }
        }
    };
    let client = soshal_media_core::blossom::BlossomClient::new(&server);
    match client.download(&hash).await {
        Ok(data) => {
            let filename = generate_cache_filename(&url);
            let cache_path = PathBuf::from(&cache_dir).join(&filename);
            match fs::write(&cache_path, &data) {
                Ok(_) => Ok(cache_path.to_string_lossy().to_string()).into(),
                Err(e) => Err(format!("Cache write failed: {e}")).into(),
            }
        }
        Err(e) => Err(format!("Fetch failed: {e}")).into(),
    }
}

/// Load media from cache or disk
#[frb(serialize)]
pub fn media_load_local(file_path: String) -> Result<Vec<u8>, String> {
    match fs::read(&file_path) {
        Ok(data) => Ok(data).into(),
        Err(e) => Err(format!("Failed to read media: {e}")).into(),
    }
}

/// Get MIME type from file path
#[frb(serialize)]
pub fn media_get_mime_type(file_path: String) -> Result<String, String> {
    Ok(infer_mime_type(&file_path)).into()
}

/// Infer MIME type from file extension
fn infer_mime_type(path: &str) -> String {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Generate a unique cache filename from URL
fn generate_cache_filename(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Clear media cache directory
#[frb(serialize)]
pub fn media_clear_cache(cache_dir: String) -> Result<String, String> {
    match fs::remove_dir_all(&cache_dir) {
        Ok(_) => Ok("Cache cleared".to_string()).into(),
        Err(e) => Err(format!("Clear failed: {e}")).into(),
    }
}

/// Upload media to the local chunk store and return the blob manifest.
/// The blob is chunked, deduplicated, and stored in the local CAS.
#[frb(sync, serialize)]
pub fn media_upload_blob(data: Vec<u8>) -> Result<String, String> {
    let store = ChunkStore::new(ChunkStore::default_root());
    let manifest = store.store_reader(Cursor::new(data))?;
    store.save_manifest(&manifest)?;
    serde_json::to_string(&manifest)
        .map_err(|e| format!("manifest serde: {e}"))
        .into()
}

/// Upload a local media file directly to the chunk store by path (zero Dart heap memory overhead).
#[frb(sync, serialize)]
pub fn media_upload_blob_file(file_path: String) -> Result<String, String> {
    let file = fs::File::open(&file_path).map_err(|e| format!("open file failed: {e}"))?;
    let store = ChunkStore::new(ChunkStore::default_root());
    let manifest = store.store_reader(file)?;
    store.save_manifest(&manifest)?;
    serde_json::to_string(&manifest)
        .map_err(|e| format!("manifest serde: {e}"))
        .into()
}

/// Fetch a blob by hash from the chunk store (local or swarm).
/// Returns the manifest JSON with success status.
#[frb(sync, serialize)]
pub fn media_fetch_blob(blob_hash: String, out_path: String) -> Result<String, String> {
    let store = ChunkStore::new(ChunkStore::default_root());
    let manifest = match store.load_manifest(&blob_hash) {
        Some(m) => m,
        None => return Err("manifest not found".to_string()).into(),
    };

    // Reconstruct the blob from chunks
    let data = soshal_media_core::cas::manifest_bytes(&store, &manifest);
    fs::write(&out_path, data).map_err(|e| format!("write failed: {e}"))?;

    serde_json::to_string(&serde_json::json!({
        "success": true,
        "blob_hash": blob_hash,
        "size": manifest.total_size,
    }))
    .map_err(|e| format!("serde: {e}"))
    .into()
}

/// Get the cache path for the chunk store.
#[frb(sync, serialize)]
pub fn media_get_cache_path() -> Result<String, String> {
    Ok(ChunkStore::default_root().to_string_lossy().to_string()).into()
}

/// Start a local HTTP range server for media playback (sendfile zero-copy).
/// Binds 127.0.0.1 on an ephemeral port; /blob/<hash> serves blob files out
/// of the chunk-store cache directory.
#[frb(sync, serialize)]
pub fn media_start_local_server() -> Result<u64, String> {
    let mut guard = MEDIA_SERVER
        .lock()
        .map_err(|_| "media server lock poisoned".to_string())?;
    if let Some(ref s) = *guard {
        return Ok(s.port() as u64).into();
    }
    let blob_root = ChunkStore::default_root().to_path_buf();
    let server = soshal_db_core::block_on(
        soshal_streaming_core::video_server::LocalVideoServer::start_with_blob_root(Some(
            blob_root,
        )),
    )?;
    let port = server.port() as u64;
    *guard = Some(server);
    Ok(port).into()
}

/// Stop the local HTTP range server.
#[frb(sync, serialize)]
pub fn media_stop_local_server() -> Result<bool, String> {
    let mut guard = MEDIA_SERVER
        .lock()
        .map_err(|_| "media server lock poisoned".to_string())?;
    if let Some(mut s) = guard.take() {
        s.stop();
    }
    Ok(true).into()
}
