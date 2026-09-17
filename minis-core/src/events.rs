//! Mini-App / Musicloud / Custom Profile event mapping and content shaping.

use serde::{Deserialize, Serialize};
use soshal_nostr_core::models::{find_tag_values_map, NostrEvent};

fn clamp_created_at(v: f64) -> u64 {
    if v.is_finite() && v >= 0.0 {
        v as u64
    } else {
        0
    }
}

fn sanitize_media_url(u: &str) -> String {
    if u.starts_with("blob://") {
        if u.len() <= 2048 {
            return u.to_string();
        }
        return String::new();
    }
    if soshal_common_core::url::is_valid_media_url(u) {
        u.to_string()
    } else {
        String::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniEventOut {
    pub id: String,
    pub pubkey: String,
    pub video_url: String,
    pub blob_hash: String,
    pub media_size: u64,
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
    pub blob_hash: String,
    pub media_size: u64,
    pub title: String,
    pub thumbnail: String,
    pub hashtags: Vec<String>,
    pub d: String,
    pub audience: String,
    pub created_at: u64,
}

/// Extracts the `["media", type, url, blob_hash, size]` tag (feed wire
/// format) from an event's tags. The blob hash must be 64 lowercase hex
/// chars; malformed tags are ignored. Returns `(blob_hash, size)` with an
/// empty hash when no valid media tag exists (URL-fallback events).
pub fn media_blob_from_tags(tags: &[Vec<String>]) -> (String, u64) {
    for tag in tags {
        if tag.first().map(|s| s.as_str()) != Some("media") {
            continue;
        }
        if tag.len() < 5 {
            continue;
        }
        if !matches!(tag[1].as_str(), "image" | "video" | "audio") {
            continue;
        }
        let blob_hash = &tag[3];
        if blob_hash.len() != 64 || !blob_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let size: u64 = tag[4].parse().unwrap_or(0);
        return (blob_hash.clone(), size);
    }
    (String::new(), 0)
}

/// Maximum text overlay length for mini video overlays (guards against hostile note floods).
const MAX_OVERLAY_LEN: usize = 10_000;
const MAX_METADATA_LEN: usize = 500;
const MAX_AUDIENCE_LEN: usize = 64;

/// Maps a kind-31020 mini event to its typed struct. Returns `None` when the
/// `url` tag is missing.
pub fn mini_event_out(ev: &NostrEvent) -> Option<MiniEventOut> {
    let [url, thumb, audience] = find_tag_values_map(&ev.tags, ["url", "image", "audience"]);
    let url = sanitize_media_url(url.unwrap_or(""));
    if url.is_empty() {
        return None;
    }
    let thumb = sanitize_media_url(thumb.unwrap_or(""));
    let audience = audience.unwrap_or("");
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    let audience = if audience.len() > MAX_AUDIENCE_LEN {
        soshal_common_core::ui_safe::truncate_str(audience, MAX_AUDIENCE_LEN)
    } else {
        audience
    };
    let text_overlay = if ev.content.len() > MAX_OVERLAY_LEN {
        soshal_common_core::ui_safe::truncate_str(&ev.content, MAX_OVERLAY_LEN).to_string()
    } else {
        ev.content.clone()
    };
    let (blob_hash, media_size) = media_blob_from_tags(&ev.tags);
    Some(MiniEventOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        video_url: url,
        blob_hash,
        media_size,
        text_overlay,
        thumbnail: thumb,
        audience: audience.to_string(),
        created_at: clamp_created_at(ev.created_at),
    })
}

/// Maps a kind-31020 mini event to wire JSON (webview format). The typed
/// struct carries the identical camelCase field set, so this just serializes
/// [`MiniEventOut`].
pub fn mini_from_event(ev: &NostrEvent) -> Option<serde_json::Value> {
    serde_json::to_value(mini_event_out(ev)?).ok()
}

/// Max hashtags harvested from a musicloud track event before we stop
/// scanning (guards against hostile tag floods).
const MAX_HASHTAGS: usize = 10_000;
const MAX_HASHTAG_LEN: usize = 1024;

/// Maps a kind-31022 musicloud event to its typed struct. Returns `None`
/// when the `url` tag is missing.
pub fn musicloud_event_out(ev: &NostrEvent) -> Option<MusicloudEventOut> {
    let mut url = String::new();
    let mut title = "";
    let mut thumbnail = "";
    let mut d_tag = "";
    let mut audience = "";
    let mut hashtags = Vec::new();

    for tag in &ev.tags {
        if tag.len() >= 2 {
            match tag[0].as_str() {
                "url" if url.is_empty() => url = sanitize_media_url(&tag[1]),
                "title" if title.is_empty() => title = &tag[1],
                "image" if thumbnail.is_empty() => thumbnail = &tag[1],
                "d" if d_tag.is_empty() => d_tag = &tag[1],
                "audience" if audience.is_empty() => audience = &tag[1],
                "t" => {
                    let tag_val = tag[1].trim();
                    if !tag_val.is_empty() && tag_val.len() <= MAX_HASHTAG_LEN {
                        hashtags.push(tag_val.to_string());
                    }
                }
                _ => {}
            }
        }
        if hashtags.len() > MAX_HASHTAGS {
            break;
        }
    }
    if url.is_empty() {
        return None;
    }
    let thumbnail = sanitize_media_url(thumbnail);
    let audience = if audience.is_empty() {
        "public"
    } else {
        audience
    };
    let audience = if audience.len() > MAX_AUDIENCE_LEN {
        soshal_common_core::ui_safe::truncate_str(audience, MAX_AUDIENCE_LEN)
    } else {
        audience
    };
    let title = if title.len() > MAX_METADATA_LEN {
        soshal_common_core::ui_safe::truncate_str(title, MAX_METADATA_LEN)
    } else {
        title
    };
    let d_tag = if d_tag.len() > MAX_METADATA_LEN {
        soshal_common_core::ui_safe::truncate_str(d_tag, MAX_METADATA_LEN)
    } else {
        d_tag
    };
    let (blob_hash, media_size) = media_blob_from_tags(&ev.tags);
    Some(MusicloudEventOut {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        audio_url: url.to_string(),
        blob_hash,
        media_size,
        title: title.to_string(),
        thumbnail: thumbnail.to_string(),
        hashtags,
        d: d_tag.to_string(),
        audience: audience.to_string(),
        created_at: clamp_created_at(ev.created_at),
    })
}

/// Maps a kind-31022 musicloud event to wire JSON (webview format). The
/// typed struct carries the identical camelCase field set.
pub fn musicloud_from_event(ev: &NostrEvent) -> Option<serde_json::Value> {
    serde_json::to_value(musicloud_event_out(ev)?).ok()
}

/// Builds the kind-30085 custom profile content payload.
pub fn custom_profile_content(nodes: &serde_json::Value, theme_id: &str) -> Result<String, String> {
    if theme_id.len() > 64
        || (!theme_id.is_empty()
            && !theme_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    {
        return Err("invalid theme_id: must be 0-64 alphanumeric/hyphen/underscore chars".into());
    }
    let payload = serde_json::json!({ "themeId": theme_id, "nodes": nodes });
    serde_json::to_string(&payload).map_err(|e| format!("serialize: {}", e))
}

/// Builds the `a` tag coordinate for a musicloud track comment.
pub fn musicloud_comment_addr(track_kind: u16, track_pubkey: &str, track_d: &str) -> String {
    format!("{}:{}:{}", track_kind, track_pubkey, track_d)
}

/// Sorts webview JSON items by `createdAt` descending (newest first).
pub fn sort_by_created_desc(items: &mut [serde_json::Value]) {
    sort_desc_by(items, |v| v["createdAt"].as_u64().unwrap_or(0));
}

/// Sorts any item list by an extracted `u64` timestamp descending.
pub fn sort_desc_by<T>(items: &mut [T], key: impl Fn(&T) -> u64) {
    items.sort_by_key(|b| std::cmp::Reverse(key(b)));
}

/// Sorts typed `MiniEventOut` items by `created_at` descending (newest first).
pub fn sort_minis_desc(items: &mut [MiniEventOut]) {
    sort_desc_by(items, |m| m.created_at);
}

/// Sorts typed `MusicloudEventOut` items by `created_at` descending (newest first).
pub fn sort_musicloud_desc(items: &mut [MusicloudEventOut]) {
    sort_desc_by(items, |m| m.created_at);
}
