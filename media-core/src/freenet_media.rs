use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_crypto_core::{
    base64::{base64_decode_bytes, base64_encode_bytes},
    hash::sha256_hex,
};

const MAX_INPUT_B64_LEN: usize = 10 * 1024 * 1024;
pub const CHUNK_SIZE: usize = 256 * 1024;

#[derive(Deserialize)]
pub struct ChunkMediaInput {
    #[serde(rename = "dataB64")]
    pub data_b64: String,
    #[serde(rename = "chunkSize")]
    pub chunk_size: Option<usize>,
}

#[derive(Serialize)]
pub struct ChunkMediaOutput {
    #[serde(rename = "chunkCount")]
    pub chunk_count: usize,
    #[serde(rename = "totalSize")]
    pub total_size: usize,
    #[serde(rename = "chunkHashes")]
    pub chunk_hashes: Vec<String>,
    #[serde(rename = "contentHash")]
    pub content_hash: String,
}

#[derive(Deserialize)]
pub struct VerifyChunkInput {
    #[serde(rename = "dataB64")]
    pub data_b64: String,
    #[serde(rename = "expectedHash")]
    pub expected_hash: String,
}

#[derive(Serialize)]
pub struct VerifyChunkOutput {
    pub valid: bool,
}

#[derive(Deserialize)]
pub struct ReconstructInput {
    #[serde(rename = "chunksB64")]
    pub chunks_b64: Vec<String>,
}

#[derive(Serialize)]
pub struct ReconstructOutput {
    #[serde(rename = "dataB64")]
    pub data_b64: String,
    #[serde(rename = "totalSize")]
    pub total_size: usize,
}

pub fn chunk_media(data_b64: &str, chunk_size: usize) -> Option<ChunkMediaOutput> {
    if data_b64.len() > MAX_INPUT_B64_LEN {
        return None;
    }
    let raw = base64_decode_bytes(data_b64)?;
    let chunk_size = if chunk_size == 0 {
        CHUNK_SIZE
    } else {
        chunk_size.min(10 * 1024 * 1024)
    };
    if raw.is_empty() {
        return None;
    }
    let total_size = raw.len();
    let mut chunk_hashes = Vec::new();
    for chunk in raw.chunks(chunk_size) {
        chunk_hashes.push(sha256_hex(chunk));
    }
    let content_hash = sha256_hex(&raw);
    Some(ChunkMediaOutput {
        chunk_count: chunk_hashes.len(),
        total_size,
        chunk_hashes,
        content_hash,
    })
}

pub fn verify_chunk(data_b64: &str, expected_hash: &str) -> bool {
    let raw = match base64_decode_bytes(data_b64) {
        Some(d) => d,
        None => return false,
    };
    sha256_hex(&raw) == expected_hash
}

pub fn reconstruct_media(chunks_b64: &[String]) -> Option<ReconstructOutput> {
    if chunks_b64.is_empty() {
        return None;
    }
    // A remote peer must not be able to exhaust memory with an unbounded
    // number/length of chunks (chunk_media caps input at 10 MiB; the
    // reconstruct side must enforce the same bound).
    if chunks_b64.len() > 128 {
        return None;
    }
    let est_cap = chunks_b64.iter().map(|c| c.len() * 3 / 4).sum();
    let mut all_data = Vec::with_capacity(est_cap);
    for chunk_b64 in chunks_b64 {
        if chunk_b64.len() > 14 * 1024 * 1024 {
            return None;
        }
        let raw = base64_decode_bytes(chunk_b64)?;
        if all_data.len() + raw.len() > MAX_INPUT_B64_LEN {
            return None;
        }
        all_data.extend_from_slice(&raw);
    }

    let total_size = all_data.len();
    let data_b64 = base64_encode_bytes(&all_data);
    Some(ReconstructOutput {
        data_b64,
        total_size,
    })
}

pub fn chunk_media_json(input: &str) -> String {
    let Some(input) = json_in::<Option<ChunkMediaInput>>(input, None) else {
        return r#"{"error":"invalid input"}"#.to_string();
    };
    match chunk_media(&input.data_b64, input.chunk_size.unwrap_or(CHUNK_SIZE)) {
        Some(r) => json_out(&r, r#"{"error":"serialize failed"}"#),
        None => r#"{"error":"chunk failed"}"#.to_string(),
    }
}

pub fn verify_chunk_json(input: &str) -> String {
    let Some(input) = json_in::<Option<VerifyChunkInput>>(input, None) else {
        return r#"{"valid":false}"#.to_string();
    };
    let valid = verify_chunk(&input.data_b64, &input.expected_hash);
    let output = VerifyChunkOutput { valid };
    json_out(&output, r#"{"valid":false}"#)
}

pub fn reconstruct_media_json(input: &str) -> String {
    let Some(input) = json_in::<Option<ReconstructInput>>(input, None) else {
        return r#"{"error":"invalid input"}"#.to_string();
    };
    match reconstruct_media(&input.chunks_b64) {
        Some(r) => json_out(&r, r#"{"error":"serialize failed"}"#),
        None => r#"{"error":"reconstruct failed"}"#.to_string(),
    }
}
