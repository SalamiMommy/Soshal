use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

type WotPeersCache = HashMap<(String, u32), HashMap<u32, Vec<String>>>;

static WOT_PEERS_CACHE: OnceLock<Mutex<WotPeersCache>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrustScore {
    pub score: f64,
    pub distance: u32,
    pub mutual_count: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct WotUser {
    pub pubkey: String,
    pub contacts: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WotUpdate {
    pub pubkey: String,
    pub distance: u32,
    pub trust_score: f64,
    pub introduced_by: Option<Vec<String>>,
}

pub fn count_mutual(contacts_a: &[String], contacts_b: &[String]) -> usize {
    if contacts_b.len() <= 16 {
        return contacts_a
            .iter()
            .filter(|c| contacts_b.iter().any(|b| b == *c))
            .count();
    }
    let mut set_b = HashSet::with_capacity(contacts_b.len());
    for s in contacts_b {
        set_b.insert(s.as_str());
    }
    contacts_a
        .iter()
        .filter(|c| set_b.contains(c.as_str()))
        .count()
}

pub fn compute_distance(
    user_pubkey: &str,
    target_pubkey: &str,
    direct_follows: &[String],
    mutual_count: usize,
) -> u32 {
    if user_pubkey == target_pubkey {
        return 0;
    }
    if direct_follows.iter().any(|f| f == target_pubkey) {
        return 1;
    }
    if mutual_count > 0 {
        return 2;
    }
    3
}

pub fn calculate_trust_score(distance: u32, mutual_count: usize, introducer_count: usize) -> f64 {
    if distance == 0 {
        return 1.0;
    }
    let score = match distance {
        1 => {
            0.6 + (mutual_count as f64 * 0.05).min(0.3) + (introducer_count as f64 * 0.05).min(0.1)
        }
        2 => 0.3 + (mutual_count as f64 * 0.03).min(0.2),
        _ => 0.1,
    };
    score.clamp(0.0, 1.0)
}

pub fn compute_trust_score(
    user_pubkey: &str,
    target_pubkey: &str,
    user_contacts: &[String],
    target_contacts: &[String],
) -> TrustScore {
    let mutual = count_mutual(user_contacts, target_contacts);
    let distance = compute_distance(user_pubkey, target_pubkey, user_contacts, mutual);
    let score = calculate_trust_score(distance, mutual, 1);
    TrustScore {
        score,
        distance,
        mutual_count: mutual,
    }
}

pub fn recalculate_wot(self_pubkey: &str, users: &[WotUser]) -> Vec<WotUpdate> {
    const MAX_CONTACTS: usize = 5_000;
    const MAX_EDGES: usize = 500_000;
    const MAX_INTRODUCERS: usize = 64;

    let mut contact_map: HashMap<&str, &[String]> = HashMap::with_capacity(users.len());
    let mut total_edges = 0usize;

    for user in users {
        if total_edges >= MAX_EDGES {
            contact_map.entry(&user.pubkey).or_insert(&[]);
            continue;
        }
        let slice = if user.contacts.len() > MAX_CONTACTS {
            &user.contacts[..MAX_CONTACTS]
        } else {
            &user.contacts
        };
        total_edges += slice.len();
        contact_map.insert(&user.pubkey, slice);
    }

    let self_contacts: HashSet<&str> = contact_map
        .get(self_pubkey)
        .map(|c| c.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    let mut distances: HashMap<&str, u32> = HashMap::with_capacity(users.len());
    let mut introduced_by: HashMap<&str, Vec<&str>> = HashMap::with_capacity(users.len());
    let mut queue: VecDeque<(&str, u32)> = VecDeque::with_capacity(users.len().min(10_000));

    distances.insert(self_pubkey, 0);
    introduced_by.insert(self_pubkey, Vec::new());
    queue.push_back((self_pubkey, 0));

    let max_processed = users.len().min(10_000);
    let mut processed = 0usize;

    while let Some((current, current_dist)) = queue.pop_front() {
        processed += 1;
        if processed > max_processed {
            break;
        }
        if let Some(contacts) = contact_map.get(current) {
            let new_dist = current_dist.saturating_add(1).min(3);
            for contact in *contacts {
                let contact_str = contact.as_str();
                if let Some(&existing) = distances.get(contact_str) {
                    if new_dist > existing {
                        continue;
                    }
                } else {
                    distances.insert(contact_str, new_dist);
                    if new_dist < 2 {
                        queue.push_back((contact_str, new_dist));
                    }
                }
                let introducers = introduced_by.entry(contact_str).or_default();
                if introducers.len() < MAX_INTRODUCERS
                    && (current != self_pubkey || self_contacts.contains(contact_str))
                    && !introducers.contains(&current)
                {
                    introducers.push(current);
                }
            }
        }
    }

    users
        .iter()
        .map(|user| {
            let distance = *distances.get(user.pubkey.as_str()).unwrap_or(&3);
            let mutual_count = user
                .contacts
                .iter()
                .filter(|c| self_contacts.contains(c.as_str()))
                .count();
            let introducer_count = introduced_by
                .get(user.pubkey.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            let trust_score = calculate_trust_score(distance, mutual_count, introducer_count);
            WotUpdate {
                pubkey: user.pubkey.clone(),
                distance,
                trust_score,
                introduced_by: introduced_by
                    .get(user.pubkey.as_str())
                    .map(|s| s.iter().map(|k| (*k).to_string()).collect::<Vec<_>>())
                    .filter(|v: &Vec<String>| !v.is_empty()),
            }
        })
        .collect()
}

/// Partitions all known Web of Trust peers by distance relative to `self_pubkey`.
/// Distance 1 = Direct Friends, Distance 2 = Friends of Friends.
pub fn get_wot_peers_by_distance(
    self_pubkey: &str,
    users: &[WotUser],
    max_distance: u32,
) -> HashMap<u32, Vec<String>> {
    if users.len() > 64 {
        let cache = WOT_PEERS_CACHE.get_or_init(|| Mutex::new(HashMap::with_capacity(16)));
        let mut guard = cache.lock().unwrap();
        if let Some(cached) = guard.get(&(self_pubkey.to_string(), max_distance)) {
            return cached.clone();
        }
        let result = partition_wot_peers(self_pubkey, users, max_distance);
        if guard.len() >= 16 {
            guard.clear();
        }
        guard.insert((self_pubkey.to_string(), max_distance), result.clone());
        return result;
    }
    partition_wot_peers(self_pubkey, users, max_distance)
}

fn partition_wot_peers(
    self_pubkey: &str,
    users: &[WotUser],
    max_distance: u32,
) -> HashMap<u32, Vec<String>> {
    let updates = recalculate_wot(self_pubkey, users);
    let mut result: HashMap<u32, Vec<String>> = HashMap::new();
    for update in updates {
        if update.distance > 0 && update.distance <= max_distance {
            result
                .entry(update.distance)
                .or_default()
                .push(update.pubkey);
        }
    }
    result
}
