use crate::MediaFile;
use reqwest::Client;
use soshal_common_core::url::is_valid_media_url;

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
    /// Same hardened client as [`Self::new`], but resolves the server
    /// hostname at call time, verifies EVERY resolved address is public
    /// (DNS-rebinding TOCTOU guard), and pins the connection to those
    /// addresses so the OS resolver cannot re-resolve to a private address
    /// between validation and connect. Preferred for production call sites.
    pub async fn new_pinned_resolve(server_url: &str) -> Result<Self, String> {
        if !is_valid_media_url(server_url) {
            return Err(
                "invalid blossom server URL (private or loopback hosts are not allowed)"
                    .to_string(),
            );
        }
        let parsed =
            url::Url::parse(server_url).map_err(|e| format!("blossom URL parse error: {e}"))?;
        let host = parsed
            .host_str()
            .map(|h| h.to_string())
            .ok_or_else(|| "blossom URL has no host".to_string())?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "blossom URL has no port".to_string())?;
        let mut pinned_addrs: Vec<std::net::SocketAddr> = Vec::new();
        match tokio::net::lookup_host((host.as_str(), port)).await {
            Ok(addrs) => {
                for addr in addrs {
                    if soshal_common_core::url::is_private_ip_str(&addr.ip().to_string()) {
                        return Err("blossom server resolves to an internal address".to_string());
                    }
                    pinned_addrs.push(addr);
                }
            }
            Err(_) => return Err("blossom server does not resolve".to_string()),
        }
        if pinned_addrs.is_empty() {
            return Err("blossom server does not resolve".to_string());
        }
        Ok(Self::with_http(server_url, Some(&host), &pinned_addrs))
    }

    /// Builds a client that never follows redirects (a redirecting server
    /// must not be able to funnel uploads/downloads to an attacker-chosen
    /// endpoint) and bounds connect/response time.
    ///
    /// Rejects loopback/private/link-local/DNS-rebinding hosts (SSRF guard):
    /// `server_url` is user-controlled (settings, NIP-65 lists) and must
    /// never point the client at internal networks. Hostname-level check
    /// only; use [`Self::new_pinned_resolve`] for DNS-rebinding protection.
    pub fn new(server_url: &str) -> Result<Self, String> {
        if !is_valid_media_url(server_url) {
            return Err(
                "invalid blossom server URL (private or loopback hosts are not allowed)"
                    .to_string(),
            );
        }
        Ok(Self::with_http(server_url, None, &[]))
    }

    /// Same hardened client as [`Self::new`], but pins `host` to the caller-
    /// validated `addrs` (SSRF-checked resolution). Pinning means the OS
    /// resolver cannot re-resolve the host to a different address at connect
    /// time (DNS rebinding TOCTOU).
    pub fn new_pinned(server_url: &str, host: &str, addrs: &[std::net::SocketAddr]) -> Self {
        Self::with_http(server_url, Some(host), addrs)
    }

    fn with_http(server_url: &str, host: Option<&str>, addrs: &[std::net::SocketAddr]) -> Self {
        // SECURITY: no_proxy forces direct connections — an env
        // HTTP(S)_PROXY would resolve the host itself, bypassing the
        // private-IP/DNS-pin SSRF guards below.
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .no_proxy();
        if let Some(host) = host {
            // Pin ALL resolved addresses: per-addr `.resolve()` calls
            // overwrite each other, leaving a DNS-rebinding window.
            if !addrs.is_empty() {
                builder = builder.resolve_to_addrs(host, addrs);
            }
        }
        let http = builder
            .build()
            // SECURITY: fallback must also ignore env proxies (would bypass
            // the SSRF guards) — never fall back to a bare Client::new().
            .unwrap_or_else(|_| {
                Client::builder()
                    .no_proxy()
                    .build()
                    .expect("no_proxy client")
            });
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
        let buf = Self::read_response_capped(resp, MAX_LIST_BYTES, "upload").await?;
        let file: MediaFile =
            serde_json::from_slice(&buf).map_err(|e| format!("parse failed: {}", e))?;
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
        let buf = Self::read_response_capped(resp, MAX_LIST_BYTES, "upload").await?;
        let file: MediaFile =
            serde_json::from_slice(&buf).map_err(|e| format!("parse failed: {}", e))?;
        Ok(file)
    }

    async fn read_response_capped(
        resp: reqwest::Response,
        max_bytes: usize,
        op: &str,
    ) -> Result<Vec<u8>, String> {
        if let Some(len) = resp.content_length() {
            if len as usize > max_bytes {
                return Err(format!("{op} response too large: {len} bytes"));
            }
        }
        let cap = resp
            .content_length()
            .map(|l| (l as usize).min(max_bytes))
            .unwrap_or(64 * 1024);
        let mut buf = Vec::with_capacity(cap);
        let mut resp = resp;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("read failed: {}", e))?
        {
            if buf.len() + chunk.len() > max_bytes {
                return Err(format!("{op} response exceeds size cap"));
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }

    pub async fn download(&self, hash: &str) -> Result<Vec<u8>, String> {
        // SECURITY: the hash is spliced into the URL path; a hash from a
        // hostile blossom server must not be able to inject path segments,
        // query strings or fragments (`..`, `/`, `?`, `#`). Only a canonical
        // 64-char lowercase hex sha256 is accepted.
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid file hash".into());
        }
        let hash_lower = hash.to_ascii_lowercase();
        let url = format!("{}/{}", self.server_url, hash_lower);
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
        // SECURITY: the hash in the URL path is asserted, not trusted — a
        // hostile or misconfigured blossom server could serve different
        // bytes. Verify the payload matches the requested sha256.
        let digest_hex = soshal_crypto_core::hash::sha256_hex(&bytes);
        if !digest_hex.eq_ignore_ascii_case(hash) {
            return Err("downloaded blob hash mismatch".into());
        }
        Ok(bytes)
    }

    pub async fn list(&self, pubkey: &str) -> Result<Vec<MediaFile>, String> {
        if pubkey.is_empty()
            || pubkey.len() > 128
            || !pubkey.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err("invalid pubkey for blossom list".to_string());
        }
        let pubkey_lower = pubkey.to_ascii_lowercase();
        let url = format!("{}/list/{}", self.server_url, pubkey_lower);
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
