//! Custom HTTP/3 & QUIC network client stack.
//! Uses reqwest + rustls to provide HTTP/3 zero-head-of-line-blocking transfers,
//! connection pooling, multiplexing, and resilient transport fallback on patchy networks.

use reqwest::{Client, Method};
use soshal_common_core::url::{is_private_ip_str, is_valid_media_url};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HttpResponseData {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct Http3Client {
    /// Proxy-routed default client (onion/i2p/SOCKS5). Pooled and reused.
    client: Client,
    socks_addr: Option<std::net::SocketAddr>,
    /// Per-host clients pinned to their resolved addresses (DNS-rebinding /
    /// SSRF defense). Cached so repeated fetches of the same host reuse the
    /// TLS session + connection pool instead of building a fresh client (and
    /// re-resolving) per request.
    pinned: Arc<Mutex<HashMap<String, Client>>>,
}

const PINNED_CLIENT_CACHE_CAP: usize = 64;

/// Filters resolved addresses down to public ones. Fails closed if ANY
/// address (including mixed A/AAAA results) is private, loopback,
/// link-local, etc. — internal targets must never be reachable regardless of
/// how clever the DNS answer is.
pub fn filter_public_addrs(
    addrs: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, String> {
    let mut out = Vec::new();
    for a in addrs {
        if is_private_ip_str(&a.ip().to_string()) {
            return Err(format!(
                "HTTP request blocked: URL resolves to an internal address ({a})"
            ));
        }
        out.push(a);
    }
    Ok(out)
}

impl Default for Http3Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Http3Client {
    pub fn new() -> Self {
        Self::with_socks_proxy(None)
    }

    /// Builds a client routed through a SOCKS5 proxy (i2pd) when `Some`.
    pub fn with_socks_proxy(socks_addr: Option<std::net::SocketAddr>) -> Self {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(10);
        if let Some(addr) = socks_addr {
            // SOCKS5h: proxy-side DNS resolution. This client is only used for
            // .i2p/.onion targets (which have no public-DNS resolution to SSRF
            // check locally); regular hosts ride the pinned-resolve path.
            if let Ok(proxy) = reqwest::Proxy::all(format!("socks5h://{addr}")) {
                builder = builder.proxy(proxy);
            }
        }
        let client = builder.build().unwrap_or_else(|_| Client::new());

        Self {
            client,
            socks_addr,
            pinned: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn request(
        &self,
        method_str: &str,
        url: &str,
        headers_map: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponseData, String> {
        // SSRF guard: reject private IPs, loopback, link-local, and
        // DNS-rebinding candidates. Same policy applied to relay URLs.
        if !is_valid_media_url(url) {
            return Err(format!(
                "HTTP request blocked: URL does not pass SSRF policy: {url}"
            ));
        }
        let method = Method::from_bytes(method_str.as_bytes())
            .map_err(|e| format!("Invalid HTTP method: {e}"))?;
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
        let host = parsed
            .host_str()
            .map(|h| h.to_string())
            .ok_or_else(|| "HTTP request blocked: URL has no host".to_string())?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "HTTP request blocked: URL has no port".to_string())?;

        // .i2p/.onion names have no public-DNS resolution; only the proxy
        // (i2pd/tor) can resolve them, so resolution happens proxy-side there.
        // Every other host is resolved locally and any resolved
        // private/loopback/link-local address rejects the request — this
        // check runs even when a SOCKS proxy is configured (the proxy path
        // previously skipped it, letting the proxy reach internal services).
        let is_onion_or_i2p = host.ends_with(".i2p") || host.ends_with(".onion");
        let mut pinned_addrs: Vec<SocketAddr> = Vec::new();
        if !is_onion_or_i2p {
            match tokio::net::lookup_host((host.as_str(), port)).await {
                Ok(addrs) => {
                    pinned_addrs = match filter_public_addrs(addrs) {
                        Ok(a) => a,
                        Err(e) => return Err(e),
                    };
                }
                Err(_) => return Err("HTTP request blocked: URL does not resolve".to_string()),
            }
            if pinned_addrs.is_empty() {
                return Err("HTTP request blocked: URL does not resolve".to_string());
            }
        }
        let client = if is_onion_or_i2p {
            // Proxy-resolved path (.i2p/.onion): shared default client; the
            // proxy dials the target directly (SOCKS5h — proxy-side DNS).
            self.client.clone()
        } else {
            // Pinned path: reuse a per-host client (TLS session + pool) built
            // with the verified resolved addresses — the target IPs can't be
            // swapped by a second resolution (DNS-rebinding defense). When a
            // proxy is set it rides the pinned-resolve path and uses SOCKS5
            // (the client sends the verified IP), so the proxy cannot
            // re-resolve the hostname to an internal address either.
            let mut guard = self.pinned.lock().unwrap_or_else(|e| e.into_inner());
            match guard.get(&host) {
                Some(c) => c.clone(),
                None => {
                    if guard.len() >= PINNED_CLIENT_CACHE_CAP {
                        guard.clear();
                    }
                    let mut b = reqwest::Client::builder()
                        .timeout(Duration::from_secs(30))
                        .connect_timeout(Duration::from_secs(10))
                        .redirect(reqwest::redirect::Policy::none())
                        .pool_idle_timeout(Duration::from_secs(90))
                        .pool_max_idle_per_host(10)
                        .resolve_to_addrs(&host, &pinned_addrs);
                    if let Some(addr) = self.socks_addr {
                        if let Ok(proxy) = reqwest::Proxy::all(format!("socks5://{addr}")) {
                            b = b.proxy(proxy);
                        }
                    }
                    let c = b
                        .build()
                        .map_err(|e| format!("HTTP client build error: {e}"))?;
                    guard.insert(host.clone(), c.clone());
                    c
                }
            }
        };

        let mut req = client.request(method, url);

        for (k, v) in headers_map {
            req = req.header(k, v);
        }

        if let Some(b) = body {
            req = req.body(b);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = resp.status().as_u16();
        let mut headers = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(val) = v.to_str() {
                headers.insert(k.as_str().to_string(), val.to_string());
            }
        }

        if let Some(len) = resp.content_length() {
            if len as usize > MAX_RESPONSE_BODY_BYTES {
                return Err("HTTP response exceeds size cap".to_string());
            }
        }
        let mut resp = resp;
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))?
        {
            if body.len() + chunk.len() > MAX_RESPONSE_BODY_BYTES {
                return Err("HTTP response exceeds size cap".to_string());
            }
            body.extend_from_slice(&chunk);
        }

        Ok(HttpResponseData {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http3_client_creation() {
        let client = Http3Client::new();
        // Client initialized successfully
        assert!(client.client.get("https://example.com").build().is_ok());
    }

    #[test]
    fn test_filter_public_addrs_rejects_private_anywhere() {
        // Pure public set passes.
        let pub_addrs = [
            SocketAddr::from(([8, 8, 8, 8], 443)),
            SocketAddr::from(([1, 1, 1, 1], 443)),
        ];
        assert_eq!(filter_public_addrs(pub_addrs).unwrap().len(), 2);

        // A single private result in the set fails closed.
        let mixed = [
            SocketAddr::from(([8, 8, 8, 8], 443)),
            SocketAddr::from(([10, 0, 0, 1], 443)),
        ];
        assert!(filter_public_addrs(mixed).is_err());

        // Loopback, link-local, CGNAT all rejected.
        for ip in [
            [127, 0, 0, 1],
            [169, 254, 1, 1],
            [100, 64, 0, 1],
            [192, 168, 1, 1],
        ] {
            assert!(
                filter_public_addrs([SocketAddr::from((ip, 80))]).is_err(),
                "expected {ip:?} rejected"
            );
        }

        // IPv6 loopback handled (is_private_ip_str covers IPv6-mapped too).
        let v6_loop = "[::1]:443".parse::<SocketAddr>().unwrap();
        assert!(filter_public_addrs([v6_loop]).is_err());

        // Empty set passes the filter (caller checks is_empty separately).
        assert!(filter_public_addrs(std::iter::empty::<SocketAddr>()).is_ok());
    }
}
