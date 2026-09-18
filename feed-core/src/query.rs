//! Feed query helpers: event-to-JSON mapping, feed buffer slicing, and
//! reaction/reply-map aggregation.

use std::collections::HashMap;

use soshal_nostr_core::models::NostrEvent;

/// True for local CAS blob references (`blob://<64hex>`, `n<64hex>`, or a
/// bare 64-hex hash) that renderers resolve through the chunk store instead
/// of fetching over HTTP.
fn is_blob_ref(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("blob://") {
        return is_hex64(rest);
    }
    if let Some(rest) = s.strip_prefix('n') {
        return is_hex64(rest);
    }
    is_hex64(s)
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Maps a timeline event to `{id, pubkey, content, created_at}`.
pub fn timeline_entry_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}

/// Maps an event to `{id, pubkey, content, created_at, tags}`.
pub fn event_with_tags_from_event(ev: &NostrEvent) -> serde_json::Value {
    let mut v = timeline_entry_from_event(ev);
    v["tags"] = serde_json::json!(ev.tags);
    v
}

/// Media-JSON decode cache. Post rows are immutable, so the parse result for
/// a given `tags_json` never goes stale; the same posts are re-fetched across
/// feed pages/threads/refreshes, and this avoids re-parsing + re-serializing
/// per row per fetch.
struct MediaJsonCache {
    map: std::collections::HashMap<std::sync::Arc<str>, Option<String>>,
    queue: std::collections::VecDeque<std::sync::Arc<str>>,
}

impl Default for MediaJsonCache {
    fn default() -> Self {
        Self {
            map: std::collections::HashMap::with_capacity(MEDIA_JSON_CACHE_CAP),
            queue: std::collections::VecDeque::with_capacity(MEDIA_JSON_CACHE_CAP),
        }
    }
}

static MEDIA_JSON_CACHE: std::sync::OnceLock<std::sync::RwLock<MediaJsonCache>> =
    std::sync::OnceLock::new();

const MEDIA_JSON_CACHE_CAP: usize = 1024;

fn cached_media_json(tags_json: &str) -> Option<Option<String>> {
    let cache = MEDIA_JSON_CACHE.get_or_init(Default::default);
    cache.read().ok()?.map.get(tags_json).cloned()
}

fn store_media_json(tags_json: &str, parsed: &Option<String>) {
    if let Ok(mut cache) = MEDIA_JSON_CACHE.get_or_init(Default::default).write() {
        if cache.map.contains_key(tags_json) {
            return;
        }
        if cache.map.len() >= MEDIA_JSON_CACHE_CAP {
            if let Some(oldest) = cache.queue.pop_front() {
                cache.map.remove(&oldest);
            }
        }
        let key: std::sync::Arc<str> = std::sync::Arc::from(tags_json);
        cache.map.insert(key.clone(), parsed.clone());
        cache.queue.push_back(key);
    }
}

/// Extracts a `["media", type, url, blob_hash, size]` tag from a post's
/// `tags_json` and renders it as `{"type","url","blob_hash","size"}`.
/// The media tag is the LAN blob-sharing wire format: `url` is a
/// `blob://<hash>` placeholder (peers crawl by hash), `blob_hash` is the
/// BLAKE3 manifest hash. Malformed tags yield `None` (hostile-input safe).
pub fn media_json_from_tags(tags_json: &str) -> Option<String> {
    if let Some(cached) = cached_media_json(tags_json) {
        return cached;
    }
    let parsed = parse_media_json(tags_json);
    store_media_json(tags_json, &parsed);
    parsed
}

fn parse_media_json(tags_json: &str) -> Option<String> {
    let tags: Vec<Vec<std::borrow::Cow<'_, str>>> = serde_json::from_str(tags_json).ok()?;
    for tag in tags {
        if tag.first().map(|s| s.as_ref()) != Some("media") {
            continue;
        }
        if tag.len() < 5 {
            continue;
        }
        let media_type = &tag[1];
        if !matches!(media_type.as_ref(), "image" | "video" | "audio") {
            continue;
        }
        let url = &tag[2];
        let url_str = if url.is_empty()
            || is_blob_ref(url)
            || soshal_common_core::url::is_valid_media_url(url)
        {
            url.as_ref()
        } else {
            ""
        };
        let blob_hash = &tag[3];
        if blob_hash.len() != 64 || !blob_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let size: u64 = tag[4].parse().unwrap_or(0);
        return Some(
            serde_json::json!({
                "type": media_type,
                "url": url_str,
                "blob_hash": blob_hash,
                "size": size,
            })
            .to_string(),
        );
    }
    None
}

