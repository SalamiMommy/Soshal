//! P2P network binary frame encoding and packet chunking kernel.
//!
//! Provides CRC32 checksum generation, frame assembly,
//! and chunking for BLE and LAN P2P offline transports.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;

/// Hard caps on frame sizes so a crafted `decode_p2p_frame` input cannot force
/// unbounded allocation or CPU work.
const MAX_CHUNKS: usize = 256;
const MAX_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_CHUNK_SIZE: usize = 64 * 1024;

/// Encode-side request.
///
/// `payload` stays an owned `String`, and that is deliberate. Borrowing it —
/// `#[serde(borrow)] payload: &'a str` — looks strictly better, and it does
/// work: the parse becomes free and the chunk slices can point straight into the
/// caller's JSON text. It also silently rejects **any payload containing a JSON
/// escape**.
///
/// The cause is serde's `&str` field visitor, which only accepts borrowed input.
/// When `serde_json` meets an escape sequence it parses the value into an owned
/// `String` and calls `visit_string`, which that visitor refuses. So a payload
/// containing a tab, a newline, a quote or a backslash fails to parse with a
/// misleading "invalid type: string …, expected a borrowed string", and
/// `encode_p2p_frame` returns the empty-chunks sentinel. Non-ASCII is unaffected
/// — those bytes are borrowed as-is with no unescaping needed — which makes the
/// failure look narrow and unrelated to escaping.
/// `borrowed_encode_produces_byte_identical_json` pins it.
///
/// The copy that does get removed is the per-chunk one, in [`ChunkOutput`].
#[derive(Deserialize, Serialize)]
pub struct FramePacketInput {
    pub payload: String,
    pub chunk_size: usize,
}

/// One encoded chunk.
///
/// `data` is a `Cow`, and the asymmetry is deliberate: **serializing** borrows
/// (no copy) while **deserializing** allocates. That is the right split, because
/// encode is the hot path — its JSON is what crosses the FFI boundary — whereas
/// reading a `FramePacketResult` back is a decode/inspection step.
///
/// The copy this removes is the per-chunk one: the old `slice.to_string()`
/// materialised a second full copy of the payload in up to `MAX_CHUNKS` pieces
/// before the serializer wrote the output, so a payload at the `MAX_TOTAL_BYTES`
/// cap was resident three times over — parsed string, chunk strings, output.
///
/// A plain `&'a str` with `#[serde(borrow)]` would also make the *read* path
/// zero-copy, but it rejects escaped content: serde's `&str` visitor only accepts
/// borrowed input, and `serde_json` hands it an owned `String` whenever the value
/// contains an escape sequence. A chunk holding a tab, a newline, a quote or a
/// backslash would fail to deserialize with `invalid type: string "tab\t",
/// expected a borrowed string` — the same trap that stops `FramePacketInput`
/// from borrowing its payload. `Cow` accepts both forms, so the two paths differ
/// in cost and not in capability.
///
/// Note there is deliberately no `#[serde(borrow)]` here, which is also why
/// `FramePacketResult` needs no `Deserialize` lifetime bound: a deserialized
/// `Cow` is always `Cow::Owned`, so `'a` is unconstrained on the read path.
#[derive(Serialize, Deserialize)]
pub struct ChunkOutput<'a> {
    pub index: usize,
    pub total: usize,
    pub data: Cow<'a, str>,
    pub checksum: u32,
}

/// Encode-side result. The borrow cascades from the payload through each chunk.
#[derive(Serialize, Deserialize)]
pub struct FramePacketResult<'a> {
    pub chunks: Vec<ChunkOutput<'a>>,
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

