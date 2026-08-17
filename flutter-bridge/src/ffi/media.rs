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

/// Upload media to a Blossom server. `source` is a local file path or an
/// `http(s)://` URL (SSRF-guarded fetch).
#[frb(serialize)]
pub async fn media_upload(file_path: String, blossom_server: String) -> Result<String, String> {
    let (data, fetched_mime) = match soshal_media_core::source::fetch_source_bytes(&file_path).await
    {
        Ok(v) => v,
        Err(e) => return Err(e).into(),
    };
    let mime_type = fetched_mime.unwrap_or_else(|| infer_mime_type(&file_path));
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

/// Infer MIME type from file extension (delegates to media-core)
fn infer_mime_type(path: &str) -> String {
    soshal_media_core::media::guess_mime_type(path).to_string()
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

/// Upload a local media file (or remote URL, SSRF-guarded) directly to the
/// chunk store (zero Dart heap memory overhead for the file path case).
#[frb(serialize)]
pub async fn media_upload_blob_file(file_path: String) -> Result<String, String> {
    let manifest = if soshal_media_core::source::is_url_source(&file_path) {
        // URL sources require an async fetch.
        let (data, _) = soshal_media_core::source::fetch_source_bytes(&file_path).await?;
        let store = ChunkStore::new(ChunkStore::default_root());
        let manifest = store.store_reader(Cursor::new(data))?;
        store.save_manifest(&manifest)?;
        manifest
    } else {
        let file = fs::File::open(&file_path).map_err(|e| format!("open file failed: {e}"))?;
        let store = ChunkStore::new(ChunkStore::default_root());
        let manifest = store.store_reader(file)?;
        store.save_manifest(&manifest)?;
        manifest
    };
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

/// Content-aware chunking window for a MIME type (JSON: min/avg/max).
#[frb(sync, serialize)]
pub fn media_chunking_for_mime(mime: String) -> Result<String, String> {
    let params = soshal_media_core::chunking::ChunkingParams::for_mime(&mime);
    super::util::json_ok(params)
}

/// Feed scroll telemetry into the global prefetcher (velocity px/s + visible indices).
#[frb(sync, serialize)]
pub fn media_update_scroll_telemetry(
    velocity: f32,
    top_index: u32,
    bottom_index: u32,
) -> Result<bool, String> {
    let prefetcher = soshal_media_core::prefetcher::global_prefetcher();
    prefetcher.update_scroll_telemetry(velocity, top_index, bottom_index);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_media_core::chunking::{chunk_bytes, ChunkManifest};

    #[test]
    fn infer_mime_type_covers_extensions() {
        assert_eq!(infer_mime_type("photo.PNG"), "image/png");
        assert_eq!(infer_mime_type("a.jpeg"), "image/jpeg");
        assert_eq!(infer_mime_type("b.gif"), "image/gif");
        assert_eq!(infer_mime_type("c.webp"), "image/webp");
        assert_eq!(infer_mime_type("d.mp4"), "video/mp4");
        assert_eq!(infer_mime_type("e.webm"), "video/webm");
        assert_eq!(infer_mime_type("f.mov"), "video/quicktime");
        assert_eq!(infer_mime_type("g.wav"), "audio/wav");
        assert_eq!(infer_mime_type("h.mp3"), "audio/mpeg");
        assert_eq!(infer_mime_type("i.m4a"), "application/octet-stream");
        assert_eq!(infer_mime_type("j.xyz"), "application/octet-stream");
        assert_eq!(infer_mime_type("noext"), "application/octet-stream");
        assert_eq!(infer_mime_type(".png"), "image/png");
        assert_eq!(infer_mime_type("a.b.png"), "image/png");
        assert_eq!(infer_mime_type("dir.with.dots/file.JPG"), "image/jpeg");
    }

    #[test]
    fn cache_filename_deterministic_and_unique() {
        let a = generate_cache_filename("https://example.com/blob/abc");
        let b = generate_cache_filename("https://example.com/blob/abc");
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!a.is_empty());
        let c = generate_cache_filename("https://example.com/blob/abd");
        assert_ne!(a, c);
        assert_ne!(
            generate_cache_filename("https://example.com"),
            generate_cache_filename("http://example.com")
        );
    }

    #[test]
    fn chunking_size_caps_per_mime() {
        let video = serde_json::from_str::<serde_json::Value>(
            &media_chunking_for_mime("video/mp4".to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(video["min"], 1024 * 1024);
        assert_eq!(video["avg"], 4 * 1024 * 1024);
        assert_eq!(video["max"], 16 * 1024 * 1024);
        let audio = serde_json::from_str::<serde_json::Value>(
            &media_chunking_for_mime("audio/wav".to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(audio["min"], 32 * 1024);
        assert_eq!(audio["avg"], 128 * 1024);
        assert_eq!(audio["max"], 512 * 1024);
        let other = serde_json::from_str::<serde_json::Value>(
            &media_chunking_for_mime("application/octet-stream".to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(other["min"], 64 * 1024);
        assert_eq!(other["avg"], 256 * 1024);
        assert_eq!(other["max"], 1024 * 1024);
    }

    #[test]
    fn ssrf_guard_rejects_private_loopback_rebinding() {
        for url in [
            "http://localhost:8080/a.jpg",
            "http://127.0.0.1/a.jpg",
            "http://127.1/a.jpg",
            "http://127.0.1.9/a.jpg",
            "http://192.168.1.10/a.jpg",
            "http://10.0.0.5/a.jpg",
            "http://172.16.0.1/a.jpg",
            "http://172.31.255.1/a.jpg",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/a.jpg",
            "http://0/a.jpg",
            "http://0x7f.1.1.1/a.jpg",
            "http://2130706433/a.jpg",
            "http://[::1]/a.jpg",
            "http://[fc00::1]/a.jpg",
            "http://[fe80::1]/a.jpg",
            "http://foo.nip.io/a.jpg",
            "http://foo.xip.io/a.jpg",
            "http://foo.sslip.io/a.jpg",
            "http://foo.localtest.me/a.jpg",
            "http://foo.loca.lt/a.jpg",
            "ftp://example.com/a.jpg",
            "javascript:alert(1)",
            "not-a-url",
            "",
        ] {
            assert!(!is_valid_media_url(url), "expected reject: {url}");
        }
    }

    #[test]
    fn ssrf_guard_accepts_public_hosts() {
        for url in [
            "https://example.com/blob/abc",
            "http://example.com:8080/a.jpg",
            "https://sub.example.org/path?q=1",
            "https://blossom.example.net/",
        ] {
            assert!(is_valid_media_url(url), "expected accept: {url}");
        }
        let too_long = format!("https://example.com/{}", "a".repeat(2100));
        assert!(!is_valid_media_url(&too_long));
    }

    #[test]
    fn media_fetch_rejects_bad_url_before_network() {
        let cache = std::env::temp_dir()
            .join(format!("soshal_media_inline_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let err = soshal_db_core::block_on(media_fetch(
            "http://127.0.0.1/a.jpg".to_string(),
            cache.clone(),
        ))
        .unwrap_err();
        assert!(err.contains("Invalid media URL"), "{err}");
        let err = soshal_db_core::block_on(media_fetch(
            "https://example.com".to_string(),
            cache.clone(),
        ))
        .unwrap_err();
        assert!(err.contains("must point to a blob path"), "{err}");
    }

    #[test]
    fn blob_manifest_hash_helpers() {
        let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
        let m = chunk_bytes(&data).unwrap();
        assert_eq!(m.blob_hash.len(), 64);
        assert!(m.blob_hash.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(m.total_size, data.len() as u64);
        assert!(m.is_valid());
        let same = chunk_bytes(&data).unwrap();
        assert_eq!(same.blob_hash, m.blob_hash);
        let mut tampered = m.clone();
        tampered.chunks[0].len += 1;
        assert!(!tampered.is_valid());
        let empty = chunk_bytes(&[]).unwrap();
        assert_eq!(empty.total_size, 0);
        assert!(empty.is_valid());
        let mut oversized = ChunkManifest {
            blob_hash: "0".repeat(64),
            total_size: 10,
            chunks: vec![soshal_media_core::chunking::ChunkRef {
                blake3: "0".repeat(64),
                offset: 0,
                len: 11,
            }],
        };
        assert!(!oversized.is_valid());
        oversized.chunks[0].len = 10;
        assert!(oversized.is_valid());
    }
}
