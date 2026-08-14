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

            let app_dir = std::env::temp_dir().join("soshal_media_cache");
            let cached_path = app_dir.join(format!("{}.bin", hash));
            if cached_path.exists() {
                if let Ok(bytes) = std::fs::read(&cached_path) {
                    if *chunk_offset < bytes.len() {
                        let end = (chunk_offset + chunk_length).min(bytes.len());
                        let slice = &bytes[*chunk_offset..end];
                        let data_b64 = soshal_crypto_core::base64::base64_encode_bytes(slice);
                        return Some(FreenetP2PCommand::MediaBlobResponse {
                            hash: hash.clone(),
                            offset: *chunk_offset,
                            total_size: bytes.len(),
                            data_b64,
                        });
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
