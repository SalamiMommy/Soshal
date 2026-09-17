//! Media FFI module
//!
//! Blossom uploads/downloads and local media caching. All network I/O is
//! Rust-side; Dart passes file paths and server URLs only.

use flutter_rust_bridge::frb;
use soshal_common_core::url::is_valid_media_url;
use soshal_media_core::cas::ChunkStore;
use std::fs;
use std::io::{Cursor, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Resolve a Dart-supplied file path to a canonical absolute path, but only
/// when it lives under an allowed root (the chunk store or the system temp
/// dir). The parent directory is canonicalized first, so `..`, symlinks and
/// other escapes are rejected. This keeps `media_fetch_blob`/`media_load_local`
/// from writing/reading arbitrary files when a compromised Dart layer (or a
/// hostile blob hash spliced into a path) reaches this boundary.
fn resolve_allowed_path(path: &str, what: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err(format!("{what} path cannot be empty"));
    }
    let p = Path::new(path);
    let parent = p
        .parent()
        .ok_or_else(|| format!("{what} path has no parent"))?;
    let canon_parent = fs::canonicalize(parent).map_err(|e| format!("{what} dir: {e}"))?;
    let allowed = [ChunkStore::default_root(), std::env::temp_dir()];
    if !allowed.iter().any(|root| canon_parent.starts_with(root)) {
        return Err(format!(
            "{what} path must be inside the media cache or temp dir"
        ));
    }
    let file_name = p
        .file_name()
        .ok_or_else(|| format!("{what} path has no file name"))?
        .to_string_lossy()
        .into_owned();
    Ok(canon_parent.join(file_name))
}

