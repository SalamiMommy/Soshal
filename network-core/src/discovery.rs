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
    let self_pubkey = input.self_pubkey.trim().to_ascii_lowercase();
    let self_contacts: HashSet<String> = input
        .self_contacts
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let user_info_map: HashMap<String, &AllUserInfo> = input
        .all_users
        .iter()
        .map(|u| (u.pubkey.trim().to_ascii_lowercase(), u))
        .collect();
    let mut candidate_display_pks: HashMap<String, &str> = HashMap::new();
    let mut candidates: HashMap<String, HashSet<String>> =
        HashMap::with_capacity(input.all_users.len().min(128));
    for user in &input.all_users {
        let user_pk = user.pubkey.trim().to_ascii_lowercase();
        if user_pk == self_pubkey || self_contacts.contains(&user_pk) {
            continue;
        }
        let mut mutuals = HashSet::new();
        for c in &user.contacts {
            let c_norm = c.trim().to_ascii_lowercase();
            if self_contacts.contains(&c_norm) {
                mutuals.insert(c_norm);
            }
        }
        if !mutuals.is_empty() {
            candidate_display_pks.insert(user_pk.clone(), user.pubkey.as_str());
            candidates.insert(user_pk, mutuals);
        }
    }
    for contact in &input.self_contacts {
        let contact_norm = contact.trim().to_ascii_lowercase();
        if let Some(user_info) = user_info_map.get(&contact_norm) {
            for their_contact in &user_info.contacts {
                let their_contact_norm = their_contact.trim().to_ascii_lowercase();
                if their_contact_norm == self_pubkey || self_contacts.contains(&their_contact_norm)
                {
                    continue;
                }
                candidate_display_pks
                    .entry(their_contact_norm.clone())
                    .or_insert(their_contact.as_str());
                candidates
                    .entry(their_contact_norm)
                    .or_default()
                    .insert(contact_norm.clone());
            }
        }
    }
    let candidates_len = candidates.len();
    let mut suggestions: Vec<SuggestionOut> = Vec::with_capacity(candidates_len);
    for (pubkey_norm, mutuals) in candidates {
        let mutual_count = mutuals.len();
        let distance = user_info_map
            .get(&pubkey_norm)
            .map(|u| u.wot_distance)
            .unwrap_or(2);
        let reason = if distance == 2 {
            format!("Connected via {} friend(s)", mutual_count)
        } else {
            format!("Shared by {} mutual contact(s)", mutual_count)
        };
        let display_pk = candidate_display_pks
            .get(&pubkey_norm)
            .copied()
            .unwrap_or(pubkey_norm.as_str());
        suggestions.push(SuggestionOut {
            pubkey: display_pk.to_string(),
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
    if input.len() > 16 * 1024 * 1024 {
        return "[]".to_string();
    }
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
    let self_pubkey = input.self_pubkey.trim().to_ascii_lowercase();
    let self_friends_set: HashSet<String> = input
        .self_friends
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut candidate_display_pks: HashMap<String, String> = HashMap::new();
    let mut candidate_seeds: HashMap<String, HashSet<String>> = HashMap::new();
    let mut candidate_contracts: HashMap<String, HashSet<String>> = HashMap::new();
    let mut candidate_gateways: HashMap<String, Option<String>> = HashMap::new();

    for manifest in &input.manifests {
        let reporter_orig = manifest.peer_pubkey.trim();
        let reporter = reporter_orig.to_ascii_lowercase();
        if reporter == self_pubkey {
            continue;
        }

        let is_direct_friend = self_friends_set.contains(&reporter);

        for target in &manifest.friends {
            let target_trim = target.trim();
            let target_norm = target_trim.to_ascii_lowercase();
            if target_norm == self_pubkey || self_friends_set.contains(&target_norm) {
                continue;
            }

            candidate_display_pks
                .entry(target_norm.clone())
                .or_insert_with(|| target_trim.to_string());

            if is_direct_friend {
                candidate_seeds
                    .entry(target_norm.clone())
                    .or_default()
                    .insert(reporter_orig.to_string());
            }

            if let Some(gw) = &manifest.gateway_url {
                if !gw.is_empty() {
                    candidate_gateways.insert(target_norm.clone(), Some(gw.clone()));
                }
            }

            let contracts_entry = candidate_contracts.entry(target_norm).or_default();
            for contract in &manifest.contracts {
                contracts_entry.insert(contract.clone());
            }
        }
    }

    let mut suggestions: Vec<FreenetSwarmSuggestion> = candidate_seeds
        .into_iter()
        .map(|(pubkey_norm, seeds)| {
            let mutual_count = seeds.len();
            let mut seed_friends: Vec<String> = seeds.into_iter().collect();
            seed_friends.sort();
            let contracts: Vec<String> = candidate_contracts
                .remove(&pubkey_norm)
                .map(|s| s.into_iter().collect())
                .unwrap_or_default();
            let gateway_url = candidate_gateways.remove(&pubkey_norm).flatten();
            let reason = format!(
                "Discovered via Freenet friend swarm ({} mutual seed peer(s))",
                mutual_count
            );

            let pubkey = candidate_display_pks
                .remove(&pubkey_norm)
                .unwrap_or(pubkey_norm);

            FreenetSwarmSuggestion {
                pubkey,
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
    if input.len() > 16 * 1024 * 1024 {
        return "[]".to_string();
    }
    let Some(input) = json_in::<Option<FreenetSwarmDiscoveryInput>>(input, None) else {
        return "[]".to_string();
    };
    json_out(&discover_freenet_swarm(input), "[]")
}
