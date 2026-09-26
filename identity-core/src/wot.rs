use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

type WotPeersCache = HashMap<(String, u32), HashMap<u32, Vec<String>>>;
type WotCacheOrder = VecDeque<(String, u32)>;

static WOT_PEERS_CACHE: OnceLock<Mutex<(WotPeersCache, WotCacheOrder)>> = OnceLock::new();

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
    if contacts_a.is_empty() || contacts_b.is_empty() {
        return 0;
    }
    let set_a: HashSet<String> = contacts_a.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut seen_b = HashSet::new();
    let mut count = 0;
    for b in contacts_b {
        let b_lower = b.to_ascii_lowercase();
        if seen_b.insert(b_lower.clone()) && set_a.contains(&b_lower) {
            count += 1;
        }
    }
    count
}

pub fn compute_distance(
    user_pubkey: &str,
    target_pubkey: &str,
    direct_follows: &[String],
    mutual_count: usize,
) -> u32 {
    if user_pubkey.eq_ignore_ascii_case(target_pubkey) {
        return 0;
    }
    if direct_follows
        .iter()
        .any(|f| f.eq_ignore_ascii_case(target_pubkey))
    {
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
    let score = calculate_trust_score(distance, mutual, mutual);
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
            let introducers = introduced_by.get(user.pubkey.as_str());
            let introducer_count = introducers.map_or(0, |s| s.len());
            let trust_score = calculate_trust_score(distance, mutual_count, introducer_count);
            let introduced_by_vec = introducers.and_then(|s| {
                if s.is_empty() {
                    None
                } else {
                    Some(s.iter().map(|k| (*k).to_string()).collect())
                }
            });
            WotUpdate {
                pubkey: user.pubkey.clone(),
                distance,
                trust_score,
                introduced_by: introduced_by_vec,
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
        let cache = WOT_PEERS_CACHE
            .get_or_init(|| Mutex::new((HashMap::with_capacity(16), VecDeque::new())));
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.0.get(&(self_pubkey.to_string(), max_distance)) {
            return cached.clone();
        }
        let result = partition_wot_peers(self_pubkey, users, max_distance);
        let key = (self_pubkey.to_string(), max_distance);
        // FIFO eviction of the oldest entry instead of clearing the whole
        // cache on overflow (the old clear thrashed on 17th entry).
        while guard.0.len() >= 16 {
            if let Some(oldest) = guard.1.pop_front() {
                guard.0.remove(&oldest);
            } else {
                guard.0.clear();
                break;
            }
        }
        guard.1.retain(|k| k != &key);
        guard.1.push_back(key.clone());
        guard.0.insert(key, result.clone());
        return result;
    }
    partition_wot_peers(self_pubkey, users, max_distance)
}

/// Drop all cached WoT partitions. Call whenever the contact graph mutates
/// (follow/unfollow) so stale distance partitions are never served.
pub fn invalidate_wot_peers_cache() {
    if let Some(cache) = WOT_PEERS_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.0.clear();
            guard.1.clear();
        }
    }
}

fn partition_wot_peers(
    self_pubkey: &str,
    users: &[WotUser],
    max_distance: u32,
) -> HashMap<u32, Vec<String>> {
    if max_distance == 0 || users.is_empty() {
        return HashMap::new();
    }

    const MAX_CONTACTS: usize = 2000;
    const MAX_EDGES: usize = 100_000;

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

    let mut distances: HashMap<&str, u32> = HashMap::with_capacity(users.len());
    let mut queue: VecDeque<(&str, u32)> = VecDeque::with_capacity(users.len().min(10_000));

    distances.insert(self_pubkey, 0);
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
                    if new_dist >= existing {
                        continue;
                    }
                } else {
                    distances.insert(contact_str, new_dist);
                    if new_dist < 2 {
                        queue.push_back((contact_str, new_dist));
                    }
                }
            }
        }
    }

    let mut result: HashMap<u32, Vec<String>> = HashMap::new();
    for user in users {
        let distance = *distances.get(user.pubkey.as_str()).unwrap_or(&3);
        if distance > 0 && distance <= max_distance {
            result
                .entry(distance)
                .or_default()
                .push(user.pubkey.clone());
        }
    }
    result
}

use petgraph::algo::dijkstra;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;

/// Directed Web of Trust graph backed by `petgraph`.
#[derive(Debug, Clone, Default)]
pub struct WotGraph {
    graph: DiGraph<String, f32>,
    node_map: HashMap<String, NodeIndex>,
}

