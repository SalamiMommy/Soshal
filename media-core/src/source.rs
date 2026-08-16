//! Upload source resolution: local file paths or remote URLs.
//!
//! Upload entry points (`media_upload`, `media_upload_blob_file`) accept a
//! source string that is either a local file path or an `http(s)://` URL.
//! URL fetches are SSRF-hardened exactly like NIP-05 verification: scheme +
//! hostname allow-list check, DNS resolution pinned with every address
//! verified non-private (DNS-rebinding TOCTOU), redirects disabled, and a
//! hard cap on the response body.

use soshal_common_core::url::{is_private_ip_str, is_valid_media_url};

/// Hard cap on URL-sourced upload payloads: a hostile endpoint must not be
/// able to exhaust memory with an unbounded body (matches Blossom download cap).
pub const MAX_SOURCE_URL_BYTES: usize = 64 * 1024 * 1024;

/// True when `source` looks like a remote URL (http/https scheme).
pub fn is_url_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Reads upload bytes from `source`: a local file path or a remote URL.
/// Returns `(bytes, mime_type)` — mime is `Some` only when a URL fetch
/// supplied a `Content-Type` header.
pub async fn fetch_source_bytes(source: &str) -> Result<(Vec<u8>, Option<String>), String> {
    if is_url_source(source) {
        fetch_url_bytes(source).await
    } else {
        let data =
            std::fs::read(source).map_err(|e| format!("Failed to read file {source}: {e}"))?;
        Ok((data, None))
    }
}

/// SSRF-hardened fetch of a remote source: validates scheme + hostname,
/// resolves and pins DNS, rejects private/loopback/link-local addresses,
/// disables redirects, and caps the body size.
async fn fetch_url_bytes(url: &str) -> Result<(Vec<u8>, Option<String>), String> {
    if !is_valid_media_url(url) {
        return Err("source URL not allowed (private or local host)".to_string());
    }
    let parsed = url::Url::parse(url).map_err(|e| format!("source URL parse error: {e}"))?;
    let host = parsed
        .host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| "source URL has no host".to_string())?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "source URL has no port".to_string())?;
    let mut pinned_addrs: Vec<std::net::SocketAddr> = Vec::new();
    match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(addrs) => {
            for addr in addrs {
                if is_private_ip_str(&addr.ip().to_string()) {
                    return Err("source URL resolves to an internal address".to_string());
                }
                pinned_addrs.push(addr);
            }
        }
        Err(_) => return Err("source URL does not resolve".to_string()),
    }
    if pinned_addrs.is_empty() {
        return Err("source URL does not resolve".to_string());
    }
    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    // Pin ALL resolved addresses (per-addr `.resolve()` overwrites each
    // other, leaving a DNS-rebinding window).
    client_builder = client_builder.resolve_to_addrs(&host, &pinned_addrs);
    let client = client_builder
        .build()
        .map_err(|e| format!("http client error: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("source fetch error: {e}"))?;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_SOURCE_URL_BYTES {
            return Err("source exceeds size cap".to_string());
        }
    }
    let mime_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
    // SECURITY: stream with a hard cap — a missing or lying Content-Length
    // must not allow an unbounded in-memory response from the host.
    let mut body = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("source read error: {e}"))?
    {
        if body.len() + chunk.len() > MAX_SOURCE_URL_BYTES {
            return Err("source exceeds size cap".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, mime_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_source_detection() {
        assert!(is_url_source("https://example.com/a.jpg"));
        assert!(is_url_source("http://example.com/a.jpg"));
        assert!(!is_url_source("/tmp/a.jpg"));
        assert!(!is_url_source("a.jpg"));
        assert!(!is_url_source("file:///tmp/a.jpg"));
    }

    #[tokio::test]
    async fn path_source_reads_local_file() {
        let dir = std::env::temp_dir().join(format!(
            "soshal-src-test-{}-{}",
            std::process::id(),
            soshal_common_core::format::now_secs()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob.bin");
        let bytes = vec![1u8, 2, 3, 4, 5];
        std::fs::write(&path, &bytes).unwrap();
        let (data, mime) = fetch_source_bytes(path.to_str().unwrap()).await.unwrap();
        assert_eq!(data, bytes);
        assert!(mime.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn path_source_missing_file_errors() {
        let err = fetch_source_bytes("/nonexistent/soshal/blob.bin")
            .await
            .unwrap_err();
        assert!(err.contains("Failed to read file"));
    }

    #[tokio::test]
    async fn url_source_rejects_loopback_host() {
        for url in [
            "http://127.0.0.1:8080/a.jpg",
            "http://localhost:8080/a.jpg",
            "http://[::1]:8080/a.jpg",
            "http://0x7f000001/a.jpg",
        ] {
            let err = fetch_source_bytes(url).await.unwrap_err();
            assert!(
                err.contains("not allowed") || err.contains("resolves to an internal address"),
                "{url}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn url_source_rejects_private_and_rebinding_hosts() {
        for url in [
            "http://192.168.1.10/a.jpg",
            "http://10.0.0.1/a.jpg",
            "http://172.16.0.1/a.jpg",
            "http://foo.localtest.me/a.jpg",
            "http://bar.nip.io/a.jpg",
        ] {
            let err = fetch_source_bytes(url).await.unwrap_err();
            assert!(err.contains("not allowed"), "{url}: {err}");
        }
    }

    #[tokio::test]
    async fn url_source_rejects_non_http_scheme() {
        let err = fetch_source_bytes("ftp://example.com/a.jpg")
            .await
            .unwrap_err();
        assert!(err.contains("Failed to read file"));
    }
}
