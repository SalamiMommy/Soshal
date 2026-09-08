use serde::Deserialize;

use soshal_common_core::json_util::{json_in_borrow, json_out};

pub use soshal_common_core::consts::MAX_TAGS;
pub const MAX_TAG_FIELDS: usize = 16;
pub const MAX_TAG_FIELD_LEN: usize = 1024;
pub const MAX_OUTPUT_IDS: usize = 5_000;

#[derive(Deserialize)]
struct DeletionEventInputBorrow<'a> {
    #[serde(borrow)]
    tags: Vec<Vec<&'a str>>,
}

use std::collections::HashSet;

pub fn extract_deletion_ids(tags: &[Vec<String>]) -> Vec<String> {
    let tag_count = tags.len().min(MAX_TAGS);
    let initial_cap = tag_count.min(MAX_OUTPUT_IDS).min(64);
    let mut ids: Vec<String> = Vec::with_capacity(initial_cap);
    let mut seen: HashSet<&str> = HashSet::with_capacity(initial_cap);
    for tag in tags.iter().take(tag_count) {
        if ids.len() >= MAX_OUTPUT_IDS {
            break;
        }
        if tag.is_empty() {
            continue;
        }
        if tag[0] != "e" {
            continue;
        }
        if tag.len() > MAX_TAG_FIELDS {
            continue;
        }
        if let Some(id) = tag.get(1) {
            if id.len() > MAX_TAG_FIELD_LEN {
                continue;
            }
            if seen.insert(id.as_str()) {
                ids.push(id.clone());
            }
        }
    }
    ids
}

pub fn extract_deletion_ids_json(input: &str) -> String {
    let Some(input) = json_in_borrow::<DeletionEventInputBorrow>(input) else {
        return "[]".to_string();
    };
    if input.tags.len() > MAX_TAGS {
        return "[]".to_string();
    }
    let initial_cap = input.tags.len().min(MAX_OUTPUT_IDS).min(64);
    let mut ids: Vec<&str> = Vec::with_capacity(initial_cap);
    let mut seen: HashSet<&str> = HashSet::with_capacity(initial_cap);
    for tag in input.tags.iter().take(MAX_TAGS) {
        if ids.len() >= MAX_OUTPUT_IDS {
            break;
        }
        if tag.is_empty() || tag[0] != "e" || tag.len() > MAX_TAG_FIELDS {
            continue;
        }
        if let Some(&id) = tag.get(1) {
            if id.len() <= MAX_TAG_FIELD_LEN && seen.insert(id) {
                ids.push(id);
            }
        }
    }
    json_out(&ids, "[]")
}
