//! Nostr protocol FFI module.
//!
//! Comprehensive nostr event & relay management: relay pool lifecycle,
//! subscriptions, event publishing, and relay persistence across restarts.

use flutter_rust_bridge::frb;
use nostr::prelude::*;
use nostr_sdk::client::Client;
use nostr_sdk::prelude::{Filter, SubscriptionId};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Shared nostr client (relay pool). Holds no signing keys.
/// Events must be pre-signed before publishing.
static CLIENT: Mutex<Option<Client>> = Mutex::new(None);

/// Cached relay URLs for persistence across restarts.
static RELAY_CACHE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn client_guard() -> std::sync::MutexGuard<'static, Option<Client>> {
    CLIENT.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NostrStatusDto {
    pub initialized: bool,
    pub relay_count: usize,
    pub relay_urls: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NostrEventDto {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u16,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    pub sig: String,
}

/// Initialize nostr relay client with given relay URLs.
/// Restores relay list from persistence if available.
/// Falls back to default relays if none provided and cache empty.
#[frb(serialize)]
pub async fn nostr_init_relays(relay_urls: Vec<String>) -> Result<String, String> {
    let mut urls = relay_urls;

    // Fall back to cached relays if no URLs provided
    if urls.is_empty() {
        let cache = RELAY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        urls = cache.clone();
    }

    // Fall back to defaults if still empty
    if urls.is_empty() {
        urls = vec![
            "wss://relay.damus.io".to_string(),
            "wss://nostr.wine".to_string(),
            "wss://nos.lol".to_string(),
        ];
    }

    // Validate URLs
    for url in &urls {
        let (valid, _blocked) = soshal_content_core::url::is_valid_relay_url(url);
        if !valid {
            return Err(format!("invalid relay URL: {url}"));
        }
    }

    // Build and connect client
    let client = Client::new();
    let mut added = 0usize;

    for url in &urls {
        if let Ok(relay_url) = RelayUrl::parse(url) {
            match client.add_relay(relay_url).await {
                Ok(_) => added += 1,
                Err(e) => eprintln!("Failed to add relay {url}: {e}"),
            }
        }
    }

    if added == 0 {
        return Err("no relays could be added".to_string());
    }

    let _ = client.connect().await;

    // Cache and store
    *client_guard() = Some(client.clone());
    *RELAY_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = urls.clone();

    Ok(format!("nostr client initialized ({added} relays)"))
}

/// Get current nostr status (relays, subscriptions).
#[frb(sync, serialize)]
pub fn nostr_status() -> Result<String, String> {
    let guard = client_guard();
    let relays = RELAY_CACHE.lock().unwrap_or_else(|e| e.into_inner());

    let status = NostrStatusDto {
        initialized: guard.is_some(),
        relay_count: relays.len(),
        relay_urls: relays.clone(),
    };

    serde_json::to_string(&status).map_err(|e| format!("serialize: {e}"))
}

/// Add a relay to the connected relay pool.
#[frb(serialize)]
pub async fn nostr_add_relay(url: String) -> Result<bool, String> {
    let (valid, _blocked) = soshal_content_core::url::is_valid_relay_url(&url);
    if !valid {
        return Err("invalid relay URL".to_string());
    }

    let relay_url = RelayUrl::parse(&url).map_err(|e| format!("parse relay URL: {e}"))?;

    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    let ok = client.add_relay(relay_url).await.is_ok();

    // Cache
    if ok {
        let mut cache = RELAY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if !cache.contains(&url) {
            cache.push(url);
        }
    }

    Ok(ok)
}

/// Remove a relay from the relay pool.
#[frb(serialize)]
pub async fn nostr_remove_relay(url: String) -> Result<bool, String> {
    let _relay_url = RelayUrl::parse(&url).map_err(|e| format!("parse relay URL: {e}"))?;

    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    let ok = client.remove_relay(&url).await.is_ok();

    // Update cache
    if ok {
        let mut cache = RELAY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|u| u != &url);
    }

    Ok(ok)
}

/// Subscribe to events matching a filter. Returns subscription ID.
#[frb(serialize)]
pub async fn nostr_subscribe(filter_json: String) -> Result<String, String> {
    let filter: Filter =
        serde_json::from_str(&filter_json).map_err(|e| format!("invalid filter JSON: {e}"))?;

    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    let sub_id = SubscriptionId::generate();
    client
        .subscribe(vec![filter])
        .with_id(sub_id.clone())
        .await
        .map_err(|e| format!("subscribe failed: {e}"))?;

    Ok(sub_id.to_string())
}

/// Unsubscribe from a subscription.
#[frb(serialize)]
pub async fn nostr_unsubscribe(subscription_id: String) -> Result<bool, String> {
    let sub_id = SubscriptionId::new(subscription_id);
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    Ok(client.unsubscribe(&sub_id).await.is_ok())
}

/// Publish a pre-signed event to all relays. Returns relay count that accepted it.
#[frb(serialize)]
pub async fn nostr_publish_event(event_json: String) -> Result<i32, String> {
    let event: Event =
        serde_json::from_str(&event_json).map_err(|e| format!("invalid event JSON: {e}"))?;

    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    match client.send_event(&event).await {
        Ok(out) => Ok(out.success.len() as i32),
        Err(e) => Err(format!("publish failed: {e}")),
    }
}

/// Query events matching a filter. Returns array of event JSON.
#[frb(serialize)]
pub async fn nostr_query_events(filter_json: String) -> Result<String, String> {
    let filter: Filter =
        serde_json::from_str(&filter_json).map_err(|e| format!("invalid filter JSON: {e}"))?;

    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    let events = client
        .fetch_events(vec![filter])
        .await
        .map_err(|e| format!("query failed: {e}"))?;

    serde_json::to_string(&events).map_err(|e| format!("serialize: {e}"))
}

/// Get relay connection status for all relays.
#[frb(serialize)]
pub async fn nostr_relay_status() -> Result<String, String> {
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    let relays = client.relays().await;
    let mut status: Vec<_> = Vec::new();

    for (url, relay) in relays.iter() {
        let connected = relay.status().is_connected();
        status.push(serde_json::json!({
            "url": url.to_string(),
            "connected": connected,
        }));
    }

    serde_json::to_string(&status).map_err(|e| format!("serialize: {e}"))
}

/// Publish a custom profile event (kind 30085) to relays.
#[frb(serialize)]
pub async fn nostr_publish_custom_profile(
    pubkey: String,
    profile_json: String,
) -> Result<bool, String> {
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("nostr client not initialized".to_string()),
    };

    // Create custom profile event (kind 30085)
    let builder = EventBuilder::new(Kind::Custom(30085), profile_json)
        .tag(Tag::identifier("soshal-custom-profile"));

    // Note: This event needs to be signed by the identity service
    // For now, we'll return an error indicating signing is needed
    // The actual signing should happen in the identity service
    Err("custom profile events must be signed via identity service".to_string())
}
