//! Freenet cache router tests

use serde_json::json;
use soshal_network_core::freenet_cache_router::{
    merge_freenet_cache_responses, process_freenet_cache_command, CacheQueryResult,
};
use soshal_network_core::p2p_frame::FreenetP2PCommand;

#[test]
fn process_freenet_cache_command_get_post_cache() {
    let cmd = FreenetP2PCommand::GetPostCache {
        authors: vec!["npub1".to_string()],
        since: 0,
        limit: 10,
        wot_distance: 1,
        allow_2hop: false,
    };
    let local_posts = vec![
        json!({"id": "post1", "pubkey": "npub1", "created_at": 1000}),
        json!({"id": "post2", "pubkey": "npub2", "created_at": 2000}),
    ];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_some());
    if let FreenetP2PCommand::PostCacheResponse { posts, .. } = result.unwrap() {
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["id"], "post1");
    } else {
        panic!("Expected PostCacheResponse");
    }
}

#[test]
fn process_freenet_cache_command_respects_since() {
    let cmd = FreenetP2PCommand::GetPostCache {
        authors: vec![],
        since: 1500,
        limit: 10,
        wot_distance: 1,
        allow_2hop: false,
    };
    let local_posts = vec![
        json!({"id": "post1", "created_at": 1000}),
        json!({"id": "post2", "created_at": 2000}),
    ];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_some());
    if let FreenetP2PCommand::PostCacheResponse { posts, .. } = result.unwrap() {
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["id"], "post2");
    } else {
        panic!("Expected PostCacheResponse");
    }
}

#[test]
fn process_freenet_cache_command_respects_limit() {
    let cmd = FreenetP2PCommand::GetPostCache {
        authors: vec![],
        since: 0,
        limit: 2,
        wot_distance: 1,
        allow_2hop: false,
    };
    let local_posts = vec![
        json!({"id": "post1", "created_at": 1000}),
        json!({"id": "post2", "created_at": 2000}),
        json!({"id": "post3", "created_at": 3000}),
    ];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_some());
    if let FreenetP2PCommand::PostCacheResponse { posts, .. } = result.unwrap() {
        assert_eq!(posts.len(), 2);
    } else {
        panic!("Expected PostCacheResponse");
    }
}

#[test]
fn process_freenet_cache_command_get_contract() {
    let cmd = FreenetP2PCommand::GetFreenetContract {
        contract_key: "key1".to_string(),
        requester_pubkey: "npub1".to_string(),
        max_hops: 5,
    };
    let local_posts = vec![
        json!({"freenet_key": "key1", "sig": "sig1"}),
        json!({"freenet_key": "key2", "sig": "sig2"}),
    ];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_some());
    if let FreenetP2PCommand::FreenetContractResponse { contract_key, .. } = result.unwrap() {
        assert_eq!(contract_key, "key1");
    } else {
        panic!("Expected FreenetContractResponse");
    }
}

#[test]
fn process_freenet_cache_command_get_contract_max_hops_zero() {
    let cmd = FreenetP2PCommand::GetFreenetContract {
        contract_key: "key1".to_string(),
        requester_pubkey: "npub1".to_string(),
        max_hops: 0,
    };
    let local_posts = vec![json!({"freenet_key": "key1", "sig": "sig1"})];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_none());
}

#[test]
fn process_freenet_cache_command_get_media_blob_not_cached() {
    let cmd = FreenetP2PCommand::GetMediaBlob {
        hash: "hash1".to_string(),
        chunk_offset: 0,
        chunk_length: 1024,
    };
    let local_posts = vec![];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_none());
}

#[test]
fn process_freenet_cache_command_unknown_command() {
    let cmd = FreenetP2PCommand::PostCacheResponse {
        posts: vec![],
        wot_distance: 1,
    };
    let local_posts = vec![];
    let result = process_freenet_cache_command(&cmd, &local_posts, "npub_self");
    assert!(result.is_none());
}

#[test]
fn merge_freenet_cache_responses_deduplicates() {
    let responses = vec![
        CacheQueryResult {
            posts: vec![json!({"id": "post1", "created_at": 1000})],
            source_wot_distance: 1,
            peer_pubkey: "peer1".to_string(),
        },
        CacheQueryResult {
            posts: vec![json!({"id": "post1", "created_at": 1000})],
            source_wot_distance: 2,
            peer_pubkey: "peer2".to_string(),
        },
    ];
    let merged = merge_freenet_cache_responses(responses);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0]["id"], "post1");
}

#[test]
fn merge_freenet_cache_responses_adds_wot_tag() {
    let responses = vec![CacheQueryResult {
        posts: vec![json!({"id": "post1", "created_at": 1000})],
        source_wot_distance: 2,
        peer_pubkey: "peer1".to_string(),
    }];
    let merged = merge_freenet_cache_responses(responses);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0]["wot_source_distance"], 2);
}

#[test]
fn merge_freenet_cache_responses_sorts_by_created_at() {
    let responses = vec![
        CacheQueryResult {
            posts: vec![json!({"id": "post2", "created_at": 2000})],
            source_wot_distance: 1,
            peer_pubkey: "peer1".to_string(),
        },
        CacheQueryResult {
            posts: vec![json!({"id": "post1", "created_at": 1000})],
            source_wot_distance: 1,
            peer_pubkey: "peer2".to_string(),
        },
    ];
    let merged = merge_freenet_cache_responses(responses);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0]["id"], "post2");
    assert_eq!(merged[1]["id"], "post1");
}

#[test]
fn merge_freenet_cache_responses_empty() {
    let responses: Vec<CacheQueryResult> = vec![];
    let merged = merge_freenet_cache_responses(responses);
    assert!(merged.is_empty());
}