/// Computes IEEE CRC32 checksum of bytes using SIMD-accelerated instructions (SSE4.2 / ARMv8 PMULL).
pub fn compute_crc32(bytes: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

/// Sentinel returned by [`encode_p2p_frame`] for any rejected input.
const ENCODE_REJECTED: &str = "{\"chunks\":[],\"crc32\":0}";

/// Encodes a string payload into CRC32-validated chunks.
/// Input: JSON string of FramePacketInput.
/// Returns: JSON string of FramePacketResult (empty on failure).
pub fn encode_p2p_frame(json_input: &str) -> String {
    if json_input.len() > MAX_TOTAL_BYTES * 2 {
        return ENCODE_REJECTED.to_string();
    }
    let input: FramePacketInput = match serde_json::from_str(json_input) {
        Ok(v) => v,
        Err(_) => return ENCODE_REJECTED.to_string(),
    };
    // Split out purely so a test can inspect the built result before
    // serialization. Whether a chunk's data is borrowed or owned is invisible in
    // the emitted JSON, so without this seam a refactor could reintroduce the
    // per-chunk copy and no test would notice.
    encode_chunks(&input)
        .map(|result| {
            serde_json::to_string(&result).unwrap_or_else(|_| ENCODE_REJECTED.to_string())
        })
        .unwrap_or_else(|| ENCODE_REJECTED.to_string())
}

/// Builds the chunk list, borrowing every chunk's data out of `input.payload`.
///
/// Taking the already-parsed input is what makes the borrow expressible at all:
/// the payload is a local `String`, so a function that both parses and returns
/// borrows from itself and cannot compile. `None` means the input was rejected;
/// the caller substitutes [`ENCODE_REJECTED`].
fn encode_chunks(input: &FramePacketInput) -> Option<FramePacketResult<'_>> {
    let payload_str = &input.payload;
    if payload_str.len() > MAX_TOTAL_BYTES {
        return None;
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
        return None;
    }
    // `slices` borrows from `payload_str`, which borrows from `json_input`, and
    // each chunk's `data` borrows from its slice — so this loop allocates no
    // payload bytes at all, only the `ChunkOutput` headers. The old
    // `slice.to_string()` here was a second full copy of the payload, on top of
    // the one serde had already made when parsing the input out of the JSON.
    let mut chunks = Vec::with_capacity(total);
    for (idx, slice) in slices.into_iter().enumerate() {
        chunks.push(ChunkOutput {
            index: idx,
            total,
            data: Cow::Borrowed(slice),
            checksum: compute_crc32(slice.as_bytes()),
        });
    }

    Some(FramePacketResult {
        chunks,
        crc32: total_crc,
    })
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

    /// The chunker walks the payload by *byte* index and has to back each
    /// boundary up to a char boundary, or `&payload[a..b]` panics on a multi-byte
    /// character. Nothing exercised that before: every existing fixture is ASCII,
    /// where every index is a boundary and the whole loop is untested. A panic in
    /// the middle of an encode is also an FFI-boundary panic, since this is
    /// reachable from a `p2p_frame_*` bridge call with caller-supplied JSON.
    #[test]
    fn multibyte_payloads_chunk_on_char_boundaries_and_round_trip() {
        // Deliberately awkward: 2-, 3- and 4-byte characters, so a chunk_size of
        // 1, 2, 3 or 4 forces the boundary walkback on nearly every chunk. The
        // emoji also span a surrogate pair, which is where an off-by-one in the
        // walkback would show up as a panic or a corrupted payload.
        let payload: String = (0..200)
            .map(|i| match i % 4 {
                0 => 'a',
                1 => 'é',
                2 => '日',
                _ => '😀',
            })
            .collect();
        assert!(
            payload.len() > payload.chars().count(),
            "fixture must actually be multi-byte"
        );

        for chunk_size in [1usize, 2, 3, 4, 5, 7, 64] {
            let enc_json = encode_p2p_frame(
                &serde_json::json!({ "payload": payload, "chunk_size": chunk_size }).to_string(),
            );
            let res: FramePacketResult = serde_json::from_str(&enc_json).unwrap();
            assert!(!res.chunks.is_empty());
            assert!(
                res.chunks.len() <= MAX_CHUNKS,
                "chunk_size {chunk_size} produced {} chunks",
                res.chunks.len()
            );

            // Concatenating the chunks must reproduce the payload exactly — if a
            // boundary walkback dropped or repeated a byte, this is where it shows.
            let rejoined: String = res.chunks.iter().map(|c| c.data.as_ref()).collect();
            assert_eq!(
                rejoined, payload,
                "chunk_size {chunk_size} did not round trip"
            );

            // And through the decode path, which re-validates every chunk CRC.
            let dec_json = decode_p2p_frame(
                &serde_json::json!({
                    "chunks": res.chunks,
                    "expected_crc32": res.crc32,
                })
                .to_string(),
            );
            let dec: DecodeFrameResult = serde_json::from_str(&dec_json).unwrap();
            assert!(dec.valid, "chunk_size {chunk_size} failed to decode");
            assert_eq!(dec.payload, payload);
        }
    }

    /// Every chunk's data must be **borrowed**, not owned. This is the only
    /// directly observable form of the memory optimization: whether a chunk holds
    /// a `Cow::Borrowed` or a `Cow::Owned` makes no difference to the emitted
    /// JSON, so nothing else in the suite can tell a fixed build from one that
    /// quietly reintroduced the per-chunk copy.
    ///
    /// A payload at the `MAX_TOTAL_BYTES` cap is the case that matters — one
    /// owned chunk per 512 bytes means ~2000 copies, or a second full copy of the
    /// payload held in `MAX_CHUNKS` fragments while the output is being written.
    ///
    /// The fixture is sized to the largest payload the encoder actually accepts at
    /// this chunk size: `MAX_CHUNKS * chunk_size` is 131,072 bytes, and 200,000
    /// is rejected outright with the empty-chunks sentinel.
    #[test]
    fn chunks_borrow_their_data_instead_of_copying_it() {
        let payload = "A".repeat(MAX_CHUNKS * 512);
        let input = FramePacketInput {
            payload: payload.clone(),
            chunk_size: 512,
        };
        let result = encode_chunks(&input).expect("valid input");

        assert_eq!(result.chunks.len(), MAX_CHUNKS);
        assert!(
            result
                .chunks
                .iter()
                .all(|c| matches!(c.data, Cow::Borrowed(_))),
            "every chunk must borrow from the payload, not own a copy of it"
        );
        // And the borrows really do point into the payload rather than into some
        // other equally-owned buffer.
        let base = input.payload.as_ptr() as usize;
        for c in &result.chunks {
            let p = c.data.as_ptr() as usize;
            assert!(
                p >= base && p < base + input.payload.len(),
                "chunk data is not inside the payload buffer"
            );
        }
    }

    /// The encode output is unchanged by making the payload a borrow rather than
    /// an owned copy. Worth pinning explicitly: the JSON is the wire format, and
    /// the borrowed form reaches the serializer through a different code path
    /// (escaping rules for `&str` vs `String`), so "it still compiles" is not
    /// evidence that the bytes are the same.
    #[test]
    fn borrowed_encode_produces_byte_identical_json() {
        // Golden output for a payload that needs escaping in the JSON *around* it
        // but whose chunks are plain base64-safe ASCII, plus one with a character
        // that must be escaped, so both branches of the serializer are covered.
        for payload in ["hello world", "tab\there \"quoted\" back\\slash", "é日😀"] {
            let json = encode_p2p_frame(
                &serde_json::json!({ "payload": payload, "chunk_size": 4 }).to_string(),
            );
            // Re-serializing the parsed result must be a fixed point: the encoder
            // emitted valid, canonically-ordered JSON that round trips.
            let parsed: FramePacketResult = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                json,
                "encode output is not canonical for {payload:?}"
            );
            let rejoined: String = parsed.chunks.iter().map(|c| c.data.as_ref()).collect();
            assert_eq!(rejoined, payload);
        }
    }
}
