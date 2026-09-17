//! NIP-05 identity verification and resolution.
//!
//! Both [`verify`] and [`resolve`] fetch `https://<domain>/.well-known/nostr.json`
//! through a DNS-pinned reqwest client that rejects loopback/private/link-local
//! addresses, blocking SSRF and DNS-rebinding attacks.

use nostr::nips::nip05::Nip05Address;
use soshal_common_core::url::{is_private_ip_str, is_valid_media_url, is_valid_relay_url};
use std::collections::{HashMap, VecDeque};

/// Maximum size of the `.well-known/nostr.json` response body.
const MAX_NIP05_BODY: usize = 256 * 1024;

type Nip05ClientCache = (HashMap<String, reqwest::Client>, VecDeque<String>);

/// DNS-pinned reqwest clients per domain. DNS pins are per-host (built from
/// a fresh `lookup_host` each fetch), but the connection pool is reused for
/// repeat lookups of the same domain — NIP-05 checks commonly batch several
/// addresses on one domain.
static NIP05_CLIENTS: std::sync::Mutex<Option<Nip05ClientCache>> = std::sync::Mutex::new(None);

const MAX_NIP05_CLIENTS: usize = 16;

fn client_for(
    host: &str,
    pinned_addrs: &[std::net::SocketAddr],
) -> Result<reqwest::Client, String> {
    let mut guard = NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
    let (map, order) = guard.get_or_insert_with(|| {
        (
            std::collections::HashMap::with_capacity(8),
            std::collections::VecDeque::new(),
        )
    });
    if let Some(client) = map.get(host) {
        return Ok(client.clone());
    }
    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    // Pin ALL resolved addresses (per-addr `.resolve()` overwrites each
    // other, leaving a DNS-rebinding window).
    client_builder = client_builder.resolve_to_addrs(host, pinned_addrs);
    let client = client_builder
        .build()
        .map_err(|e| format!("http client error: {}", e))?;
    while map.len() >= MAX_NIP05_CLIENTS {
        if let Some(oldest) = order.pop_front() {
            map.remove(&oldest);
        }
    }
    order.push_back(host.to_string());
    map.insert(host.to_string(), client.clone());
    Ok(client)
}

#[derive(Debug, serde::Serialize)]
pub struct Nip05Result {
    pub verified: bool,
    pub pubkey: Option<String>,
    pub relays: Vec<String>,
    pub error: Option<String>,
}

