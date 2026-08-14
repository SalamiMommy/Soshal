use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use soshal_common_core::json_util::{json_in, json_out};

const MAX_USERS: usize = 100_000;
const MAX_POSTS: usize = 10_000;
const MAX_TAGS: usize = 100;
const MAX_INTEREST_LEN: usize = 64;

#[derive(Deserialize)]
struct DiscoverByInterestInput {
    events: Vec<DiscoveryEventInput>,
    tags: Vec<String>,
    self_pubkey: String,
    self_contacts: Vec<String>,
    limit: usize,
}

#[derive(Deserialize)]
struct DiscoveryEventInput {
    pubkey: String,
    content: String,
}

#[derive(Serialize)]
struct DiscoverResultOut {
    pubkey: String,
    reason: String,
    #[serde(rename = "mutualCount")]
    mutual_count: u32,
    distance: u32,
}

fn discover_by_interest(input: DiscoverByInterestInput) -> Vec<DiscoverResultOut> {
    if input.events.len() > MAX_POSTS || input.tags.len() > MAX_TAGS || input.limit > MAX_USERS {
        return Vec::new();
    }
    if input.events.len() * input.tags.len() > 1_000_000 {
        return Vec::new();
    }
    let self_contacts_set: HashSet<&str> = input.self_contacts.iter().map(|s| s.as_str()).collect();
    let lower_tags: Vec<String> = input
        .tags
        .iter()
        .filter(|t| t.len() <= MAX_INTEREST_LEN)
        .map(|t| t.to_lowercase())
        .collect();
    let mut results = Vec::new();
    for event in &input.events {
        if event.content.len() > 64 * 1024 {
            continue;
        }
        if event.pubkey == input.self_pubkey || self_contacts_set.contains(event.pubkey.as_str()) {
            continue;
        }
        let matched_tags: Vec<&str> = lower_tags
            .iter()
            .filter(|t| {
                let needle = t.as_bytes();
                !needle.is_empty()
                    && event.content.len() >= needle.len()
                    && event
                        .content
                        .as_bytes()
                        .windows(needle.len())
                        .any(|w| w.eq_ignore_ascii_case(needle))
            })
            .map(|s| s.as_str())
            .collect();
        if !matched_tags.is_empty() {
            results.push(DiscoverResultOut {
                pubkey: event.pubkey.clone(),
                reason: format!("Shared interests: {}", matched_tags.join(", ")),
                mutual_count: 0,
                distance: 2,
            });
        }
    }
    results.truncate(input.limit);
    results
}

pub fn discover_by_interest_json(input: &str) -> String {
    let Some(input) = json_in::<Option<DiscoverByInterestInput>>(input, None) else {
        return "[]".to_string();
    };
    let out = discover_by_interest(input);
    json_out(&out, "[]")
}