/// Open an allowed path read-only with O_NOFOLLOW and verify the opened
/// inode/device matches the resolved path, defeating TOCTOU symlink
/// substitution between path resolution and open.
#[cfg(unix)]
fn open_allowed_read(full: &Path, what: &str) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(full)
        .map_err(|e| format!("{what} open failed (symlink?): {e}"))?;
    let file_meta = file
        .metadata()
        .map_err(|e| format!("{what} metadata: {e}"))?;
    let path_meta = fs::metadata(full).map_err(|e| format!("{what} path metadata: {e}"))?;
    if file_meta.dev() != path_meta.dev() || file_meta.ino() != path_meta.ino() {
        return Err(format!("{what} path changed between resolution and open"));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_allowed_read(full: &Path, what: &str) -> Result<fs::File, String> {
    fs::File::open(full).map_err(|e| format!("{what} open failed: {e}"))
}

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
pub async fn media_decode_image_rgba(
    file_path_or_url: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> Result<DecodedImageRgbaDto, String> {
    tokio::task::spawn_blocking(move || {
        let full = resolve_allowed_path(&file_path_or_url, "image decode")?;
        let file = open_allowed_read(&full, "image decode")?;
        let cap = file.metadata().map(|m| m.len() as usize).unwrap_or(0);
        let mut bytes = Vec::with_capacity(cap);
        let mut file = file;
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("Failed to read image file: {e}"))?;

        let frame = soshal_media_core::decoder::decode_to_rgba(&bytes, max_width, max_height)?;
        Ok(DecodedImageRgbaDto {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels,
        })
    })
    .await
    .map_err(|e| format!("decode join: {e}"))?
}

/// Upload media to a Blossom server. `source` is a local file path or an
/// `http(s)://` URL (SSRF-guarded fetch).
#[frb(serialize)]
pub async fn media_upload(file_path: String, blossom_server: String) -> Result<String, String> {
    // Local file sources must pass the allowed-path guard before reading; URL
    // sources go through the SSRF-guarded async fetch in fetch_source_bytes.
    let source = if soshal_media_core::source::is_url_source(&file_path) {
        file_path.clone()
    } else {
        let path = resolve_allowed_path(&file_path, "media upload")?;
        path.to_string_lossy().into_owned()
    };
    let (data, fetched_mime) = match soshal_media_core::source::fetch_source_bytes(&source).await {
        Ok(v) => v,
        Err(e) => return Err(e).into(),
    };
    let mime_type = fetched_mime.unwrap_or_else(|| infer_mime_type(&source));
    let client =
        soshal_media_core::blossom::BlossomClient::new_pinned_resolve(&blossom_server).await?;
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
    // Validate cache_dir: canonicalize and ensure it stays inside allowed roots
    // (temp dir or the chunk-store app-data directory). This prevents a
    // compromised Dart caller from directing writes to arbitrary FS locations.
    if cache_dir.is_empty() {
        return Err("cache_dir cannot be empty".to_string()).into();
    }
    let canon_cache = tokio::task::spawn_blocking(move || {
        let _ = std::fs::create_dir_all(&cache_dir);
        let canon =
            std::fs::canonicalize(&cache_dir).map_err(|e| format!("cache_dir invalid: {e}"))?;
        let allowed_cache_roots = [ChunkStore::default_root(), std::env::temp_dir()];
        if !allowed_cache_roots
            .iter()
            .any(|root| canon.starts_with(root))
        {
            return Err(
                "cache_dir must be inside the media cache or system temp directory".to_string(),
            );
        }
        Ok::<_, String>(canon)
    })
    .await
    .map_err(|e| format!("spawn_blocking join: {e}"))??;
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
    let client = soshal_media_core::blossom::BlossomClient::new_pinned_resolve(&server).await?;
    match client.download(&hash).await {
        Ok(data) => {
            let filename = generate_cache_filename(&url);
            let cache_path = canon_cache.join(&filename);
            tokio::task::spawn_blocking(move || {
                fs::write(&cache_path, &data).map_err(|e| format!("Cache write failed: {e}"))?;
                Ok::<_, String>(cache_path.to_string_lossy().to_string())
            })
            .await
            .map_err(|e| format!("spawn_blocking join: {e}"))?
            .into()
        }
        Err(e) => Err(format!("Fetch failed: {e}")).into(),
    }
}

/// Load media from cache or disk (path must resolve inside the media cache
/// or temp dir; arbitrary file reads are rejected).
#[frb(serialize)]
pub async fn media_load_local(file_path: String) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || {
        const MAX_LOCAL_MEDIA_BYTES: u64 = 200 * 1024 * 1024; // 200 MiB
        let full = resolve_allowed_path(&file_path, "media")?;
        let file = open_allowed_read(&full, "media")?;
        let cap = match file.metadata() {
            Ok(meta) => {
                if meta.len() > MAX_LOCAL_MEDIA_BYTES {
                    return Err(format!(
                        "media file too large ({} bytes; max {} bytes)",
                        meta.len(),
                        MAX_LOCAL_MEDIA_BYTES
                    ));
                }
                meta.len() as usize
            }
            Err(_) => 0,
        };
        let mut data = Vec::with_capacity(cap);
        let file = file;
        // Use `take` as a second-line defence in case metadata lied or the
        // file grew between the metadata check and the read (sparse files,
        // special filesystems, etc.).
        use std::io::Read;
        file.take(MAX_LOCAL_MEDIA_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("Failed to read media: {e}"))?;
        if data.len() as u64 > MAX_LOCAL_MEDIA_BYTES {
            return Err(format!(
                "media file exceeds {}-byte limit",
                MAX_LOCAL_MEDIA_BYTES
            ));
        }
        Ok(data)
    })
    .await
    .map_err(|e| format!("load join: {e}"))?
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

/// Generate a stable, collision-resistant cache filename from URL.
/// Uses the first 16 hex chars of SHA-256(url) instead of DefaultHasher, which
/// is non-cryptographic and deterministic — two URLs with the same hash would
/// silently overwrite each other's cache entry (cache poisoning).
fn generate_cache_filename(url: &str) -> String {
    let digest = soshal_crypto_core::hash::sha256(url.as_bytes());
    hex::encode(&digest[..8]) // 16 hex chars = 64 bits of SHA-256
}

