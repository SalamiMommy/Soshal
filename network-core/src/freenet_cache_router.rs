//! Freenet-native Web of Trust (WoT 1 & 2) cache router.
//! Handles fanning out post and media requests to direct friends (WoT 1) and friends-of-friends (WoT 2).

use crate::p2p_frame::FreenetP2PCommand;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CacheQueryResult {
    pub posts: Vec<Value>,
    pub source_wot_distance: u32,
    pub peer_pubkey: String,
}

/// Dispatches a cache request to a peer and returns the processed response.
pub fn process_freenet_cache_command(
    cmd: &FreenetP2PCommand,
    local_posts: &[Value],
    _self_pubkey: &str,
) -> Option<FreenetP2PCommand> {
    match cmd {
        FreenetP2PCommand::GetPostCache {
            authors,
            since,
            limit,
            wot_distance,
            ..
        } => {
            let limit = (*limit).min(200);
            let mut matching: Vec<Value> = local_posts
                .iter()
                .filter(|p| {
                    let created_at = p["created_at"].as_i64().unwrap_or(0);
                    if created_at < *since {
                        return false;
                    }
                    if !authors.is_empty() {
                        if let Some(pk) = p["pubkey"].as_str() {
                            return authors.iter().any(|a| a == pk);
                        }
                        return false;
                    }
                    true
                })
                .cloned()
                .collect();

            matching.truncate(limit);

            Some(FreenetP2PCommand::PostCacheResponse {
                posts: matching,
                wot_distance: *wot_distance,
            })
        }
        FreenetP2PCommand::GetFreenetContract {
            contract_key,
            max_hops,
            ..
        } => {
            if *max_hops == 0 {
                return None;
            }
            // Check if local node has the contract state for key
            for p in local_posts {
                if p["freenet_key"].as_str() == Some(contract_key.as_str()) {
                    return Some(FreenetP2PCommand::FreenetContractResponse {
                        contract_key: contract_key.clone(),
                        state_json: p.to_string(),
                        signature: p["sig"].as_str().unwrap_or("").to_string(),
                    });
                }
            }
            None
        }
        FreenetP2PCommand::GetMediaBlob {
            hash,
            chunk_offset,
            chunk_length,
        } => {
            // Content-addressed path first: serves byte ranges assembled from
            // the local chunk CAS (verified on read). Falls back to the legacy
            // whole-file cache so pre-chunked data keeps serving.
            if *chunk_length > 0 && *chunk_length <= MAX_BLOB_RESPONSE {
                let store = soshal_media_core::cas::ChunkStore::new(
                    soshal_media_core::cas::ChunkStore::default_root(),
                );
                if let Some(manifest) = store.load_manifest(hash) {
                    if let Some(bytes) = store.blob_slice(&manifest, *chunk_offset, *chunk_length) {
                        let data_b64 = soshal_crypto_core::base64::base64_encode_bytes(&bytes);
                        return Some(FreenetP2PCommand::MediaBlobResponse {
                            hash: hash.clone(),
                            offset: *chunk_offset,
                            total_size: manifest.total_size as usize,
                            data_b64,
                        });
                    }
                    return None;
                }
            }

            // Legacy whole-file cache fallback, guarded like the CAS path:
            // bounded chunk, validated 64-hex content hash, checked range,
            // read capped at MAX_BLOB_RESPONSE.
            let valid_hash = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            if *chunk_length > 0 && *chunk_length <= MAX_BLOB_RESPONSE && valid_hash {
                let app_dir = std::env::temp_dir().join("soshal_media_cache");
                let cached_path = app_dir.join(format!("{}.bin", hash));
                if cached_path.exists() {
                    if let Ok(file) = std::fs::File::open(&cached_path) {
                        let total = file.metadata().map(|m| m.len() as usize).unwrap_or(0);
                        if *chunk_offset < total {
                            if let Some(end) = chunk_offset.checked_add(*chunk_length) {
                                let end = end.min(total);
                                use std::io::{Read, Seek, SeekFrom};
                                let mut file = file;
                                let mut slice = vec![0u8; end.saturating_sub(*chunk_offset)];
                                if file.seek(SeekFrom::Start(*chunk_offset as u64)).is_ok()
                                    && file.read_exact(&mut slice).is_ok()
                                {
                                    let data_b64 =
                                        soshal_crypto_core::base64::base64_encode_bytes(&slice);
                                    return Some(FreenetP2PCommand::MediaBlobResponse {
                                        hash: hash.clone(),
                                        offset: *chunk_offset,
                                        total_size: total,
                                        data_b64,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Upper bound on a single media blob response (1 MiB — one nominal chunk).
const MAX_BLOB_RESPONSE: usize = 1024 * 1024;

/// Aggregates and deduplicates post results returned from multiple WoT 1 & WoT 2 peers.
pub fn merge_freenet_cache_responses(responses: Vec<CacheQueryResult>) -> Vec<Value> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for res in responses {
        for post in res.posts {
            if let Some(id) = post["id"].as_str() {
                if seen.insert(id.to_string()) {
                    let mut tagged_post = post;
                    if tagged_post.get("wot_source_distance").is_none() {
                        tagged_post["wot_source_distance"] =
                            serde_json::json!(res.source_wot_distance);
                    }
                    merged.push(tagged_post);
                }
            }
        }
    }

    merged.sort_by(|a, b| {
        let ta = a["created_at"].as_i64().unwrap_or(0);
        let tb = b["created_at"].as_i64().unwrap_or(0);
        tb.cmp(&ta)
    });

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_valid_get_post_cache() {
        let json_str = r#"{"type":"get_post_cache","authors":["npub1"],"since":0,"limit":10,"wot_distance":1,"allow_2hop":false}"#;
        let cmd = FreenetP2PCommand::from_json(json_str).unwrap();
        match cmd {
            FreenetP2PCommand::GetPostCache {
                authors,
                limit,
                wot_distance,
                ..
            } => {
                assert_eq!(authors, vec!["npub1".to_string()]);
                assert_eq!(limit, 10);
                assert_eq!(wot_distance, 1);
            }
            _ => panic!("expected GetPostCache"),
        }
    }

    #[test]
    fn parse_valid_media_blob_response() {
        let json_str = r#"{"type":"media_blob_response","hash":"h","offset":0,"total_size":10,"data_b64":"AQI="}"#;
        let cmd = FreenetP2PCommand::from_json(json_str).unwrap();
        match cmd {
            FreenetP2PCommand::MediaBlobResponse {
                hash,
                offset,
                total_size,
                data_b64,
            } => {
                assert_eq!(hash, "h");
                assert_eq!(offset, 0);
                assert_eq!(total_size, 10);
                assert_eq!(data_b64, "AQI=");
            }
            _ => panic!("expected MediaBlobResponse"),
        }
    }

    #[test]
    fn json_round_trip_all_variants() {
        let cmds = vec![
            FreenetP2PCommand::GetPostCache {
                authors: vec![],
                since: 1,
                limit: 5,
                wot_distance: 2,
                allow_2hop: true,
            },
            FreenetP2PCommand::PostCacheResponse {
                posts: vec![json!({"id": "p"})],
                wot_distance: 1,
            },
            FreenetP2PCommand::GetFreenetContract {
                contract_key: "k".to_string(),
                requester_pubkey: "r".to_string(),
                max_hops: 3,
            },
            FreenetP2PCommand::FreenetContractResponse {
                contract_key: "k".to_string(),
                state_json: "{}".to_string(),
                signature: "s".to_string(),
            },
            FreenetP2PCommand::GetMediaBlob {
                hash: "h".to_string(),
                chunk_offset: 0,
                chunk_length: 1024,
            },
            FreenetP2PCommand::MediaBlobResponse {
                hash: "h".to_string(),
                offset: 0,
                total_size: 10,
                data_b64: "AQI=".to_string(),
            },
        ];
        for cmd in cmds {
            let parsed = FreenetP2PCommand::from_json(&cmd.to_json());
            assert!(parsed.is_some(), "round-trip failed");
        }
    }

    #[test]
    fn empty_json_rejected() {
        assert!(FreenetP2PCommand::from_json("").is_none());
        assert!(FreenetP2PCommand::from_json("  ").is_none());
        assert!(FreenetP2PCommand::from_json("null").is_none());
        assert!(FreenetP2PCommand::from_json("{}").is_none());
    }

    #[test]
    fn partial_json_rejected() {
        let partials = [
            "{",
            "{\"type\"",
            "{\"type\":\"get_post_cache\"",
            "{\"type\":\"get_post_cache\",\"authors\":[",
            "{\"type\":\"get_post_cache\",\"authors\":[]",
            "{\"type\":\"media_blob_response\",\"hash\":\"h\",\"offset\":",
        ];
        for p in partials {
            assert!(
                FreenetP2PCommand::from_json(p).is_none(),
                "partial parsed: {p}"
            );
        }
    }

    #[test]
    fn truncated_valid_frame_rejected() {
        let full = FreenetP2PCommand::GetPostCache {
            authors: vec![],
            since: 0,
            limit: 1,
            wot_distance: 1,
            allow_2hop: false,
        }
        .to_json();
        for cut in 0..full.len() {
            assert!(
                FreenetP2PCommand::from_json(&full[..cut]).is_none(),
                "truncated at {cut} parsed"
            );
        }
    }

    #[test]
    fn empty_fields_parse_and_process() {
        let json_str = r#"{"type":"get_post_cache","authors":[],"since":0,"limit":0,"wot_distance":0,"allow_2hop":false}"#;
        let cmd = FreenetP2PCommand::from_json(json_str).unwrap();
        let result = process_freenet_cache_command(&cmd, &[], "self");
        assert!(result.is_some());
    }

    #[test]
    fn empty_local_posts_yields_empty_response() {
        let cmd = FreenetP2PCommand::GetPostCache {
            authors: vec![],
            since: 0,
            limit: 10,
            wot_distance: 1,
            allow_2hop: false,
        };
        let result = process_freenet_cache_command(&cmd, &[], "self").unwrap();
        match result {
            FreenetP2PCommand::PostCacheResponse { posts, .. } => assert!(posts.is_empty()),
            _ => panic!("expected PostCacheResponse"),
        }
    }

    #[test]
    fn hostile_limit_capped() {
        let cmd = FreenetP2PCommand::GetPostCache {
            authors: vec![],
            since: 0,
            limit: usize::MAX,
            wot_distance: 1,
            allow_2hop: false,
        };
        let posts: Vec<Value> = (0..500)
            .map(|i| json!({"id": format!("p{i}"), "created_at": i}))
            .collect();
        let result = process_freenet_cache_command(&cmd, &posts, "self").unwrap();
        match result {
            FreenetP2PCommand::PostCacheResponse { posts, .. } => assert_eq!(posts.len(), 200),
            _ => panic!("expected PostCacheResponse"),
        }
    }

    #[test]
    fn hostile_huge_authors_bounded() {
        let authors: Vec<String> = (0..10_000).map(|i| format!("npub{i}")).collect();
        let cmd = FreenetP2PCommand::GetPostCache {
            authors,
            since: 0,
            limit: 1,
            wot_distance: 1,
            allow_2hop: false,
        };
        let posts: Vec<Value> = (0..200)
            .map(|i| json!({"id": format!("p{i}"), "pubkey": "other", "created_at": i}))
            .collect();
        let result = process_freenet_cache_command(&cmd, &posts, "self").unwrap();
        match result {
            FreenetP2PCommand::PostCacheResponse { posts, .. } => assert!(posts.is_empty()),
            _ => panic!("expected PostCacheResponse"),
        }
    }

    #[test]
    fn hostile_absurd_chunk_length_rejected() {
        let cmd = FreenetP2PCommand::GetMediaBlob {
            hash: "nope".to_string(),
            chunk_offset: 0,
            chunk_length: usize::MAX,
        };
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    }

    #[test]
    fn hostile_absurd_chunk_offset_rejected() {
        let cmd = FreenetP2PCommand::GetMediaBlob {
            hash: "nope".to_string(),
            chunk_offset: usize::MAX,
            chunk_length: 1024,
        };
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    }

    #[test]
    fn hostile_zero_chunk_length_no_panic() {
        let cmd = FreenetP2PCommand::GetMediaBlob {
            hash: "nope".to_string(),
            chunk_offset: 0,
            chunk_length: 0,
        };
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    }

    #[test]
    fn hostile_max_hops_zero_rejected() {
        let cmd = FreenetP2PCommand::GetFreenetContract {
            contract_key: "k".to_string(),
            requester_pubkey: "r".to_string(),
            max_hops: 0,
        };
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    }

    #[test]
    fn hostile_oversized_string_fields() {
        let big = "x".repeat(64 * 1024);
        let json_str = format!(
            r#"{{"type":"get_freenet_contract","contract_key":"{}","requester_pubkey":"{}","max_hops":255}}"#,
            big, big
        );
        let cmd = FreenetP2PCommand::from_json(&json_str).unwrap();
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    }

    #[test]
    fn hostile_wrong_types_rejected() {
        let bad = [
            r#"{"type":"get_post_cache","authors":{},"since":0,"limit":1,"wot_distance":1,"allow_2hop":false}"#,
            r#"{"type":"get_post_cache","authors":[],"since":"now","limit":1,"wot_distance":1,"allow_2hop":false}"#,
            r#"{"type":"get_post_cache","authors":[],"since":0,"limit":-1,"wot_distance":1,"allow_2hop":false}"#,
            r#"{"type":"get_media_blob","hash":[],"chunk_offset":0,"chunk_length":1}"#,
        ];
        for b in bad {
            assert!(
                FreenetP2PCommand::from_json(b).is_none(),
                "bad json parsed: {b}"
            );
        }
    }

    #[test]
    fn unknown_variant_rejected() {
        assert!(FreenetP2PCommand::from_json(r#"{"type":"pwn","x":1}"#).is_none());
        assert!(FreenetP2PCommand::from_json(r#"{"type":"get_post_cache"}"#).is_none());
    }

    #[test]
    fn fuzz_garbage_json_no_panic() {
        let mut state: u64 = 0x5eed_2026_0815_0001;
        for _ in 0..100 {
            let mut bytes = Vec::new();
            let len = (state % 256) as usize;
            for _ in 0..len {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                bytes.push((state >> 33) as u8);
            }
            let input = String::from_utf8_lossy(&bytes);
            if let Some(cmd) = FreenetP2PCommand::from_json(&input) {
                let _ = process_freenet_cache_command(&cmd, &[], "self");
                let _ = process_freenet_cache_command(
                    &cmd,
                    &[json!({"id": "p", "pubkey": "npub1", "created_at": 1})],
                    "self",
                );
            }
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
        }
    }
}
