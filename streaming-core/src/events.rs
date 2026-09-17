//! Story / live-stream / live-chat / chatrandom event mapping and content
//! shaping.

use soshal_nostr_core::models::NostrEvent;

/// Builds kind-30078 story content JSON.
pub fn story_content(media_urls: &[String], text: Option<&str>) -> Result<String, String> {
    let has_text = text.is_some_and(|t| !t.trim().is_empty());
    if media_urls.is_empty() && !has_text {
        return Err("story must have text or at least one media URL".into());
    }
    if media_urls.len() > 32 {
        return Err("too many media URLs (max 32)".into());
    }
    for u in media_urls {
        if !soshal_common_core::url::is_valid_media_url(u) {
            return Err(format!("invalid or unsafe media URL: {}", u));
        }
    }
    if let Some(t) = text {
        if t.len() > 10_000 {
            return Err("text too long (max 10000)".into());
        }
    }
    let mut content = serde_json::json!({
        "media": media_urls.iter().map(|u| serde_json::json!({"url": u, "type": "image"})).collect::<Vec<_>>(),
    });
    if let Some(t) = text {
        content["text"] = serde_json::json!(t);
    }
    serde_json::to_string(&content).map_err(|e| format!("serialize: {}", e))
}

fn clamp_created_at(v: f64) -> u64 {
    if v.is_finite() && v >= 0.0 {
        v as u64
    } else {
        0
    }
}

/// Maps a kind-30078 story event to its webview JSON.
pub fn story_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": clamp_created_at(ev.created_at),
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
        "created_at": clamp_created_at(ev.created_at),
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
    if stream_url.is_empty()
        || stream_url.len() > 2048
        || !soshal_common_core::url::is_valid_media_url(stream_url)
    {
        return Err(format!("invalid or unsafe stream URL: {}", stream_url));
    }
    if title.len() > 500 {
        return Err("title too long (max 500)".into());
    }
    if let Some(s) = summary {
        if s.len() > 5000 {
            return Err("summary too long (max 5000)".into());
        }
    }
    if status.len() > 64 {
        return Err("status too long (max 64)".into());
    }
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
        "created_at": clamp_created_at(ev.created_at),
        "d_tag": find_tag_opt(&ev.tags, "d"),
        "status_tag": find_tag_opt(&ev.tags, "status"),
        "audience": find_tag_opt(&ev.tags, "audience").unwrap_or_else(|| "public".to_string()),
        "category": find_tag_opt(&ev.tags, "category").unwrap_or_else(|| "Other".to_string()),
    })
}

/// Builds kind-20030 chatrandom availability content JSON.
pub fn chatrandom_available_content(interests: &[String], media_type: &str, mode: &str) -> String {
    let bounded_interests: Vec<String> = interests
        .iter()
        .take(64)
        .map(|s| {
            if s.len() > 64 {
                s[..s.floor_char_boundary(64)].to_string()
            } else {
                s.clone()
            }
        })
        .collect();
    let media_type = if media_type.len() > 32 {
        &media_type[..media_type.floor_char_boundary(32)]
    } else {
        media_type
    };
    let mode = if mode.len() > 32 {
        &mode[..mode.floor_char_boundary(32)]
    } else {
        mode
    };
    serde_json::json!({
        "interests": bounded_interests,
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
        "created_at": clamp_created_at(ev.created_at),
    })
}

/// Maps a chatrandom request type to its event kind + content JSON.
pub fn chatrandom_request_parts(request_type: &str) -> Result<(u16, String), String> {
    let req_clean = request_type.trim().to_ascii_lowercase();
    let kind = match req_clean.as_str() {
        "available" => 20030u16,
        "request" => 20031u16,
        "accept" => 20032,
        _ => return Err("invalid request type, use available/request/accept".into()),
    };
    let content = serde_json::json!({"type": req_clean}).to_string();
    Ok((kind, content))
}

fn find_tag_opt(tags: &[Vec<String>], name: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some(name))
        .and_then(|t| t.get(1))
        .cloned()
}
