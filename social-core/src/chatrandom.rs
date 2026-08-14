//! Chat-random peer matching: Jaccard-based interest scoring for 1:1 and group matching.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use soshal_common_core::json_util::{json_in, json_out};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChatRandomPeerCandidate {
    pub pubkey: String,
    pub interests: Vec<String>,
    #[serde(default)]
    pub expires_at: u64,
}

#[derive(Deserialize)]
pub struct RankChatRandomPeersInput {
    pub candidates: Vec<ChatRandomPeerCandidate>,
    pub my_interests: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ScoredPeer {
    pub pubkey: String,
    pub score: f64,
}

#[derive(Deserialize)]
pub struct GroupRoomCandidate {
    pub room_id: String,
    pub interests: Vec<String>,
    pub participant_count: usize,
    pub max_participants: usize,
}

#[derive(Deserialize)]
pub struct MatchGroupChatRandomInput {
    pub rooms: Vec<GroupRoomCandidate>,
    pub user_interests: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ScoredRoom {
    pub room_id: String,
    pub score: f64,
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

/// Jaccard similarity between two tag lists.
pub fn compute_jaccard_score(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: HashSet<String> = a
        .iter()
        .map(|s| s.to_lowercase().trim().to_string())
        .collect();
    let set_b: HashSet<String> = b
        .iter()
        .map(|s| s.to_lowercase().trim().to_string())
        .collect();
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Ranks candidate peers by interest overlap score descending.
/// Takes JSON input, returns JSON string of scored peers.
pub fn rank_chatrandom_peers(json_input: &str) -> String {
    let Some(input) = json_in::<Option<RankChatRandomPeersInput>>(json_input, None) else {
        return "[]".to_string();
    };

    let my_set: HashSet<String> = input
        .my_interests
        .into_iter()
        .map(|s| s.to_lowercase().trim().to_string())
        .collect();

    let mut scored: Vec<ScoredPeer> = input
        .candidates
        .into_iter()
        .map(|peer| {
            let peer_set: HashSet<String> = peer
                .interests
                .into_iter()
                .map(|s| s.to_lowercase().trim().to_string())
                .collect();
            let score = if my_set.is_empty() || peer_set.is_empty() {
                0.0
            } else {
                let intersection = my_set.intersection(&peer_set).count();
                let union = my_set.union(&peer_set).count();
                if union == 0 {
                    0.0
                } else {
                    intersection as f64 / union as f64
                }
            };
            ScoredPeer {
                pubkey: peer.pubkey,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    json_out(&scored, "[]")
}

/// Scores and ranks group chat-random rooms for matching.
/// Takes JSON input, returns JSON string of scored rooms.
pub fn match_group_chatrandom(json_input: &str) -> String {
    let Some(input) = json_in::<Option<MatchGroupChatRandomInput>>(json_input, None) else {
        return "[]".to_string();
    };

    let user_set: HashSet<String> = input
        .user_interests
        .into_iter()
        .map(|s| s.to_lowercase().trim().to_string())
        .collect();

    let mut scored: Vec<ScoredRoom> = input
        .rooms
        .into_iter()
        .filter(|room| room.participant_count < room.max_participants)
        .map(|room| {
            let room_set: HashSet<String> = room
                .interests
                .into_iter()
                .map(|s| s.to_lowercase().trim().to_string())
                .collect();

            let overlap = user_set.intersection(&room_set).count();
            let union = user_set.union(&room_set).count();
            let jaccard = if union == 0 {
                0.0
            } else {
                overlap as f64 / union as f64
            };

            let fullness_bonus = (room.participant_count as f64) / (room.max_participants as f64);
            let score = jaccard * 0.7 + fullness_bonus * 0.3;

            ScoredRoom {
                room_id: room.room_id,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    json_out(&scored, "[]")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
