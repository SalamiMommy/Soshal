//! P2P network binary frame encoding and packet chunking kernel.
//!
//! Provides CRC32 checksum generation, frame assembly,
//! and chunking for BLE and LAN P2P offline transports.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Hard caps on frame sizes so a crafted `decode_p2p_frame` input cannot force
/// unbounded allocation or CPU work.
const MAX_CHUNKS: usize = 256;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_CHUNK_SIZE: usize = 64 * 1024;

#[derive(Deserialize, Serialize)]
pub struct FramePacketInput {
    pub payload: String,
    pub chunk_size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ChunkOutput {
    pub index: usize,
    pub total: usize,
    pub data: String,
    pub checksum: u32,
}

#[derive(Serialize, Deserialize)]
pub struct FramePacketResult {
    pub chunks: Vec<ChunkOutput>,
    pub crc32: u32,
}

#[derive(Deserialize, Serialize)]
pub struct ChunkInput {
    pub index: usize,
    pub total: usize,
    pub data: String,
    pub checksum: u32,
}

#[derive(Deserialize, Serialize)]
pub struct DecodeFrameInput {
    pub chunks: Vec<ChunkInput>,
    pub expected_crc32: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct DecodeFrameResult {
    pub payload: String,
    pub valid: bool,
    pub crc32: u32,
}

/// Freenet-native P2P commands for WoT depth 1 (friends) and depth 2 (friends-of-friends) cache retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FreenetP2PCommand {
    GetPostCache {
        authors: Vec<String>,
        since: i64,
        limit: usize,
        wot_distance: u32,
        allow_2hop: bool,
    },
    PostCacheResponse {
        posts: Vec<serde_json::Value>,
        wot_distance: u32,
    },
    GetFreenetContract {
        contract_key: String,
        requester_pubkey: String,
        max_hops: u8,
    },
    FreenetContractResponse {
        contract_key: String,
        state_json: String,
        signature: String,
    },
    GetMediaBlob {
        hash: String,
        chunk_offset: usize,
        chunk_length: usize,
    },
    MediaBlobResponse {
        hash: String,
        offset: usize,
        total_size: usize,
        data_b64: String,
    },
}

impl FreenetP2PCommand {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// Precomputed IEEE 802.3 CRC32 lookup table (polynomial 0xedb8_8320).
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// Computes IEEE CRC32 checksum of bytes using table lookup.
pub fn compute_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        let idx = ((crc ^ u32::from(byte)) & 0xff) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[idx];
    }
    !crc
}

/// Encodes a string payload into CRC32-validated chunks.
/// Input: JSON string of FramePacketInput.
/// Returns: JSON string of FramePacketResult (empty on failure).
pub fn encode_p2p_frame(json_input: &str) -> String {
    if json_input.len() > MAX_TOTAL_BYTES * 2 {
        return "{\"chunks\":[],\"crc32\":0}".to_string();
    }
    let input: FramePacketInput = match serde_json::from_str(json_input) {
        Ok(v) => v,
        Err(_) => return "{\"chunks\":[],\"crc32\":0}".to_string(),
    };

    let payload_str = &input.payload;
    if payload_str.len() > MAX_TOTAL_BYTES {
        return "{\"chunks\":[],\"crc32\":0}".to_string();
    }
    let total_crc = compute_crc32(payload_str.as_bytes());
    let chunk_size = if input.chunk_size == 0 {
        512
    } else {
        input.chunk_size.min(MAX_CHUNK_SIZE)
    };

    let mut slices = Vec::new();
    let mut start = 0;
    while start < payload_str.len() {
        let mut end = (start + chunk_size).min(payload_str.len());
        while end > start && !payload_str.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = payload_str[start..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| start + i)
                .unwrap_or(payload_str.len());
        }
        slices.push(&payload_str[start..end]);
        start = end;
    }

    let total = slices.len();
    if total > MAX_CHUNKS {
        return "{\"chunks\":[],\"crc32\":0}".to_string();
    }
    let mut chunks = Vec::with_capacity(total);
    for (idx, slice) in slices.into_iter().enumerate() {
        chunks.push(ChunkOutput {
            index: idx,
            total,
            data: slice.to_string(),
            checksum: compute_crc32(slice.as_bytes()),
        });
    }

    let result = FramePacketResult {
        chunks,
        crc32: total_crc,
    };

    serde_json::to_string(&result).unwrap_or_else(|_| "{\"chunks\":[],\"crc32\":0}".to_string())
}

