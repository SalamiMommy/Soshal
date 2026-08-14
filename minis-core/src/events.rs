//! Mini-App / Musicloud / Custom Profile event mapping and content shaping.

use serde::{Deserialize, Serialize};
use soshal_nostr_core::models::NostrEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniEventOut {
    pub id: String,
    pub pubkey: String,
    pub video_url: String,
    pub text_overlay: String,
    pub thumbnail: String,
    pub audience: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicloudEventOut {
    pub id: String,
    pub pubkey: String,
    pub audio_url: String,
    pub title: String,
    pub thumbnail: String,
    pub hashtags: Vec<String>,
    pub d: String,
    pub audience: String,
    pub created_at: u64,
}

/// Maps a kind-31020 mini event to its typed struct. Returns `None` when the
/// `url` tag is missing.
pub fn mini_from_event(ev: &NostrEvent) -> Option<serde_json::Value> {
    let url = find_tag_str(&ev.tags, "url");
    if url.is_empty() {
        return None;
    }
    let thumb = find_tag_str(&ev.tags, "image");
    let audience = find_tag_str(&ev.tags, "audience");
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    Some(serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "videoUrl": url,
        "textOverlay": ev.content,
        "thumbnail": thumb,
        "audience": audience,
        "createdAt": ev.created_at as u64,
    }))
}

/// Maps a kind-31020 mini event to a strongly typed `MiniEventOut` struct.
pub fn mini_event_out(ev: &NostrEvent) -> Option<MiniEventOut> {
    let url = find_tag_str(&ev.tags, "url");
    if url.is_empty() {
        return None;
    }
    let thumb = find_tag_str(&ev.tags, "image");
    let audience = find_tag_str(&ev.tags, "audience");
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    Some(MiniEventOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        video_url: url.to_string(),
        text_overlay: ev.content.clone(),
        thumbnail: thumb.to_string(),
        audience: audience.to_string(),
        created_at: ev.created_at as u64,
    })
}

/// Maps a kind-31022 musicloud event to its webview JSON.
pub fn musicloud_from_event(ev: &NostrEvent) -> Option<serde_json::Value> {
    let mut url = "";
    let mut title = "";
    let mut thumbnail = "";
    let mut d_tag = "";
    let mut audience = "";
    let mut hashtags = Vec::new();

    for tag in &ev.tags {
        if tag.len() >= 2 {
            match tag[0].as_str() {
                "url" if url.is_empty() => url = &tag[1],
                "title" if title.is_empty() => title = &tag[1],
                "image" if thumbnail.is_empty() => thumbnail = &tag[1],
                "d" if d_tag.is_empty() => d_tag = &tag[1],
                "audience" if audience.is_empty() => audience = &tag[1],
                "t" => hashtags.push(tag[1].clone()),
                _ => {}
            }
        }
    }
    if url.is_empty() {
        return None;
    }
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    Some(serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "audioUrl": url,
        "title": title,
        "thumbnail": thumbnail,
        "hashtags": hashtags,
        "d": d_tag,
        "audience": audience,
        "createdAt": ev.created_at as u64,
    }))
}

/// Maps a kind-31022 musicloud event to a strongly typed `MusicloudEventOut` struct.
pub fn musicloud_event_out(ev: &NostrEvent) -> Option<MusicloudEventOut> {
    let mut url = "";
    let mut title = "";
    let mut thumbnail = "";
    let mut d_tag = "";
    let mut audience = "";
    let mut hashtags = Vec::new();

    for tag in &ev.tags {
        if tag.len() >= 2 {
            match tag[0].as_str() {
                "url" if url.is_empty() => url = &tag[1],
                "title" if title.is_empty() => title = &tag[1],
                "image" if thumbnail.is_empty() => thumbnail = &tag[1],
                "d" if d_tag.is_empty() => d_tag = &tag[1],
                "audience" if audience.is_empty() => audience = &tag[1],
                "t" => hashtags.push(tag[1].clone()),
                _ => {}
            }
        }
    }
    if url.is_empty() {
        return None;
    }
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    Some(MusicloudEventOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        audio_url: url.to_string(),
        title: title.to_string(),
        thumbnail: thumbnail.to_string(),
        hashtags,
        d: d_tag.to_string(),
        audience: audience.to_string(),
        created_at: ev.created_at as u64,
    })
}

/// Builds the kind-30085 custom profile content payload.
pub fn custom_profile_content(nodes: &serde_json::Value, theme_id: &str) -> Result<String, String> {
    let payload = serde_json::json!({ "themeId": theme_id, "nodes": nodes });
    serde_json::to_string(&payload).map_err(|e| format!("serialize: {}", e))
}

/// Builds the `a` tag coordinate for a musicloud track comment.
pub fn musicloud_comment_addr(track_kind: u16, track_pubkey: &str, track_d: &str) -> String {
    format!("{}:{}:{}", track_kind, track_pubkey, track_d)
}

/// Sorts webview JSON items by `createdAt` descending (newest first).
pub fn sort_by_created_desc(items: &mut [serde_json::Value]) {
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["createdAt"].as_u64().unwrap_or(0))
    });
}

/// Sorts typed `MiniEventOut` items by `created_at` descending (newest first).
pub fn sort_minis_desc(items: &mut [MiniEventOut]) {
    items.sort_by_key(|b| std::cmp::Reverse(b.created_at));
}

/// Sorts typed `MusicloudEventOut` items by `created_at` descending (newest first).
pub fn sort_musicloud_desc(items: &mut [MusicloudEventOut]) {
    items.sort_by_key(|b| std::cmp::Reverse(b.created_at));
}

pub fn find_tag_str<'a>(tags: &'a [Vec<String>], name: &str) -> &'a str {
    for t in tags {
        if t.len() >= 2 && t[0] == name {
            return &t[1];
        }
    }
    ""
}
