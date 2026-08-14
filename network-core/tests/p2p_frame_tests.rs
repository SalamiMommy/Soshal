//! P2P frame encoding tests

use soshal_network_core::p2p_frame::{
    compute_crc32, decode_p2p_frame, encode_p2p_frame, ChunkInput, DecodeFrameInput,
    DecodeFrameResult, FramePacketInput, FramePacketResult, FreenetP2PCommand,
};

#[test]
fn compute_crc32_is_deterministic() {
    let data = b"hello world";
    let crc1 = compute_crc32(data);
    let crc2 = compute_crc32(data);
    assert_eq!(crc1, crc2);
}

#[test]
fn compute_crc32_differs_for_different_data() {
    let crc1 = compute_crc32(b"hello");
    let crc2 = compute_crc32(b"world");
    assert_ne!(crc1, crc2);
}

#[test]
fn encode_p2p_frame_basic() {
    let input = FramePacketInput {
        payload: "hello world".to_string(),
        chunk_size: 5,
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = encode_p2p_frame(&json_input);

    let parsed: FramePacketResult = serde_json::from_str(&result).unwrap();
    assert!(!parsed.chunks.is_empty());
    assert_ne!(parsed.crc32, 0);
}

#[test]
fn encode_p2p_frame_rejects_oversized_payload() {
    let input = FramePacketInput {
        payload: "x".repeat(1024 * 1024 + 1),
        chunk_size: 512,
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = encode_p2p_frame(&json_input);

    assert_eq!(result, "{\"chunks\":[],\"crc32\":0}");
}

#[test]
fn encode_p2p_frame_caps_chunk_size() {
    let input = FramePacketInput {
        payload: "hello world".to_string(),
        chunk_size: 100_000, // Exceeds MAX_CHUNK_SIZE
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = encode_p2p_frame(&json_input);

    let parsed: FramePacketResult = serde_json::from_str(&result).unwrap();
    assert!(!parsed.chunks.is_empty());
}

#[test]
fn decode_p2p_frame_valid() {
    let encode_input = FramePacketInput {
        payload: "hello world".to_string(),
        chunk_size: 5,
    };
    let encode_json = serde_json::to_string(&encode_input).unwrap();
    let encode_result = encode_p2p_frame(&encode_json);
    let encoded: FramePacketResult = serde_json::from_str(&encode_result).unwrap();

    let decode_input = DecodeFrameInput {
        chunks: encoded
            .chunks
            .into_iter()
            .map(|c| ChunkInput {
                index: c.index,
                total: c.total,
                data: c.data,
                checksum: c.checksum,
            })
            .collect(),
        expected_crc32: Some(encoded.crc32),
    };
    let decode_json = serde_json::to_string(&decode_input).unwrap();
    let decode_result = decode_p2p_frame(&decode_json);

    let decoded: DecodeFrameResult = serde_json::from_str(&decode_result).unwrap();
    assert!(decoded.valid);
    assert_eq!(decoded.payload, "hello world");
    assert_eq!(decoded.crc32, encoded.crc32);
}

#[test]
fn decode_p2p_frame_rejects_too_many_chunks() {
    let chunks: Vec<ChunkInput> = (0..257)
        .map(|i| ChunkInput {
            index: i,
            total: 257,
            data: "x".to_string(),
            checksum: 0,
        })
        .collect();
    let input = DecodeFrameInput {
        chunks,
        expected_crc32: None,
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = decode_p2p_frame(&json_input);

    assert_eq!(result, "{\"payload\":\"\",\"valid\":false,\"crc32\":0}");
}

#[test]
fn decode_p2p_frame_rejects_duplicate_indices() {
    let chunks = vec![
        ChunkInput {
            index: 0,
            total: 2,
            data: "hello".to_string(),
            checksum: 0,
        },
        ChunkInput {
            index: 0,
            total: 2,
            data: "world".to_string(),
            checksum: 0,
        },
    ];
    let input = DecodeFrameInput {
        chunks,
        expected_crc32: None,
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = decode_p2p_frame(&json_input);

    assert_eq!(result, "{\"payload\":\"\",\"valid\":false,\"crc32\":0}");
}

#[test]
fn decode_p2p_frame_rejects_oversized_total() {
    let chunks = vec![ChunkInput {
        index: 0,
        total: 2,
        data: "x".repeat(1024 * 1024 + 1),
        checksum: 0,
    }];
    let input = DecodeFrameInput {
        chunks,
        expected_crc32: None,
    };
    let json_input = serde_json::to_string(&input).unwrap();
    let result = decode_p2p_frame(&json_input);

    assert_eq!(result, "{\"payload\":\"\",\"valid\":false,\"crc32\":0}");
}

#[test]
fn decode_p2p_frame_crc_mismatch() {
    let encode_input = FramePacketInput {
        payload: "hello world".to_string(),
        chunk_size: 5,
    };
    let encode_json = serde_json::to_string(&encode_input).unwrap();
    let encode_result = encode_p2p_frame(&encode_json);
    let encoded: FramePacketResult = serde_json::from_str(&encode_result).unwrap();

    let mut chunks: Vec<ChunkInput> = encoded
        .chunks
        .into_iter()
        .map(|c| ChunkInput {
            index: c.index,
            total: c.total,
            data: c.data,
            checksum: c.checksum,
        })
        .collect();
    if let Some(first) = chunks.get_mut(0) {
        first.checksum = 999999;
    }

    let decode_input = DecodeFrameInput {
        chunks,
        expected_crc32: None,
    };
    let decode_json = serde_json::to_string(&decode_input).unwrap();
    let decode_result = decode_p2p_frame(&decode_json);

    let decoded: DecodeFrameResult = serde_json::from_str(&decode_result).unwrap();
    assert!(!decoded.valid);
}

#[test]
fn freenet_p2p_command_serialization() {
    let cmd = FreenetP2PCommand::GetPostCache {
        authors: vec!["npub1".to_string()],
        since: 123456,
        limit: 100,
        wot_distance: 1,
        allow_2hop: false,
    };
    let json = cmd.to_json();
    let parsed = FreenetP2PCommand::from_json(&json);
    assert!(parsed.is_some());
}

#[test]
fn freenet_p2p_command_all_variants() {
    let commands = vec![
        FreenetP2PCommand::GetPostCache {
            authors: vec!["npub1".to_string()],
            since: 0,
            limit: 10,
            wot_distance: 1,
            allow_2hop: false,
        },
        FreenetP2PCommand::PostCacheResponse {
            posts: vec![serde_json::json!({})],
            wot_distance: 1,
        },
        FreenetP2PCommand::GetFreenetContract {
            contract_key: "key1".to_string(),
            requester_pubkey: "npub1".to_string(),
            max_hops: 5,
        },
        FreenetP2PCommand::FreenetContractResponse {
            contract_key: "key1".to_string(),
            state_json: "{}".to_string(),
            signature: "sig1".to_string(),
        },
        FreenetP2PCommand::GetMediaBlob {
            hash: "hash1".to_string(),
            chunk_offset: 0,
            chunk_length: 1024,
        },
        FreenetP2PCommand::MediaBlobResponse {
            hash: "hash1".to_string(),
            offset: 0,
            total_size: 2048,
            data_b64: "data".to_string(),
        },
    ];

    for cmd in commands {
        let json = cmd.to_json();
        let parsed = FreenetP2PCommand::from_json(&json);
        assert!(parsed.is_some(), "Failed to parse: {}", json);
    }
}