/// Splits the feed buffer at `since_id` (exclusive) into pending events and
/// the pre-split total. Trims the remaining buffer to `max` items.
pub fn split_feed_buffer(
    events: &mut std::collections::VecDeque<serde_json::Value>,
    since_id: &str,
    max: usize,
) -> (Vec<serde_json::Value>, usize) {
    let total = events.len();
    let start = if since_id.is_empty() {
        0
    } else {
        match events
            .iter()
            .position(|e| e["id"].as_str() == Some(since_id))
        {
            Some(i) => i + 1,
            None => return (vec![], total),
        }
    };
    let pending = if start >= total {
        vec![]
    } else {
        events.split_off(start).into_iter().collect()
    };
    while events.len() > max {
        events.pop_front();
    }
    (pending, total)
}

#[derive(Default)]
struct ReactionCounts {
    count: u64,
    emojis: HashMap<String, u64>,
}

/// Aggregates kind-7 reaction events into a per-target map
/// `{"count": n, "emojis": {emoji: n}}`. Reaction target is the `e` tag.
pub fn aggregate_reaction_map(events: &[NostrEvent]) -> serde_json::Map<String, serde_json::Value> {
    let mut counts_map: HashMap<String, ReactionCounts> = HashMap::new();
    let mut seen_targets = std::collections::HashSet::new();
    for ev in events {
        seen_targets.clear();
        for tag in &ev.tags {
            if tag.first().map(|s| s.as_str()) == Some("e") {
                let Some(target) = tag.get(1) else {
                    continue;
                };
                if target.is_empty() || target.len() > 64 || !seen_targets.insert(target.as_str()) {
                    continue;
                }
                let entry = counts_map.entry(target.clone()).or_default();
                entry.count += 1;
                let emoji = if ev.content.is_empty() {
                    "+"
                } else if ev.content.len() > 32 {
                    let bound = ev.content.floor_char_boundary(32);
                    if bound == 0 {
                        "+"
                    } else {
                        &ev.content[..bound]
                    }
                } else {
                    ev.content.as_str()
                };
                *entry.emojis.entry(emoji.to_string()).or_default() += 1;
            }
        }
    }
    let mut map = serde_json::Map::with_capacity(counts_map.len());
    for (target, counts) in counts_map {
        let mut emojis_map = serde_json::Map::with_capacity(counts.emojis.len());
        for (emoji, cnt) in counts.emojis {
            emojis_map.insert(emoji, serde_json::json!(cnt));
        }
        map.insert(
            target,
            serde_json::json!({
                "count": counts.count,
                "emojis": emojis_map
            }),
        );
    }
    map
}

/// Counts kind-1 reply events per target post, keyed by the first `e` tag
/// (the root reference). Each reply counts once against its root thread.
pub fn aggregate_reply_map(events: &[NostrEvent]) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for ev in events {
        if ev.kind != soshal_common_core::consts::KIND_TEXT_NOTE as u32 {
            continue;
        }
        if let Some(target) = ev.find_tag("e") {
            *map.entry(target.to_string()).or_insert(0) += 1;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_url_sanitized_at_parse() {
        let bad = r#"[["media","image","http://192.168.1.1/x.png","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","123"]]"#;
        let parsed = parse_media_json(bad).unwrap();
        let v: serde_json::Value = serde_json::from_str(&parsed).unwrap();
        assert_eq!(v["url"], "", "private-IP media url must be blanked");

        let good = r#"[["media","image","https://example.com/x.png","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","123"]]"#;
        let parsed = parse_media_json(good).unwrap();
        let v: serde_json::Value = serde_json::from_str(&parsed).unwrap();
        assert_eq!(v["url"], "https://example.com/x.png");

        let blob = r#"[["media","image","blob://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","123"]]"#;
        let parsed = parse_media_json(blob).unwrap();
        let v: serde_json::Value = serde_json::from_str(&parsed).unwrap();
        assert_eq!(
            v["url"], "blob://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "blob refs must survive"
        );
    }

    #[test]
    fn blob_ref_detection() {
        let h = "a".repeat(64);
        assert!(is_blob_ref(&format!("blob://{h}")));
        assert!(is_blob_ref(&format!("n{h}")));
        assert!(is_blob_ref(&h));
        assert!(!is_blob_ref("https://example.com/a.png"));
        assert!(!is_blob_ref(&"a".repeat(63)));
    }

    #[test]
    fn skips_invalid_media_tag_and_finds_valid_tag() {
        let tags = r#"[
            ["media", "unsupported"],
            ["media", "document", "https://example.com/doc.pdf", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "100"],
            ["media", "image", "https://example.com/pic.png", "not-hex", "200"],
            ["media", "image", "https://example.com/good.png", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "300"]
        ]"#;
        let parsed = parse_media_json(tags).expect("should find valid tag");
        let v: serde_json::Value = serde_json::from_str(&parsed).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["url"], "https://example.com/good.png");
        assert_eq!(v["size"], 300);
    }
}
