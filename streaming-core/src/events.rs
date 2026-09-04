//! Story / live-stream / live-chat / chatrandom event mapping and content
//! shaping.

use soshal_nostr_core::models::NostrEvent;

/// Builds kind-30078 story content JSON.
pub fn story_content(media_urls: &[String], text: Option<&str>) -> Result<String, String> {
    let mut content = serde_json::json!({
        "media": media_urls.iter().map(|u| serde_json::json!({"url": u, "type": "image"})).collect::<Vec<_>>(),
    });
    if let Some(t) = text {
        content["text"] = serde_json::json!(t);
    }
    serde_json::to_string(&content).map_err(|e| format!("serialize: {}", e))
}

/// Maps a kind-30078 story event to its webview JSON.
pub fn story_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
        "expiration": find_tag_opt(&ev.tags, "expiration"),
        "audience": find_tag_opt(&ev.tags, "audience").unwrap_or_else(|| "public".to_string()),
    })
}

/// Maps a kind-1311 live chat message event to its webview JSON.
pub fn live_chat_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}

/// Builds kind-30311 live stream content JSON.
pub fn stream_content(
    title: &str,
    summary: Option<&str>,
    stream_url: &str,
    status: &str,
    category: Option<&str>,
) -> Result<String, String> {
    let mut content = serde_json::json!({
        "title": title,
        "stream_url": stream_url,
        "status": status,
    });
    if let Some(s) = summary {
        content["summary"] = serde_json::json!(s);
    }
    if let Some(c) = category {
        if !c.is_empty() {
            content["category"] = serde_json::json!(c);
        }
    }
    serde_json::to_string(&content).map_err(|e| format!("serialize: {}", e))
}

/// Maps a kind-30311 live stream event to its webview JSON.
pub fn live_stream_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
        "d_tag": find_tag_opt(&ev.tags, "d"),
        "status_tag": find_tag_opt(&ev.tags, "status"),
        "audience": find_tag_opt(&ev.tags, "audience").unwrap_or_else(|| "public".to_string()),
        "category": find_tag_opt(&ev.tags, "category").unwrap_or_else(|| "Other".to_string()),
    })
}

/// Builds kind-20030 chatrandom availability content JSON.
pub fn chatrandom_available_content(interests: &[String], media_type: &str, mode: &str) -> String {
    serde_json::json!({
        "interests": interests,
        "media_type": media_type,
        "mode": mode,
    })
    .to_string()
}

/// Maps a kind-20030 chatrandom availability event to its webview JSON.
pub fn chatrandom_peer_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}

/// Maps a chatrandom request type to its event kind + content JSON.
pub fn chatrandom_request_parts(request_type: &str) -> Result<(u16, String), String> {
    let kind = match request_type {
        "available" => 20030u16,
        "request" => 20031u16,
        "accept" => 20032,
        _ => return Err("invalid request type, use available/request/accept".into()),
    };
    let content = serde_json::json!({"type": request_type}).to_string();
    Ok((kind, content))
}

fn find_tag_opt(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some(name))
        .and_then(|t| t.get(1))
        .cloned()
}