/// Clear media cache directory
#[frb(serialize)]
pub fn media_clear_cache(cache_dir: String) -> Result<String, String> {
    if cache_dir.is_empty() {
        return Err("cache_dir cannot be empty".to_string());
    }
    let p = std::path::Path::new(&cache_dir);
    if !p.exists() {
        return Err("Cache directory does not exist".to_string());
    }
    let canon = std::fs::canonicalize(p).map_err(|e| format!("canonicalize: {e}"))?;
    let chunk_root = ChunkStore::default_root();
    let temp_root = std::env::temp_dir();
    let canon_temp = std::fs::canonicalize(&temp_root).unwrap_or_else(|_| temp_root.clone());
    if canon == canon_temp {
        return Err("cannot clear system temp directory itself".to_string()).into();
    }
    let canon_chunk = std::fs::canonicalize(&chunk_root).unwrap_or_else(|_| chunk_root.clone());
    let is_chunk_root = canon == canon_chunk;

    // M6 fix: always-available safe roots (chunk store + system temp).
    // These can be checked even before the DB is initialized.
    let always_safe = [&canon_chunk, &canon_temp];
    let in_safe_root = always_safe.iter().any(|root| canon.starts_with(root));
    if !in_safe_root {
        // Not inside a guaranteed-safe root: require the DB to be initialized
        // so we can verify against the app directory.
        match super::db::db_path() {
            Ok(db_p) if !db_p.is_empty() && !db_p.starts_with(':') => {
                if let Some(parent) = std::path::Path::new(&db_p).parent() {
                    if !parent.as_os_str().is_empty() {
                        if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                            if canon == canon_parent {
                                return Err("cannot clear application root directory".to_string())
                                    .into();
                            }
                            if !canon.starts_with(&canon_parent) {
                                return Err(
                                    "cache_dir must be inside application directory or media cache"
                                        .to_string(),
                                )
                                .into();
                            }
                            if let Ok(canon_db) = std::fs::canonicalize(&db_p) {
                                if canon_db.starts_with(&canon) {
                                    return Err("cannot clear directory containing the database"
                                        .to_string())
                                    .into();
                                }
                            }
                        }
                    }
                }
            }
            Ok(_) => {
                // DB path is empty (in-memory): reject — no application dir to check against.
                return Err(
                    "cache_dir must be inside the media cache or system temp directory".to_string(),
                )
                .into();
            }
            Err(_) => {
                // DB not initialized: reject — no application dir to check against.
                return Err(
                    "cache_dir must be inside the media cache or system temp directory \
                     (database not initialized)"
                        .to_string(),
                )
                .into();
            }
        }
    }
    match fs::remove_dir_all(&canon) {
        Ok(_) => {
            if is_chunk_root {
                let _ = fs::create_dir_all(&canon);
            }
            Ok("Cache cleared".to_string()).into()
        }
        Err(e) => Err(format!("Clear failed: {e}")).into(),
    }
}

