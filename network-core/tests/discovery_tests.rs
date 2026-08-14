//! Discovery tests

use soshal_network_core::discovery::{
    discover_freenet_swarm, discover_freenet_swarm_json, suggest_mutual_friends,
    suggest_mutual_friends_json, AllUserInfo, FreenetSwarmDiscoveryInput, FreenetSwarmManifest,
    SuggestMutualFriendsInput,
};

#[test]
fn suggest_mutual_friends_basic() {
    let input = SuggestMutualFriendsInput {
        self_pubkey: "user1".to_string(),
        self_contacts: vec!["friend1".to_string(), "friend2".to_string()],
        all_users: vec![
            AllUserInfo {
                pubkey: "user2".to_string(),
                contacts: vec!["friend1".to_string(), "friend3".to_string()],
                wot_distance: 2,
            },
            AllUserInfo {
                pubkey: "user3".to_string(),
                contacts: vec!["friend2".to_string()],
                wot_distance: 3,
            },
        ],
        limit: 10,
    };

    let results = suggest_mutual_friends(input);
    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .any(|r| r.pubkey == "user2" && r.mutual_count == 1));
    assert!(results
        .iter()
        .any(|r| r.pubkey == "user3" && r.mutual_count == 1));
}

#[test]
fn suggest_mutual_friends_respects_limit() {
    let mut users = Vec::new();
    for i in 2..=20 {
        users.push(AllUserInfo {
            pubkey: format!("user{}", i),
            contacts: vec!["friend1".to_string()],
            wot_distance: 2,
        });
    }

    let input = SuggestMutualFriendsInput {
        self_pubkey: "user1".to_string(),
        self_contacts: vec!["friend1".to_string()],
        all_users: users,
        limit: 5,
    };

    let results = suggest_mutual_friends(input);
    assert_eq!(results.len(), 5);
}

#[test]
fn suggest_mutual_friends_rejects_oversized_input() {
    let input = SuggestMutualFriendsInput {
        self_pubkey: "user1".to_string(),
        self_contacts: vec![],
        all_users: vec![
            AllUserInfo {
                pubkey: "user2".to_string(),
                contacts: vec![],
                wot_distance: 2,
            };
            100_001
        ],
        limit: 10,
    };

    let results = suggest_mutual_friends(input);
    assert!(results.is_empty());
}

#[test]
fn suggest_mutual_friends_json_interface() {
    let json_input = r#"{
        "self_pubkey": "user1",
        "self_contacts": ["friend1"],
        "all_users": [
            {
                "pubkey": "user2",
                "contacts": ["friend1"],
                "wot_distance": 2
            }
        ],
        "limit": 10
    }"#;

    let result = suggest_mutual_friends_json(json_input);
    assert!(result.contains("user2"));
}

#[test]
fn discover_freenet_swarm_basic() {
    let input = FreenetSwarmDiscoveryInput {
        self_pubkey: "user1".to_string(),
        self_friends: vec!["friend1".to_string()],
        manifests: vec![FreenetSwarmManifest {
            peer_pubkey: "friend1".to_string(),
            friends: vec!["user2".to_string(), "user3".to_string()],
            contracts: vec!["contract1".to_string()],
            gateway_url: Some("http://gateway1".to_string()),
        }],
        limit: 10,
    };

    let results = discover_freenet_swarm(input);
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.pubkey == "user2"));
    assert!(results.iter().any(|r| r.pubkey == "user3"));
}

#[test]
fn discover_freenet_swarm_only_direct_friends() {
    let input = FreenetSwarmDiscoveryInput {
        self_pubkey: "user1".to_string(),
        self_friends: vec!["friend1".to_string()],
        manifests: vec![FreenetSwarmManifest {
            peer_pubkey: "stranger".to_string(),
            friends: vec!["user2".to_string()],
            contracts: vec![],
            gateway_url: None,
        }],
        limit: 10,
    };

    let results = discover_freenet_swarm(input);
    assert!(results.is_empty());
}

#[test]
fn discover_freenet_swarm_aggregates_contracts() {
    let input = FreenetSwarmDiscoveryInput {
        self_pubkey: "user1".to_string(),
        self_friends: vec!["friend1".to_string(), "friend2".to_string()],
        manifests: vec![
            FreenetSwarmManifest {
                peer_pubkey: "friend1".to_string(),
                friends: vec!["user2".to_string()],
                contracts: vec!["contract1".to_string()],
                gateway_url: None,
            },
            FreenetSwarmManifest {
                peer_pubkey: "friend2".to_string(),
                friends: vec!["user2".to_string()],
                contracts: vec!["contract2".to_string()],
                gateway_url: None,
            },
        ],
        limit: 10,
    };

    let results = discover_freenet_swarm(input);
    let user2_result = results.iter().find(|r| r.pubkey == "user2").unwrap();
    assert_eq!(user2_result.contracts.len(), 2);
    assert!(user2_result.contracts.contains(&"contract1".to_string()));
    assert!(user2_result.contracts.contains(&"contract2".to_string()));
}

#[test]
fn discover_freenet_swarm_json_interface() {
    let json_input = r#"{
        "self_pubkey": "user1",
        "self_friends": ["friend1"],
        "manifests": [
            {
                "peer_pubkey": "friend1",
                "friends": ["user2"],
                "contracts": [],
                "gateway_url": null
            }
        ],
        "limit": 10
    }"#;

    let result = discover_freenet_swarm_json(json_input);
    assert!(result.contains("user2"));
}