/// Fetches and parses the NIP-05 `nostr.json` document for `nip05_address`
/// with full SSRF protection. Returns the parsed JSON on success.
async fn fetch_nostr_json(nip05_address: &str) -> Result<serde_json::Value, String> {
    let address =
        Nip05Address::parse(nip05_address).map_err(|e| format!("invalid nip05 address: {}", e))?;
    let url = address.url();
    if !is_valid_media_url(url.as_str()) {
        return Err("nip05 domain is not allowed (private or local host)".into());
    }
    let host = url::Url::parse(url.as_str())
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .ok_or_else(|| "nip05 url has no host".to_string())?;
    let mut pinned_addrs: Vec<std::net::SocketAddr> = Vec::new();
    match tokio::net::lookup_host((host.as_str(), 443)).await {
        Ok(addrs) => {
            for addr in addrs {
                if is_private_ip_str(&addr.ip().to_string()) {
                    return Err("nip05 domain resolves to an internal address".into());
                }
                pinned_addrs.push(addr);
            }
        }
        Err(_) => return Err("nip05 domain does not resolve".into()),
    }
    if pinned_addrs.is_empty() {
        return Err("nip05 domain does not resolve".into());
    }
    let client = client_for(&host, &pinned_addrs)?;
    let resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("nip05 fetch error: {}", e))?;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_NIP05_BODY {
            return Err("nip05 response too large".into());
        }
    }
    // SECURITY: stream with a hard cap — a missing or lying Content-Length
    // must not allow an unbounded in-memory response from the host.
    let mut body = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("nip05 read error: {}", e))?
    {
        if body.len() + chunk.len() > MAX_NIP05_BODY {
            return Err("nip05 response too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| format!("invalid nip05 response: {}", e))
}

/// Verifies a NIP-05 address by fetching `https://<domain>/.well-known/nostr.json`
/// and checking that `public_key` is the name's claimed owner.
pub async fn verify(nip05_address: &str, public_key: &str) -> Nip05Result {
    let pk = match nostr::key::PublicKey::from_hex(public_key) {
        Ok(p) => p,
        Err(e) => {
            return Nip05Result {
                verified: false,
                pubkey: None,
                relays: vec![],
                error: Some(format!("invalid pubkey: {}", e)),
            }
        }
    };
    let address = match Nip05Address::parse(nip05_address) {
        Ok(a) => a,
        Err(e) => {
            return Nip05Result {
                verified: false,
                pubkey: None,
                relays: vec![],
                error: Some(format!("invalid nip05 address: {}", e)),
            }
        }
    };
    let json = match fetch_nostr_json(nip05_address).await {
        Ok(j) => j,
        Err(e) => {
            return Nip05Result {
                verified: false,
                pubkey: None,
                relays: vec![],
                error: Some(e),
            }
        }
    };
    let verified = nostr::nips::nip05::verify_from_json(&pk, &address, &json);
    let relays = json["relays"][public_key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                // Only return usable relay URLs; NIP-05 JSON is external
                // untrusted data, so wss:// only (no cleartext, no
                // loopback/private/raw-IP endpoints).
                .filter(|r| is_valid_relay_url(r).0 && r.starts_with("wss://"))
                .collect()
        })
        .unwrap_or_default();
    Nip05Result {
        verified,
        pubkey: if verified {
            Some(public_key.to_string())
        } else {
            None
        },
        relays,
        error: None,
    }
}

/// Resolves a NIP-05 address to its claimed pubkey without knowing the key
/// beforehand (used for import flows). Relays advertised for the resolved
/// pubkey are returned too.
pub async fn resolve(nip05_address: &str) -> Nip05Result {
    let address = match Nip05Address::parse(nip05_address) {
        Ok(a) => a,
        Err(e) => {
            return Nip05Result {
                verified: false,
                pubkey: None,
                relays: vec![],
                error: Some(format!("invalid nip05 address: {}", e)),
            }
        }
    };
    let name = address.name().to_string();
    let json = match fetch_nostr_json(nip05_address).await {
        Ok(j) => j,
        Err(e) => {
            return Nip05Result {
                verified: false,
                pubkey: None,
                relays: vec![],
                error: Some(e),
            }
        }
    };
    let Some(pubkey) = json["names"][&name].as_str().map(|s| s.to_string()) else {
        return Nip05Result {
            verified: false,
            pubkey: None,
            relays: vec![],
            error: Some(format!("no pubkey listed for {name}")),
        };
    };
    if let Err(e) = nostr::key::PublicKey::from_hex(&pubkey) {
        return Nip05Result {
            verified: false,
            pubkey: None,
            relays: vec![],
            error: Some(format!("invalid pubkey in nip05 response: {e}")),
        };
    }
    let relays = json["relays"][&pubkey]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|r| is_valid_relay_url(r).0 && r.starts_with("wss://"))
                .collect()
        })
        .unwrap_or_default();
    Nip05Result {
        verified: true,
        pubkey: Some(pubkey),
        relays,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static NIP05_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn dead_addrs() -> Vec<std::net::SocketAddr> {
        vec!["127.0.0.1:1".parse().unwrap()]
    }

    fn cache_size() -> usize {
        let guard = NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|(map, _)| map.len()).unwrap_or(0)
    }

    #[test]
    fn client_for_caches_per_host() {
        let _g = NIP05_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        client_for("cache-host-a.example", &dead_addrs()).unwrap();
        client_for("cache-host-a.example", &dead_addrs()).unwrap();
        assert_eq!(cache_size(), 1);
        client_for("cache-host-b.example", &dead_addrs()).unwrap();
        assert_eq!(cache_size(), 2);
        *NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[test]
    fn client_for_evicts_oldest_at_capacity() {
        let _g = NIP05_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        for i in 0..(MAX_NIP05_CLIENTS + 5) {
            client_for(&format!("evict-{i}.example"), &dead_addrs()).unwrap();
        }
        assert_eq!(cache_size(), MAX_NIP05_CLIENTS);
        // The oldest host must have been evicted: re-requesting it builds a
        // fresh client and still keeps the cache at capacity.
        client_for("evict-0.example", &dead_addrs()).unwrap();
        assert_eq!(cache_size(), MAX_NIP05_CLIENTS);
        *NIP05_CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[tokio::test]
    async fn fetch_rejects_private_ip_resolution() {
        // Raw private IPs must be rejected before any request is attempted
        // (either at the URL check or at the post-lookup check).
        for host in ["10.0.0.1", "192.168.1.1", "127.0.0.1"] {
            let err = fetch_nostr_json(&format!("user@{host}")).await.unwrap_err();
            assert!(
                err.contains("not allowed") || err.contains("internal address"),
                "host {host}: got {err}"
            );
        }
    }
}
