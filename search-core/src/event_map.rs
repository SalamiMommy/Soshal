use crate::MAX_CONTENT_LEN;
use crate::MAX_TAGS;
use serde::{Deserialize, Serialize};
use soshal_common_core::consts::{KIND_EVENT, KIND_LISTING};
use soshal_common_core::json_util::{json_in, json_out};

pub type SearchEventStub = soshal_nostr_core::models::NostrEvent;

/// A normalized search result returned to the UI.
#[derive(Serialize)]
pub struct SearchResultOut {
    #[serde(rename = "type")]
    pub result_type: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    #[serde(rename = "imageUrl", skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    pub pubkey: String,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
}

/// Input for event_to_search_result.
#[derive(Deserialize)]
pub struct EventToSearchResultInput {
    pub event: SearchEventStub,
    pub kind: u32,
}

use soshal_nostr_core::models::find_tag_values_map;

/// Maps a Nostr event to a search result based on its kind.
pub fn event_to_search_result(input: &EventToSearchResultInput) -> Option<SearchResultOut> {
    let ev = &input.event;
    let kind = input.kind;
    let result_type: String;
    let title: String;
    let mut subtitle = String::new();
    let mut image_url: Option<String> = None;
    match kind {
        0 => {
            result_type = "user".to_string();
            match serde_json::from_str::<serde_json::Value>(&ev.content) {
                Ok(meta) => {
                    let display_name = meta
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .or_else(|| meta.get("displayName").and_then(|v| v.as_str()))
                        .or_else(|| meta.get("name").and_then(|v| v.as_str()));
                    title = match display_name {
                        Some(n) if !n.is_empty() => soshal_common_core::format::truncate(n, 120),
                        _ => ev.pubkey.chars().take(8).collect(),
                    };
                    let raw_subtitle = meta.get("about").and_then(|v| v.as_str()).unwrap_or("");
                    subtitle = soshal_common_core::format::truncate(raw_subtitle, 200);
                    image_url = meta
                        .get("picture")
                        .and_then(|v| v.as_str())
                        .and_then(soshal_common_core::url::sanitize_link_url);
                }
                Err(_) => {
                    title = ev.pubkey.chars().take(8).collect();
                }
            }
        }
        1 => {
            result_type = "post".to_string();
            title = ev.content.chars().take(80).collect::<String>();
            subtitle = format!("{}...", ev.pubkey.chars().take(8).collect::<String>());
        }
        n if n == KIND_EVENT as u32 => {
            result_type = "event".to_string();
            let [d_tag, title_tag] = find_tag_values_map(&ev.tags, ["d", "title"]);
            let d_str = d_tag.unwrap_or("");
            let title_str = title_tag.unwrap_or("");
            let raw_title = if !title_str.is_empty() {
                title_str
            } else {
                d_str
            };
            title = soshal_common_core::format::truncate(raw_title, 120);
            subtitle = soshal_common_core::format::truncate(&ev.content, 80);
        }
        n if n == KIND_LISTING as u32 => {
            result_type = "listing".to_string();
            let [title_tag, price_tag, image_tag] =
                find_tag_values_map(&ev.tags, ["title", "price", "image"]);
            let title_str = title_tag.unwrap_or("");
            let price = price_tag.unwrap_or("");
            let raw_title = if !title_str.is_empty() {
                title_str
            } else {
                "Marketplace Listing"
            };
            title = soshal_common_core::format::truncate(raw_title, 120);
            subtitle = soshal_common_core::format::truncate(price, 80);
            if let Some(img) = image_tag {
                image_url = soshal_common_core::url::sanitize_link_url(img);
            }
        }
        _ => return None,
    }
    let created_at = if ev.created_at.is_finite() && ev.created_at >= 0.0 {
        ev.created_at
    } else {
        0.0
    };
    Some(SearchResultOut {
        result_type,
        id: ev.id.clone(),
        title,
        subtitle,
        image_url,
        pubkey: ev.pubkey.clone(),
        created_at,
    })
}

/// JSON-based public API: maps a Nostr event JSON to a search result JSON.
/// Returns `"null"` on error or unsupported kind.
pub fn event_to_search_result_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<EventToSearchResultInput>>(input_json, None) else {
        return "null".to_string();
    };
    if input.event.tags.len() > MAX_TAGS {
        return "null".to_string();
    }
    if input.event.content.len() > MAX_CONTENT_LEN {
        return "null".to_string();
    }
    json_out(&event_to_search_result(&input), "null")
}