/// Reject sensitive/host-key paths when uploading a local media file to the
/// chunk store. A compromised Dart caller must not be able to read SSH keys,
/// cloud credentials, GPG keys, etc. into a blob that a hostile peer could
/// then exfiltrate over P2P.
///
/// Two-layer guard:
///   Layer 1 — OS-level sensitive directories (/etc, /proc, /sys, …).
///   Layer 2 — sensitive subdirectories under the user's home dir.
///
/// Paths under standard removable/data mount roots (/run/media, /media,
/// /mnt) bypass the blocklist: udisks automounts external hard drives at
/// /run/media/<user>, which the blanket /run block previously rejected as a
/// "system directory" even though it holds ordinary user media.
fn validate_local_source_path(canon: &Path) -> Result<(), String> {
    // Layer 0: user media on removable/data mounts is always allowed.
    const REMOVABLE_ROOTS: &[&str] = &["/run/media", "/media", "/mnt"];
    if REMOVABLE_ROOTS.iter().any(|root| canon.starts_with(root)) {
        return Ok(());
    }

    // Layer 1: block known sensitive OS directories. /run stays listed here:
    // the removable carve-out above is the only /run subtree that may pass.
    const BLOCKED_SYSTEM_DIRS: &[&str] = &[
        "/etc", "/proc", "/sys", "/root", "/boot", "/dev", "/run", "/snap", "/var/lib",
    ];
    if BLOCKED_SYSTEM_DIRS
        .iter()
        .any(|prefix| canon.starts_with(prefix))
    {
        return Err(
            "file path must not be inside a system directory (/etc, /proc, /sys, …)".to_string(),
        );
    }

    // Layer 2: block sensitive subdirectories under the user's home dir.
    // These hold secret key material that must never enter the chunk store.
    const BLOCKED_HOME_SUBDIRS: &[&str] = &[
        ".ssh",
        ".gnupg",
        ".aws",
        ".config",
        ".local/share/keyrings",
        ".password-store",
        ".netrc",
        ".credentials",
    ];
    if let Some(home) = dirs::home_dir() {
        for sub in BLOCKED_HOME_SUBDIRS {
            let blocked = home.join(sub);
            if canon.starts_with(&blocked) {
                return Err(format!(
                    "file path must not be inside a sensitive directory (~/{sub})"
                ));
            }
        }
    }
    Ok(())
}

/// Upload media to the local chunk store and return the blob manifest.
/// The blob is chunked, deduplicated, and stored in the local CAS.
#[frb(serialize)]
pub async fn media_upload_blob(data: Vec<u8>) -> Result<String, String> {
    let manifest = tokio::task::spawn_blocking(move || {
        let store = ChunkStore::new(ChunkStore::default_root());
        let manifest = store.store_reader(Cursor::new(data))?;
        store.save_manifest(&manifest)?;
        Ok::<_, String>(manifest)
    })
    .await
    .map_err(|e| format!("upload blob join: {e}"))??;
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
        tokio::task::spawn_blocking(move || {
            let store = ChunkStore::new(ChunkStore::default_root());
            let manifest = store.store_reader(Cursor::new(data))?;
            store.save_manifest(&manifest)?;
            Ok::<_, String>(manifest)
        })
        .await
        .map_err(|e| format!("spawn_blocking join: {e}"))??
    } else {
        // Local file path: canonicalize and reject sensitive directories to
        // prevent a compromised Dart caller from reading SSH keys, cloud
        // credentials, GPG keys, etc. into the chunk store (from where they
        // could be exfiltrated via P2P blob requests from a hostile peer).
        tokio::task::spawn_blocking(move || {
            let canon =
                fs::canonicalize(&file_path).map_err(|e| format!("file path invalid: {e}"))?;
            validate_local_source_path(&canon)?;
            // O_NOFOLLOW prevents symlink substitution between canonicalize and open.
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&canon)
                .map_err(|e| format!("open file failed (symlink?): {e}"))?;
            // Verify the opened file's inode/device matches the canonical path to
            // catch a TOCTOU race where a symlink was swapped in between.
            let file_meta = file
                .metadata()
                .map_err(|e| format!("open file metadata failed: {e}"))?;
            let canonical_meta =
                fs::metadata(&canon).map_err(|e| format!("canonical metadata failed: {e}"))?;
            if file_meta.dev() != canonical_meta.dev() || file_meta.ino() != canonical_meta.ino() {
                return Err(
                    "path changed between validation and open (possible symlink race)".to_string(),
                );
            }
            let store = ChunkStore::new(ChunkStore::default_root());
            let manifest = store.store_reader(file)?;
            store.save_manifest(&manifest)?;
            Ok::<_, String>(manifest)
        })
        .await
        .map_err(|e| format!("spawn_blocking join: {e}"))??
    };
    serde_json::to_string(&manifest)
        .map_err(|e| format!("manifest serde: {e}"))
        .into()
}

