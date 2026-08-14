//! NIP-05 identity verification and resolution.
//!
//! Both [`verify`] and [`resolve`] fetch `https://<domain>/.well-known/nostr.json`
//! through a DNS-pinned reqwest client that rejects loopback/private/link-local
//! addresses, blocking SSRF and DNS-rebinding attacks.

use nostr::nips::nip05::Nip05Address;
use soshal_common_core::url::{is_private_ip_str, is_valid_media_url, is_valid_relay_url};

/// Maximum size of the `.well-known/nostr.json` response body.
const MAX_NIP05_BODY: usize = 256 * 1024;

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
    let mut client_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    // Pin ALL resolved addresses (per-addr `.resolve()` overwrites each
    // other, leaving a DNS-rebinding window).
    client_builder = client_builder.resolve_to_addrs(&host, &pinned_addrs);
    let client = client_builder
        .build()
        .map_err(|e| format!("http client error: {}", e))?;
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
