use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;

use soshal_common_core::json_util::{json_in_borrow, json_out};

const MAX_USERS: usize = 100_000;
const MAX_POSTS: usize = 10_000;
const MAX_TAGS: usize = 100;
const MAX_INTEREST_LEN: usize = 64;

#[derive(Deserialize)]
struct DiscoverByInterestInput<'a> {
    #[serde(borrow)]
    events: Vec<DiscoveryEventInput<'a>>,
    #[serde(borrow)]
    tags: Vec<&'a str>,
    #[serde(borrow)]
    self_pubkey: &'a str,
    #[serde(borrow)]
    self_contacts: Vec<&'a str>,
    limit: usize,
}

#[derive(Deserialize)]
struct DiscoveryEventInput<'a> {
    #[serde(borrow)]
    pubkey: &'a str,
    #[serde(borrow)]
    content: &'a str,
}

#[derive(Serialize)]
struct DiscoverResultOut<'a> {
    pubkey: &'a str,
    reason: String,
    #[serde(rename = "mutualCount")]
    mutual_count: u32,
    distance: u32,
}

fn discover_by_interest<'a>(input: DiscoverByInterestInput<'a>) -> Vec<DiscoverResultOut<'a>> {
    if input.events.len() > MAX_POSTS || input.tags.len() > MAX_TAGS || input.limit > MAX_USERS {
        return Vec::new();
    }
    if input.events.len() * input.tags.len() > 1_000_000 {
        return Vec::new();
    }
    let self_contacts_set: HashSet<&str> = input.self_contacts.iter().copied().collect();
    let lower_tags: Vec<String> = input
        .tags
        .iter()
        .filter(|t| !t.is_empty() && t.len() <= MAX_INTEREST_LEN)
        .map(|t| t.to_lowercase())
        .collect();
    if lower_tags.is_empty() {
        return Vec::new();
    }
    // One Aho-Corasick pass per event instead of a windows() substring scan
    // per tag (O(content x tags) worst case).
    let matcher =
        aho_corasick::AhoCorasick::new(&lower_tags).expect("empty patterns are pre-filtered");
    let mut results = Vec::with_capacity(input.events.len().min(input.limit));
    for event in &input.events {
        if results.len() >= input.limit {
            break;
        }
        if event.content.len() > 64 * 1024 {
            continue;
        }
        if event.pubkey == input.self_pubkey || self_contacts_set.contains(event.pubkey) {
            continue;
        }
        let content_to_match: Cow<str> = if event.content.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(event.content.to_lowercase())
        } else {
            Cow::Borrowed(event.content)
        };
        let mut matched: Vec<&str> = Vec::new();
        let mut seen_mask = [0u64; 2];
        for m in matcher.find_iter(&*content_to_match) {
            let idx = m.pattern().as_usize();
            let word = idx / 64;
            let bit = 1u64 << (idx % 64);
            if word < 2 && (seen_mask[word] & bit) == 0 {
                seen_mask[word] |= bit;
                matched.push(lower_tags[idx].as_str());
            }
        }
        if !matched.is_empty() {
            results.push(DiscoverResultOut {
                pubkey: event.pubkey,
                reason: format!("Shared interests: {}", matched.join(", ")),
                mutual_count: 0,
                distance: 2,
            });
        }
    }
    results
}

pub fn discover_by_interest_json(input: &str) -> String {
    let Some(input) = json_in_borrow::<DiscoverByInterestInput>(input) else {
        return "[]".to_string();
    };
    let out = discover_by_interest(input);
    json_out(&out, "[]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_by_interest_empty_tags_does_not_panic() {
        let input = DiscoverByInterestInput {
            events: vec![DiscoveryEventInput {
                pubkey: "pk1",
                content: "hello world",
            }],
            tags: vec![],
            self_pubkey: "self",
            self_contacts: vec![],
            limit: 10,
        };
        let res = discover_by_interest(input);
        assert!(res.is_empty());

        let input_blank = DiscoverByInterestInput {
            events: vec![DiscoveryEventInput {
                pubkey: "pk1",
                content: "hello world",
            }],
            tags: vec!["", "   "],
            self_pubkey: "self",
            self_contacts: vec![],
            limit: 10,
        };
        let res_blank = discover_by_interest(input_blank);
        assert!(res_blank.is_empty());
    }
}