/// Fetch a blob by hash from the chunk store (local or swarm).
/// Returns the manifest JSON with success status.
#[frb(serialize)]
pub async fn media_fetch_blob(blob_hash: String, out_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let store = ChunkStore::new(ChunkStore::default_root());
        let manifest = match store.load_manifest(&blob_hash) {
            Some(m) => m,
            None => return Err("manifest not found".to_string()),
        };

        // Reconstruct the blob from chunks
        let data = soshal_media_core::cas::manifest_bytes(&store, &manifest);
        let full = resolve_allowed_path(&out_path, "blob output")?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&full)
            .map_err(|e| format!("blob output open failed (symlink?): {e}"))?;
        file.write_all(&data)
            .map_err(|e| format!("write failed: {e}"))?;

        serde_json::to_string(&serde_json::json!({
            "success": true,
            "blob_hash": blob_hash,
            "size": manifest.total_size,
        }))
        .map_err(|e| format!("serde: {e}"))
    })
    .await
    .map_err(|e| format!("fetch blob join: {e}"))?
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
        let err = soshal_db_core::block_on(media_fetch("https://example.com".to_string(), cache))
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
        let mut tampered = m;
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

    #[test]
    fn validate_local_source_path_allows_removable_mounts() {
        for p in [
            "/run/media/sam/External HD/podcast.mp3",
            "/run/media/sam/.hidden/track.flac",
            "/media/sam/USB/rip.wav",
            "/mnt/data/music/album.ogg",
        ] {
            assert!(
                validate_local_source_path(Path::new(p)).is_ok(),
                "expected allow: {p}"
            );
        }
    }

    #[test]
    fn validate_local_source_path_blocks_system_dirs() {
        for p in [
            "/etc/shadow",
            "/etc/ssh/ssh_host_ed25519_key",
            "/proc/1/environ",
            "/sys/kernel/kexec_crash_loaded",
            "/root/.ssh/id_ed25519",
            "/boot/initramfs.img",
            "/dev/mem",
            "/run/secrets/kube-token",
            "/run/user/1000/keyring/secret",
            "/snap/canonical-test/current/lib",
            "/var/lib/gpg/private-keys-v1.d/key.gpg",
        ] {
            let err = validate_local_source_path(Path::new(p)).unwrap_err();
            assert!(
                err.contains("system directory"),
                "{p}: expected system-dir reject, got: {err}"
            );
        }
    }

    #[test]
    fn validate_local_source_path_blocks_home_secrets() {
        let cases = [
            ".ssh/id_ed25519",
            ".gnupg/private-keys-v1.d/key.gpg",
            ".aws/credentials",
            ".config/foo/token",
            ".local/share/keyrings/login.keyring",
            ".password-store/gpg/github.gpg",
            ".netrc",
            ".credentials",
        ];
        for rel in cases {
            let blocked = dirs::home_dir().unwrap().join(rel);
            let err = validate_local_source_path(&blocked).unwrap_err();
            assert!(
                err.contains("sensitive directory"),
                "{rel}: expected home-secrets reject, got: {err}"
            );
        }
    }

    #[test]
    fn validate_local_source_path_allows_ordinary_user_paths() {
        let home = dirs::home_dir().unwrap();
        for p in [
            home.join("Music").join("track.mp3"),
            home.join("Desktop").join("notes.txt"),
            PathBuf::from("/tmp/upload_pending.mp3"),
            PathBuf::from("/home/other/user/media/x.wav"),
            PathBuf::from("/data/shared/audio/loop.ogg"),
        ] {
            assert!(
                validate_local_source_path(&p).is_ok(),
                "expected allow: {}",
                p.display()
            );
        }
    }
}