impl WotGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a user pubkey if not present, returning its NodeIndex.
    pub fn get_or_insert_node(&mut self, pubkey: &str) -> NodeIndex {
        let pk_lower = pubkey.to_ascii_lowercase();
        if let Some(&idx) = self.node_map.get(&pk_lower) {
            idx
        } else {
            let idx = self.graph.add_node(pk_lower.clone());
            self.node_map.insert(pk_lower, idx);
            idx
        }
    }

    /// Add a follow/trust directed edge from `source` to `target` with a trust weight (default 1.0).
    pub fn add_trust_edge(&mut self, source: &str, target: &str, weight: f32) {
        let u = self.get_or_insert_node(source);
        let v = self.get_or_insert_node(target);
        self.graph.update_edge(u, v, weight);
    }

    /// Build a WotGraph from a list of WotUsers.
    pub fn from_users(users: &[WotUser]) -> Self {
        let mut g = Self::new();
        for user in users {
            let u = g.get_or_insert_node(&user.pubkey);
            for contact in &user.contacts {
                let v = g.get_or_insert_node(contact);
                g.graph.update_edge(u, v, 1.0);
            }
        }
        g
    }

    /// Compute shortest path trust distance between two pubkeys using Dijkstra.
    pub fn shortest_distance(&self, source: &str, target: &str) -> Option<f32> {
        let src_idx = *self.node_map.get(&source.to_ascii_lowercase())?;
        let tgt_idx = *self.node_map.get(&target.to_ascii_lowercase())?;
        if src_idx == tgt_idx {
            return Some(0.0);
        }
        let node_scores = dijkstra(&self.graph, src_idx, Some(tgt_idx), |edge| *edge.weight());
        node_scores.get(&tgt_idx).copied()
    }

    /// Perform a BFS to find all pubkeys within `max_hops` from `root`.
    pub fn find_k_hop_peers(&self, root: &str, max_hops: usize) -> HashMap<String, usize> {
        let mut results = HashMap::new();
        let Some(&root_idx) = self.node_map.get(&root.to_ascii_lowercase()) else {
            return results;
        };
        let mut bfs = Bfs::new(&self.graph, root_idx);
        let mut distances: HashMap<NodeIndex, usize> = HashMap::new();
        distances.insert(root_idx, 0);

        while let Some(nx) = bfs.next(&self.graph) {
            let dist = *distances.get(&nx).unwrap_or(&0);
            if dist >= max_hops {
                continue;
            }
            for neighbor in self.graph.neighbors(nx) {
                if let std::collections::hash_map::Entry::Vacant(e) = distances.entry(neighbor) {
                    e.insert(dist + 1);
                    results.insert(self.graph[neighbor].clone(), dist + 1);
                }
            }
        }
        results
    }

    /// Number of nodes and edges in the graph.
    pub fn stats(&self) -> (usize, usize) {
        (self.graph.node_count(), self.graph.edge_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_mutual_deduplication_and_case_insensitivity() {
        let contacts_a = vec!["ALICE".to_string(), "bob".to_string(), "alice".to_string()];
        let contacts_b = vec![
            "alice".to_string(),
            "ALICE".to_string(),
            "alice".to_string(),
            "Charlie".to_string(),
        ];
        assert_eq!(count_mutual(&contacts_a, &contacts_b), 1);
    }

    #[test]
    fn test_compute_distance_case_insensitivity() {
        let follows = vec!["BOB".to_string()];
        assert_eq!(compute_distance("alice", "ALICE", &follows, 0), 0);
        assert_eq!(compute_distance("alice", "bob", &follows, 0), 1);
        assert_eq!(compute_distance("alice", "BOB", &follows, 0), 1);
    }

    #[test]
    fn test_wot_graph_traversal() {
        let mut graph = WotGraph::new();
        graph.add_trust_edge("alice", "bob", 1.0);
        graph.add_trust_edge("bob", "charlie", 1.0);
        graph.add_trust_edge("alice", "david", 1.0);

        let (nodes, edges) = graph.stats();
        assert_eq!(nodes, 4);
        assert_eq!(edges, 3);

        assert_eq!(graph.shortest_distance("alice", "alice"), Some(0.0));
        assert_eq!(graph.shortest_distance("alice", "bob"), Some(1.0));
        assert_eq!(graph.shortest_distance("alice", "charlie"), Some(2.0));
        assert_eq!(graph.shortest_distance("charlie", "alice"), None);

        let hops = graph.find_k_hop_peers("alice", 2);
        assert_eq!(hops.get("bob"), Some(&1));
        assert_eq!(hops.get("david"), Some(&1));
        assert_eq!(hops.get("charlie"), Some(&2));
    }
}
