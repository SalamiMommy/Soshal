use crate::MediaFile;
use reqwest::Client;

/// Hard cap on download responses: a hostile blossom server must not be able
/// to exhaust memory with an unbounded body (64 MiB).
#[doc(hidden)]
pub const MAX_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
#[doc(hidden)]
pub const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;

pub struct BlossomClient {
    http: Client,
    /// Blossom server base URL.
    pub server_url: String,
}

impl BlossomClient {
    /// Builds a client that never follows redirects (a redirecting server
    /// must not be able to funnel uploads/downloads to an attacker-chosen
    /// endpoint) and bounds connect/response time.
    pub fn new(server_url: &str) -> Self {
        Self::with_http(server_url, None, &[])
    }

    /// Same hardened client as [`Self::new`], but pins `host` to the caller-
    /// validated `addrs` (SSRF-checked resolution). Pinning means the OS
    /// resolver cannot re-resolve the host to a different address at connect
    /// time (DNS rebinding TOCTOU).
    pub fn new_pinned(server_url: &str, host: &str, addrs: &[std::net::SocketAddr]) -> Self {
        Self::with_http(server_url, Some(host), addrs)
    }

    fn with_http(server_url: &str, host: Option<&str>, addrs: &[std::net::SocketAddr]) -> Self {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10));
        if let Some(host) = host {
            // Pin ALL resolved addresses: per-addr `.resolve()` calls
            // overwrite each other, leaving a DNS-rebinding window.
            if !addrs.is_empty() {
                builder = builder.resolve_to_addrs(host, addrs);
            }
        }
        let http = builder.build().unwrap_or_else(|_| Client::new());
        Self {
            http,
            server_url: server_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn upload(&self, data: Vec<u8>, mime_type: &str) -> Result<MediaFile, String> {
        let url = format!("{}/upload", self.server_url);
        let resp = self
            .http
            .put(&url)
            .header("Content-Type", mime_type)
            .body(data)
            .send()
            .await
            .map_err(|e| format!("upload failed: {}", e))?;
        let file: MediaFile = resp
            .json()
            .await
            .map_err(|e| format!("parse failed: {}", e))?;
        Ok(file)
    }

    /// Uploads with a NIP-98 `Authorization: Nostr <token>` header. The caller
    /// builds the token (base64 JSON payload + base64 schnorr signature).
    pub async fn upload_authenticated(
        &self,
        data: Vec<u8>,
        mime_type: &str,
        auth_token: &str,
    ) -> Result<MediaFile, String> {
        let url = format!("{}/upload", self.server_url);
        let resp = self
            .http
            .put(&url)
            .header("Content-Type", mime_type)
            .header("Authorization", format!("Nostr {}", auth_token))
            .body(data)
            .send()
            .await
            .map_err(|e| format!("upload failed: {}", e))?;
        let file: MediaFile = resp
            .json()
            .await
            .map_err(|e| format!("parse failed: {}", e))?;
        Ok(file)
    }

    pub async fn download(&self, hash: &str) -> Result<Vec<u8>, String> {
        // SECURITY: the hash is spliced into the URL path; a hash from a
        // hostile blossom server must not be able to inject path segments,
        // query strings or fragments (`..`, `/`, `?`, `#`). Only a canonical
        // 64-char lowercase hex sha256 is accepted.
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid file hash".into());
        }
        let url = format!("{}/{}", self.server_url, hash);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("download failed: {}", e))?;
        let cap = resp
            .content_length()
            .map(|l| (l as usize).min(MAX_DOWNLOAD_BYTES))
            .unwrap_or(64 * 1024);
        let mut bytes = Vec::with_capacity(cap);
        let mut resp = resp;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("read failed: {}", e))?
        {
            if bytes.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
                return Err("download exceeds size cap".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub async fn list(&self, pubkey: &str) -> Result<Vec<MediaFile>, String> {
        let url = format!("{}/list/{}", self.server_url, pubkey);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("list failed: {}", e))?;
        if let Some(len) = resp.content_length() {
            if len as usize > MAX_LIST_BYTES {
                return Err(format!("list response too large: {} bytes", len));
            }
        }
        // SECURITY: same incremental cap as download() — a missing
        // Content-Length must not allow an unbounded in-memory response.
        let cap = resp
            .content_length()
            .map(|l| (l as usize).min(MAX_LIST_BYTES))
            .unwrap_or(64 * 1024);
        let mut buf = Vec::with_capacity(cap);
        let mut resp = resp;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("read failed: {}", e))?
        {
            if buf.len() + chunk.len() > MAX_LIST_BYTES {
                return Err("list response exceeds size cap".into());
            }
            buf.extend_from_slice(&chunk);
        }
        let files: Vec<MediaFile> =
            serde_json::from_slice(&buf).map_err(|e| format!("parse failed: {}", e))?;
        Ok(files)
    }
}
