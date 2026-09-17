use crate::event_map::SearchResultOut;
use crate::MAX_CONTENT_LEN;
use serde::Deserialize;
use soshal_common_core::json_util::{json_in, json_out};

/// A raw FTS5 row as read from the database.
#[derive(Deserialize)]
pub struct SearchRowInput {
    pub id: String,
    #[serde(rename = "type")]
    pub row_type: String,
    pub title: String,
    pub content: String,
    pub pubkey: String,
    pub created_at: f64,
}

/// A normalized search row returned to the UI (same shape as `SearchResultOut`).
pub type MappedSearchRow = SearchResultOut;

/// Maps an FTS5 row to a normalized search result.
pub fn map_search_row(row: &SearchRowInput) -> MappedSearchRow {
    let mut image_url: Option<String> = None;
    let subtitle = if row.row_type == "user" {
        match serde_json::from_str::<serde_json::Value>(&row.content) {
            Ok(meta) => {
                image_url = meta
                    .get("picture")
                    .and_then(|v| v.as_str())
                    .and_then(soshal_common_core::url::sanitize_link_url);
                let about = meta.get("about").and_then(|v| v.as_str()).unwrap_or("");
                soshal_common_core::format::truncate(about, 200)
            }
            Err(_) => String::new(),
        }
    } else {
        soshal_common_core::format::truncate(&row.content, 200)
    };
    let title = soshal_common_core::format::truncate(&row.title, 120);
    let created_at = if row.created_at.is_finite() && row.created_at >= 0.0 {
        row.created_at
    } else {
        0.0
    };
    MappedSearchRow {
        result_type: row.row_type.clone(),
        id: row.id.clone(),
        title,
        subtitle,
        image_url,
        pubkey: row.pubkey.clone(),
        created_at,
    }
}

/// JSON-based public API: maps an FTS5 row JSON to a normalized search result JSON.
/// Returns "null" on error.
pub fn map_search_row_json(input_json: &str) -> String {
    let Some(row) = json_in::<Option<SearchRowInput>>(input_json, None) else {
        return "null".to_string();
    };
    if row.content.len() > MAX_CONTENT_LEN || row.title.len() > MAX_CONTENT_LEN {
        return "null".to_string();
    }
    json_out(&map_search_row(&row), "null")
}
