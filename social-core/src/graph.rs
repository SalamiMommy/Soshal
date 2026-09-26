//! Social follow graph analysis, degrees of separation, mutual circles,
//! and friend-of-a-friend recommendation algorithms using `petgraph`.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Directed relation edge type between users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationType {
    /// Follower -> Followee.
    Follows,
}

/// In-memory social graph tracking follow relationships and network topology.
#[derive(Debug, Clone, Default)]
pub struct SocialFollowGraph {
    graph: DiGraph<String, RelationType>,
    pubkey_to_node: HashMap<String, NodeIndex>,
}

impl SocialFollowGraph {
    /// Create a new empty social follow graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            pubkey_to_node: HashMap::new(),
        }
    }

    /// Ensure a user node exists in the graph and return its internal index.
    pub fn add_user(&mut self, pubkey: &str) -> NodeIndex {
        if let Some(&node) = self.pubkey_to_node.get(pubkey) {
            return node;
        }
        let node = self.graph.add_node(pubkey.to_string());
        self.pubkey_to_node.insert(pubkey.to_string(), node);
        node
    }

    /// Add a directed follow relationship (`follower` follows `followee`).
    pub fn add_follow(&mut self, follower: &str, followee: &str) {
        if follower == followee {
            return;
        }
        let a = self.add_user(follower);
        let b = self.add_user(followee);

        // Check if edge already exists to prevent duplicate parallel edges
        if !self.graph.contains_edge(a, b) {
            self.graph.add_edge(a, b, RelationType::Follows);
        }
    }

    /// Remove a follow edge.
    pub fn remove_follow(&mut self, follower: &str, followee: &str) -> bool {
        let (Some(&a), Some(&b)) = (
            self.pubkey_to_node.get(follower),
            self.pubkey_to_node.get(followee),
        ) else {
            return false;
        };

        if let Some(edge) = self.graph.find_edge(a, b) {
            self.graph.remove_edge(edge);
            true
        } else {
            false
        }
    }

    /// Check if `follower` follows `followee`.
    pub fn is_following(&self, follower: &str, followee: &str) -> bool {
        let (Some(&a), Some(&b)) = (
            self.pubkey_to_node.get(follower),
            self.pubkey_to_node.get(followee),
        ) else {
            return false;
        };
        self.graph.contains_edge(a, b)
    }

    /// Check if two users mutually follow each other.
    pub fn is_mutual(&self, user_a: &str, user_b: &str) -> bool {
        self.is_following(user_a, user_b) && self.is_following(user_b, user_a)
    }

    /// Find all users mutually followed by both `user_a` and `user_b`.
    pub fn mutual_follows(&self, user_a: &str, user_b: &str) -> Vec<String> {
        let (Some(&a), Some(&b)) = (
            self.pubkey_to_node.get(user_a),
            self.pubkey_to_node.get(user_b),
        ) else {
            return Vec::new();
        };

        let follows_a: std::collections::HashSet<NodeIndex> = self
            .graph
            .neighbors_directed(a, Direction::Outgoing)
            .collect();

        self.graph
            .neighbors_directed(b, Direction::Outgoing)
            .filter(|n| follows_a.contains(n))
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Calculate the degrees of separation between two users using Breadth-First Search (BFS).
    ///
    /// - Returns `Some(1)` if `from` directly follows `to`.
    /// - Returns `Some(2)` if `from` follows someone who follows `to` (friend of a friend).
    /// - Returns `None` if no path exists or if `from == to`.
    pub fn degrees_of_separation(&self, from: &str, to: &str) -> Option<usize> {
        if from == to {
            return None;
        }
        let (Some(&start), Some(&goal)) =
            (self.pubkey_to_node.get(from), self.pubkey_to_node.get(to))
        else {
            return None;
        };

        let mut distances: HashMap<NodeIndex, usize> = HashMap::new();
        distances.insert(start, 0);

        let mut bfs = Bfs::new(&self.graph, start);
        while let Some(current) = bfs.next(&self.graph) {
            let current_dist = distances[&current];
            if current == goal {
                return Some(current_dist);
            }

            for neighbor in self.graph.neighbors_directed(current, Direction::Outgoing) {
                distances
                    .entry(neighbor)
                    .or_insert_with(|| current_dist + 1);
            }
        }

        distances.get(&goal).copied()
    }

    /// Recommend users based on Friend-of-a-Friend (FOAF) connectivity.
    ///
    /// Returns a list of `(pubkey, common_connections_count)` sorted descending by relevance.
    pub fn friend_of_friend_recommendations(
        &self,
        user: &str,
        limit: usize,
    ) -> Vec<(String, usize)> {
        let Some(&user_node) = self.pubkey_to_node.get(user) else {
            return Vec::new();
        };

        let mut already_following: std::collections::HashSet<NodeIndex> = self
            .graph
            .neighbors_directed(user_node, Direction::Outgoing)
            .collect();
        already_following.insert(user_node);

        let mut candidate_scores: HashMap<NodeIndex, usize> = HashMap::new();

        // For each user that `user` follows:
        for followed_node in self
            .graph
            .neighbors_directed(user_node, Direction::Outgoing)
        {
            // Check who they follow
            for foaf_node in self
                .graph
                .neighbors_directed(followed_node, Direction::Outgoing)
            {
                if !already_following.contains(&foaf_node) {
                    *candidate_scores.entry(foaf_node).or_insert(0) += 1;
                }
            }
        }

        let mut results: Vec<(String, usize)> = candidate_scores
            .into_iter()
            .map(|(node, score)| (self.graph[node].clone(), score))
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        results.truncate(limit);
        results
    }

    /// Number of followers for a given user.
    pub fn follower_count(&self, user: &str) -> usize {
        self.pubkey_to_node
            .get(user)
            .map(|&n| {
                self.graph
                    .neighbors_directed(n, Direction::Incoming)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Number of users a given user follows.
    pub fn following_count(&self, user: &str) -> usize {
        self.pubkey_to_node
            .get(user)
            .map(|&n| {
                self.graph
                    .neighbors_directed(n, Direction::Outgoing)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Total number of users registered in the graph.
    pub fn user_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Total number of follow edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_graph_follows_and_mutuals() {
        let mut graph = SocialFollowGraph::new();
        graph.add_follow("alice", "bob");
        graph.add_follow("bob", "alice");
        graph.add_follow("alice", "charlie");

        assert!(graph.is_following("alice", "bob"));
        assert!(graph.is_following("bob", "alice"));
        assert!(graph.is_mutual("alice", "bob"));

        assert!(graph.is_following("alice", "charlie"));
        assert!(!graph.is_following("charlie", "alice"));
        assert!(!graph.is_mutual("alice", "charlie"));

        assert_eq!(graph.follower_count("bob"), 1);
        assert_eq!(graph.following_count("alice"), 2);
    }

    #[test]
    fn test_degrees_of_separation() {
        let mut graph = SocialFollowGraph::new();
        // alice -> bob -> charlie -> dave
        graph.add_follow("alice", "bob");
        graph.add_follow("bob", "charlie");
        graph.add_follow("charlie", "dave");

        assert_eq!(graph.degrees_of_separation("alice", "bob"), Some(1));
        assert_eq!(graph.degrees_of_separation("alice", "charlie"), Some(2));
        assert_eq!(graph.degrees_of_separation("alice", "dave"), Some(3));
        assert_eq!(graph.degrees_of_separation("dave", "alice"), None);
        assert_eq!(graph.degrees_of_separation("alice", "alice"), None);
    }

    #[test]
    fn test_mutual_follows_and_foaf_recommendations() {
        let mut graph = SocialFollowGraph::new();
        // Alice follows Bob and Dave
        graph.add_follow("alice", "bob");
        graph.add_follow("alice", "dave");

        // Charlie also follows Bob and Dave
        graph.add_follow("charlie", "bob");
        graph.add_follow("charlie", "dave");

        let mut mutuals = graph.mutual_follows("alice", "charlie");
        mutuals.sort();
        assert_eq!(mutuals, vec!["bob", "dave"]);

        // Bob and Dave both follow Eve (whom Alice does not follow)
        graph.add_follow("bob", "eve");
        graph.add_follow("dave", "eve");

        let recs = graph.friend_of_friend_recommendations("alice", 5);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].0, "eve");
        assert_eq!(recs[0].1, 2); // 2 mutual connections (Bob and Dave) recommend Eve
    }
}
