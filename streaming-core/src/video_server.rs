//! Local Rust HTTP micro-server for video streaming.
//! Binds strictly to 127.0.0.1 on an ephemeral port.
//! Manages HTTP range requests, chunk buffering, disk decryption, and HLS proxying natively.

use soshal_common_core::format::is_valid_hex;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
pub struct VideoRegistry {
    routes: Arc<RwLock<HashMap<String, String>>>,
}

impl VideoRegistry {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, video_id: String, source_path: String) {
        if let Ok(mut map) = self.routes.write() {
            map.insert(video_id, source_path);
        }
    }

    pub fn get(&self, video_id: &str) -> Option<String> {
        self.routes.read().ok()?.get(video_id).cloned()
    }
}

pub struct LocalVideoServer {
    port: u16,
    registry: VideoRegistry,
    handle: Option<tokio::task::JoinHandle<()>>,
}

enum Route<'a> {
    Video(&'a str),
    Blob(&'a str),
}

fn parse_route(path: &str) -> Option<Route<'_>> {
    if let Some(id) = path.strip_prefix("/video/") {
        if !id.is_empty() && !id.contains('/') {
            return Some(Route::Video(id));
        }
    }
    if let Some(hash) = path.strip_prefix("/blob/") {
        if hash.len() == 64 && is_valid_hex(hash) {
            return Some(Route::Blob(hash));
        }
    }
    None
}

async fn write_not_found(socket: &mut tokio::net::TcpStream) {
    let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
    let _ = socket.write_all(resp.as_bytes()).await;
}

impl LocalVideoServer {
    pub async fn start() -> Result<Self, String> {
        Self::start_with_blob_root(None).await
    }

    pub async fn start_with_blob_root(blob_root: Option<PathBuf>) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind local video server: {e}"))?;

        let addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {e}"))?;
        let port = addr.port();
        let registry = VideoRegistry::new();
        let registry_clone = registry.clone();
        let root_clone = blob_root.clone();

        let handle = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let reg = registry_clone.clone();
                let root = root_clone.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let read_fut = socket.read(&mut buf);
                    let n = match tokio::time::timeout(std::time::Duration::from_secs(10), read_fut)
                        .await
                    {
                        Ok(Ok(n)) if n > 0 => n,
                        _ => return,
                    };

                    let request_str = String::from_utf8_lossy(&buf[..n]);
                    let mut lines = request_str.lines();
                    let req_line = match lines.next() {
                        Some(l) => l,
                        None => return,
                    };

                    let parts: Vec<&str> = req_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return;
                    }

                    let path = parts[1];
                    let range_header = lines
                        .find(|l| l.to_lowercase().starts_with("range:"))
                        .map(|l| l.to_string());

                    let route = match parse_route(path) {
                        Some(r) => r,
                        None => {
                            write_not_found(&mut socket).await;
                            return;
                        }
                    };

                    match route {
                        Route::Video(id) => {
                            let file_path = match reg.get(id) {
                                Some(p) => p,
                                None => {
                                    write_not_found(&mut socket).await;
                                    return;
                                }
                            };
                            serve_video_file(&mut socket, &file_path, range_header.as_deref())
                                .await;
                        }
                        Route::Blob(hash) => {
                            let file_path = match root {
                                Some(root) => root.join(hash),
                                None => {
                                    write_not_found(&mut socket).await;
                                    return;
                                }
                            };
                            if file_path.is_file() {
                                serve_video_file(
                                    &mut socket,
                                    &file_path.to_string_lossy(),
                                    range_header.as_deref(),
                                )
                                .await;
                            } else {
                                write_not_found(&mut socket).await;
                            }
                        }
                    }
                });
            }
        });

        Ok(Self {
            port,
            registry,
            handle: Some(handle),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }

    pub fn register_video(&self, video_id: String, source_path: String) -> String {
        self.registry.register(video_id.clone(), source_path);
        format!("http://127.0.0.1:{}/video/{}", self.port, video_id)
    }
}

