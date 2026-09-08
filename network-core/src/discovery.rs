use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use std::collections::{HashMap, HashSet};

const MAX_USERS: usize = 100_000;

#[derive(Deserialize, Clone)]
pub struct AllUserInfo {
    pub pubkey: String,
    pub contacts: Vec<String>,
    pub wot_distance: u32,
}

#[derive(Deserialize)]
pub struct SuggestMutualFriendsInput {
    pub self_pubkey: String,
    pub self_contacts: Vec<String>,
    pub all_users: Vec<AllUserInfo>,
    pub limit: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SuggestionOut {
    pub pubkey: String,
    pub reason: String,
    #[serde(rename = "mutualCount")]
    pub mutual_count: usize,
    pub distance: u32,
}

pub fn suggest_mutual_friends(input: SuggestMutualFriendsInput) -> Vec<SuggestionOut> {
    if input.all_users.len() > MAX_USERS || input.limit > MAX_USERS {
        return Vec::new();
    }
    let self_pubkey = &input.self_pubkey;
    let self_contacts: HashSet<&str> = input.self_contacts.iter().map(|s| s.as_str()).collect();
    let user_info_map: HashMap<&str, &AllUserInfo> = input
        .all_users
        .iter()
        .map(|u| (u.pubkey.as_str(), u))
        .collect();
    let mut candidates: HashMap<&str, HashSet<&str>> =
        HashMap::with_capacity(input.all_users.len().min(128));
    for user in &input.all_users {
        if &user.pubkey == self_pubkey || self_contacts.contains(user.pubkey.as_str()) {
            continue;
        }
        let mut mutuals = HashSet::new();
        for c in &user.contacts {
            if self_contacts.contains(c.as_str()) {
                mutuals.insert(c.as_str());
            }
        }
        if !mutuals.is_empty() {
            candidates.insert(user.pubkey.as_str(), mutuals);
        }
    }
    for contact in &input.self_contacts {
        if let Some(user_info) = user_info_map.get(contact.as_str()) {
            for their_contact in &user_info.contacts {
                if their_contact == self_pubkey || self_contacts.contains(their_contact.as_str()) {
                    continue;
                }
                candidates
                    .entry(their_contact.as_str())
                    .or_default()
                    .insert(contact.as_str());
            }
        }
    }
    let candidates_len = candidates.len();
    let mut suggestions: Vec<SuggestionOut> = Vec::with_capacity(candidates_len);
    for (pubkey, mutuals) in candidates {
        let mutual_count = mutuals.len();
        let distance = user_info_map
            .get(pubkey)
            .map(|u| u.wot_distance)
            .unwrap_or(2);
        let reason = if distance == 2 {
            format!("Connected via {} friend(s)", mutual_count)
        } else {
            format!("Shared by {} mutual contact(s)", mutual_count)
        };
        suggestions.push(SuggestionOut {
            pubkey: pubkey.to_string(),
            reason,
            mutual_count,
            distance,
        });
    }
    suggestions.sort_by(|a, b| {
        let cmp = b.mutual_count.cmp(&a.mutual_count);
        if cmp == std::cmp::Ordering::Equal {
            a.pubkey.cmp(&b.pubkey)
        } else {
            cmp
        }
    });
    if suggestions.len() > input.limit {
        suggestions.truncate(input.limit);
    }
    suggestions
}

pub fn suggest_mutual_friends_json(input: &str) -> String {
    let Some(input) = json_in::<Option<SuggestMutualFriendsInput>>(input, None) else {
        return "[]".to_string();
    };
    json_out(&suggest_mutual_friends(input), "[]")
}

// ---------------------------------------------------------------------------
// Freenet Torrent Swarm (PEX) Friend Discovery
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FreenetSwarmManifest {
    pub peer_pubkey: String,
    pub friends: Vec<String>,
    pub contracts: Vec<String>,
    pub gateway_url: Option<String>,
}

#[derive(Deserialize)]
pub struct FreenetSwarmDiscoveryInput {
    pub self_pubkey: String,
    pub self_friends: Vec<String>,
    pub manifests: Vec<FreenetSwarmManifest>,
    pub limit: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FreenetSwarmSuggestion {
    pub pubkey: String,
    pub seed_friends: Vec<String>,
    pub mutual_count: usize,
    pub contracts: Vec<String>,
    pub gateway_url: Option<String>,
    pub reason: String,
}

pub fn discover_freenet_swarm(input: FreenetSwarmDiscoveryInput) -> Vec<FreenetSwarmSuggestion> {
    if input.manifests.len() > MAX_USERS || input.limit > MAX_USERS {
        return Vec::new();
    }
    let self_pubkey = &input.self_pubkey;
    let self_friends_set: HashSet<&str> = input.self_friends.iter().map(|s| s.as_str()).collect();

    let mut candidate_seeds: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut candidate_contracts: HashMap<&str, HashSet<String>> = HashMap::new();
    let mut candidate_gateways: HashMap<&str, Option<String>> = HashMap::new();

    for manifest in &input.manifests {
        let reporter = manifest.peer_pubkey.as_str();
        if reporter == self_pubkey {
            continue;
        }

        let is_direct_friend = self_friends_set.contains(reporter);

        for target in &manifest.friends {
            let target_str = target.as_str();
            if target_str == self_pubkey || self_friends_set.contains(target_str) {
                continue;
            }

            if is_direct_friend {
                candidate_seeds
                    .entry(target_str)
                    .or_default()
                    .insert(reporter);
            }

            if let Some(gw) = &manifest.gateway_url {
                if !gw.is_empty() {
                    candidate_gateways.insert(target_str, Some(gw.clone()));
                }
            }

            let contracts_entry = candidate_contracts.entry(target_str).or_default();
            for contract in &manifest.contracts {
                contracts_entry.insert(contract.clone());
            }
        }
    }

    let mut suggestions: Vec<FreenetSwarmSuggestion> = candidate_seeds
        .into_iter()
        .map(|(pubkey, seeds)| {
            let mutual_count = seeds.len();
            let mut seed_friends: Vec<String> = seeds.into_iter().map(|s| s.to_string()).collect();
            seed_friends.sort();
            let contracts: Vec<String> = candidate_contracts
                .remove(pubkey)
                .map(|s| s.into_iter().collect())
                .unwrap_or_default();
            let gateway_url = candidate_gateways.remove(pubkey).flatten();
            let reason = format!(
                "Discovered via Freenet friend swarm ({} mutual seed peer(s))",
                mutual_count
            );

            FreenetSwarmSuggestion {
                pubkey: pubkey.to_string(),
                seed_friends,
                mutual_count,
                contracts,
                gateway_url,
                reason,
            }
        })
        .collect();

    suggestions.sort_by(|a, b| {
        let cmp = b.mutual_count.cmp(&a.mutual_count);
        if cmp == std::cmp::Ordering::Equal {
            a.pubkey.cmp(&b.pubkey)
        } else {
            cmp
        }
    });

    if suggestions.len() > input.limit {
        suggestions.truncate(input.limit);
    }

    suggestions
}

pub fn discover_freenet_swarm_json(input: &str) -> String {
    let Some(input) = json_in::<Option<FreenetSwarmDiscoveryInput>>(input, None) else {
        return "[]".to_string();
    };
    json_out(&discover_freenet_swarm(input), "[]")
}
