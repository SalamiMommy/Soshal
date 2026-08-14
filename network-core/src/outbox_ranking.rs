use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RelayRank {
    pub relay_url: String,
    pub author_count: usize,
    pub covered_pubkeys: Vec<String>,
}

pub fn rank_outbox_relays(author_relays: &[(String, Vec<String>)], limit: usize) -> Vec<RelayRank> {
    if author_relays.is_empty() || limit == 0 {
        return vec![];
    }
    let total_relays: usize = author_relays.iter().map(|(_, r)| r.len()).sum();
    let mut relay_authors: HashMap<String, HashSet<&str>> =
        HashMap::with_capacity(total_relays.min(256));
    for (pubkey, relays) in author_relays {
        for relay in relays {
            let trimmed = relay.trim();
            if let Some(authors) = relay_authors.get_mut(trimmed) {
                authors.insert(pubkey.as_str());
            } else {
                let normalized = if trimmed.chars().any(|c| c.is_uppercase()) {
                    trimmed.to_lowercase()
                } else if trimmed.len() != relay.len() {
                    trimmed.to_string()
                } else {
                    relay.clone()
                };
                relay_authors
                    .entry(normalized)
                    .or_default()
                    .insert(pubkey.as_str());
            }
        }
    }
    let mut ranked: Vec<RelayRank> = Vec::with_capacity(relay_authors.len());
    for (relay_url, authors) in relay_authors {
        let count = authors.len();
        let covered: Vec<String> = authors.into_iter().map(|s| s.to_string()).collect();
        ranked.push(RelayRank {
            relay_url,
            author_count: count,
            covered_pubkeys: covered,
        });
    }
    if ranked.len() > limit {
        ranked.select_nth_unstable_by(limit, |a, b| b.author_count.cmp(&a.author_count));
        ranked.truncate(limit);
        ranked.sort_by_key(|b| std::cmp::Reverse(b.author_count));
    } else {
        ranked.sort_by_key(|b| std::cmp::Reverse(b.author_count));
    }
    ranked
}