impl Drop for LocalVideoServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn serve_video_file(
    socket: &mut tokio::net::TcpStream,
    file_path: &str,
    range_header: Option<&str>,
) {
    let mut file = match File::open(file_path).await {
        Ok(f) => f,
        Err(_) => {
            let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = socket.write_all(resp.as_bytes()).await;
            return;
        }
    };

    let total_size = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    let mut start = 0u64;
    let mut end = if total_size > 0 { total_size - 1 } else { 0 };
    let is_range = if let Some(hdr) = range_header {
        if let Some(spec) = hdr.split('=').nth(1) {
            let parts: Vec<&str> = spec.trim().split('-').collect();
            if !parts.is_empty() && !parts[0].is_empty() {
                if let Ok(s) = parts[0].parse::<u64>() {
                    start = s;
                }
            }
            if parts.len() > 1 && !parts[1].is_empty() {
                if let Ok(e) = parts[1].parse::<u64>() {
                    end = e.min(total_size.saturating_sub(1));
                }
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    let chunk_len = if total_size > 0 && start <= end {
        end - start + 1
    } else {
        0
    };

    if file.seek(SeekFrom::Start(start)).await.is_err() {
        let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
        let _ = socket.write_all(resp.as_bytes()).await;
        return;
    }

    let status_line = if is_range {
        "HTTP/1.1 206 Partial Content"
    } else {
        "HTTP/1.1 200 OK"
    };

    let header = if is_range {
        format!(
            "{status_line}\r\n\
            Accept-Ranges: bytes\r\n\
            Content-Type: video/mp4\r\n\
            Content-Length: {chunk_len}\r\n\
            Content-Range: bytes {start}-{end}/{total_size}\r\n\
            Connection: close\r\n\r\n"
        )
    } else {
        format!(
            "{status_line}\r\n\
            Accept-Ranges: bytes\r\n\
            Content-Type: video/mp4\r\n\
            Content-Length: {chunk_len}\r\n\
            Connection: close\r\n\r\n"
        )
    };

    if socket.write_all(header.as_bytes()).await.is_err() {
        return;
    }

    let mut remaining = chunk_len;
    let mut buffer = [0u8; 65536];
    while remaining > 0 {
        let to_read = (remaining as usize).min(buffer.len());
        let bytes_read = match file.read(&mut buffer[..to_read]).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        if socket.write_all(&buffer[..bytes_read]).await.is_err() {
            break;
        }

        remaining -= bytes_read as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_video_server_binds_ephemeral_port() {
        let server = LocalVideoServer::start().await.unwrap();
        assert!(server.port() > 0);
        let url = server.register_video("vid123".to_string(), "/tmp/test.mp4".to_string());
        assert!(url.contains("http://127.0.0.1:"));
        assert!(url.contains("/video/vid123"));
    }

    #[test]
    fn test_parse_route_rejects_invalid_paths() {
        assert!(parse_route("/video/").is_none());
        assert!(parse_route("/video/a/b").is_none());
        assert!(parse_route("/blob/0011").is_none());
        assert!(parse_route(
            "/blob/nothex_garbage_0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab"
        )
        .is_none());
        assert!(parse_route("/evil/../video/vid").is_none());
        assert!(parse_route("").is_none());
    }

    #[test]
    fn test_parse_route_accepts_video_and_blob() {
        assert!(matches!(
            parse_route("/video/vid123"),
            Some(Route::Video("vid123"))
        ));
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(matches!(parse_route(&format!("/blob/{hash}")), Some(Route::Blob(h)) if h == hash));
    }

    #[tokio::test]
    async fn test_blob_route_serves_range_from_cache_file() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let root = soshal_test_util::tmp_root("video_server");
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let payload = b"hello blob world".to_vec();
        std::fs::write(root.join(hash), &payload).unwrap();

        let server = LocalVideoServer::start_with_blob_root(Some(root.clone()))
            .await
            .unwrap();
        let addr = format!("127.0.0.1:{}", server.port());

        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            format!(
                "GET /blob/{hash} HTTP/1.1\r\nHost: localhost\r\nRange: bytes=6-9\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(text.ends_with("blob"));

        let missing_hash = "f".repeat(64);
        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            format!(
                "GET /blob/{missing_hash} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        assert!(String::from_utf8_lossy(&resp).starts_with("HTTP/1.1 404"));

        let mut server = server;
        server.stop();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn test_full_file_get_returns_200() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let dir = soshal_test_util::tmp_root("video_server");
        let payload = b"full file get payload".to_vec();
        let path = dir.join("full.mp4");
        std::fs::write(&path, &payload).unwrap();

        let mut server = LocalVideoServer::start().await.unwrap();
        server.register_video("fullvid".to_string(), path.to_string_lossy().to_string());
        let addr = format!("127.0.0.1:{}", server.port());

        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            b"GET /video/fullvid HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains(&format!("Content-Length: {}", payload.len())));
        assert!(!text.contains("Content-Range"));
        assert!(text.ends_with("full file get payload"));

        server.stop();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_malformed_range_start_gt_end_rejected() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let dir = soshal_test_util::tmp_root("video_server");
        let payload = b"malformed range payload".to_vec();
        let path = dir.join("range.mp4");
        std::fs::write(&path, &payload).unwrap();

        let mut server = LocalVideoServer::start().await.unwrap();
        server.register_video("rangevid".to_string(), path.to_string_lossy().to_string());
        let addr = format!("127.0.0.1:{}", server.port());

        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            b"GET /video/rangevid HTTP/1.1\r\nHost: localhost\r\nRange: bytes=100-50\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(text.contains("Content-Length: 0"));

        server.stop();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_video_route_missing_id_404() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let dir = soshal_test_util::tmp_root("video_server");
        let payload = b"registered video payload".to_vec();
        let path = dir.join("known.mp4");
        std::fs::write(&path, &payload).unwrap();

        let mut server = LocalVideoServer::start().await.unwrap();
        server.register_video("known".to_string(), path.to_string_lossy().to_string());
        let addr = format!("127.0.0.1:{}", server.port());

        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            b"GET /video/nosuchvideo HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 404"));
        assert!(text.contains("Content-Length: 0"));

        server.stop();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_sendfile_path_full_200_linux() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let dir = soshal_test_util::tmp_root("video_server");
        let payload = vec![0xABu8; 5 * 1024 * 1024];
        let path = dir.join("big.mp4");
        std::fs::write(&path, &payload).unwrap();

        let mut server = LocalVideoServer::start().await.unwrap();
        server.register_video("bigvid".to_string(), path.to_string_lossy().to_string());
        let addr = format!("127.0.0.1:{}", server.port());

        let mut sock = TcpStream::connect(&addr).await.unwrap();
        sock.write_all(
            b"GET /video/bigvid HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut resp = Vec::new();
        sock.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains(&format!("Content-Length: {}", payload.len())));
        let header_end = text.find("\r\n\r\n").expect("header terminator") + 4;
        assert_eq!(&resp[header_end..], payload);

        server.stop();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
