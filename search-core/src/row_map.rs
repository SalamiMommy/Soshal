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
                    .map(|s| s.to_string());
                meta.get("about")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            }
            Err(_) => String::new(),
        }
    } else {
        row.content.clone()
    };
    MappedSearchRow {
        result_type: row.row_type.clone(),
        id: row.id.clone(),
        title: row.title.clone(),
        subtitle,
        image_url,
        pubkey: row.pubkey.clone(),
        created_at: row.created_at,
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