/// Decodes and validates CRC32 chunks into original frame payload.
/// Input: JSON string of DecodeFrameInput.
/// Returns: JSON string of DecodeFrameResult.
pub fn decode_p2p_frame(json_input: &str) -> String {
    if json_input.len() > MAX_TOTAL_BYTES * 4 {
        return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
    }
    let input: DecodeFrameInput = match serde_json::from_str(json_input) {
        Ok(v) => v,
        Err(_) => return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string(),
    };

    // SECURITY: reject hostile chunk sets before doing any work — too many
    // chunks, out-of-range indices, duplicate indices, or an aggregate size
    // past the frame cap would otherwise cause unbounded allocation and CRC
    // CPU from crafted JSON.
    if input.chunks.is_empty() || input.chunks.len() > MAX_CHUNKS {
        return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
    }
    let expected_total = input.chunks[0].total;
    if expected_total == 0 || expected_total > MAX_CHUNKS || input.chunks.len() != expected_total {
        return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
    }
    for chunk in &input.chunks {
        if chunk.total != expected_total || chunk.index >= expected_total {
            return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
        }
    }
    let mut seen = HashSet::with_capacity(input.chunks.len());
    for chunk in &input.chunks {
        if !seen.insert(chunk.index) {
            return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
        }
    }
    let total_len: usize = input.chunks.iter().map(|c| c.data.len()).sum();
    if total_len > MAX_TOTAL_BYTES {
        return "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string();
    }

    let mut sorted_chunks = input.chunks;
    sorted_chunks.sort_unstable_by_key(|c| c.index);

    let mut payload_bytes = Vec::with_capacity(total_len);
    let mut valid = true;

    for chunk in &sorted_chunks {
        let bytes = chunk.data.as_bytes();
        let computed_crc = compute_crc32(bytes);
        if chunk.checksum != 0 && computed_crc != chunk.checksum {
            valid = false;
        }
        payload_bytes.extend_from_slice(bytes);
    }

    let total_crc = compute_crc32(&payload_bytes);
    if let Some(exp) = input.expected_crc32 {
        if exp != total_crc {
            valid = false;
        }
    }

    let payload = match String::from_utf8(payload_bytes) {
        Ok(s) => s,
        Err(e) => {
            valid = false;
            String::from_utf8_lossy(e.as_bytes()).into_owned()
        }
    };
    let result = DecodeFrameResult {
        payload,
        valid,
        crc32: total_crc,
    };

    serde_json::to_string(&result)
        .unwrap_or_else(|_| "{\"payload\":\"\",\"valid\":false,\"crc32\":0}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let payload = "hello p2p mesh frame world";
        let enc_input = serde_json::json!({
            "payload": payload,
            "chunk_size": 8
        });
        let enc_json = encode_p2p_frame(&enc_input.to_string());
        let res: FramePacketResult = serde_json::from_str(&enc_json).unwrap();
        assert!(!res.chunks.is_empty());
        assert!(res.chunks.len() <= MAX_CHUNKS);

        let dec_input = serde_json::json!({
            "chunks": res.chunks,
            "expected_crc32": res.crc32
        });
        let dec_json = decode_p2p_frame(&dec_input.to_string());
        let dec_res: DecodeFrameResult = serde_json::from_str(&dec_json).unwrap();
        assert!(dec_res.valid);
        assert_eq!(dec_res.payload, payload);
    }

    #[test]
    fn test_chunk_explosion_rejected() {
        // 10,000 bytes with chunk_size 1 would create 10,000 chunks > MAX_CHUNKS (256)
        let payload = "a".repeat(10_000);
        let enc_input = serde_json::json!({
            "payload": payload,
            "chunk_size": 1
        });
        let enc_json = encode_p2p_frame(&enc_input.to_string());
        assert_eq!(enc_json, "{\"chunks\":[],\"crc32\":0}");
    }
}
