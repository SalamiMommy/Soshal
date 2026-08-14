//! Integration tests for soshal-media-core.

use soshal_media_core::decoder::decode_to_rgba;
use soshal_media_core::envelope::{open_dm, open_group, seal_dm, seal_group};
use soshal_media_core::prefetcher::Prefetcher;
use soshal_media_core::freenet::{
    freenet_content_hash, freenet_content_hash_json, freenet_contract_hash,
    freenet_contract_hash_json,
};
use soshal_media_core::freenet_media::{chunk_media, reconstruct_media, verify_chunk, CHUNK_SIZE};
use soshal_media_core::freenet_post::{
    build_freenet_post_contract, verify_and_unpack_freenet_post,
};
use soshal_media_core::livestream_util::{
    merge_chat_messages, merge_chat_messages_json, parse_live_streams, parse_live_streams_json,
};
use soshal_media_core::media::guess_mime_type;
use soshal_media_core::mini_util::{parse_minis, parse_minis_json};
use soshal_media_core::musicloud_util::{parse_musiclouds, parse_musiclouds_json};

fn event(
    id: &str,
    pubkey: &str,
    content: &str,
    tags: Vec<Vec<String>>,
    created_at: f64,
) -> soshal_nostr_core::models::NostrEvent {
    soshal_nostr_core::models::NostrEvent {
        id: id.into(),
        pubkey: pubkey.into(),
        content: content.into(),
        tags,
        created_at,
        kind: 30078,
    }
}

// ---------------------------------------------------------------------------
// MIME guessing
// ---------------------------------------------------------------------------

#[test]
fn guess_mime_type_maps_known_extensions() {
    assert_eq!(guess_mime_type("jpg"), "image/jpeg");
    assert_eq!(guess_mime_type(".png"), "image/png");
    assert_eq!(guess_mime_type("gif"), "image/gif");
    assert_eq!(guess_mime_type("webp"), "image/webp");
    assert_eq!(guess_mime_type("avif"), "image/avif");
    assert_eq!(guess_mime_type("mp4"), "video/mp4");
    assert_eq!(guess_mime_type("webm"), "video/webm");
    assert_eq!(guess_mime_type("mov"), "video/quicktime");
    assert_eq!(guess_mime_type("mp3"), "audio/mpeg");
    assert_eq!(guess_mime_type("ogg"), "application/octet-stream");
    assert_eq!(guess_mime_type("wav"), "audio/wav");
    assert_eq!(guess_mime_type("flac"), "audio/flac");
    assert_eq!(guess_mime_type("pdf"), "application/pdf");
}

#[test]
fn guess_mime_type_unknown_falls_back() {
    assert_eq!(guess_mime_type("xyz"), "application/octet-stream");
    assert_eq!(guess_mime_type(""), "application/octet-stream");
    assert_eq!(guess_mime_type(".exe"), "application/octet-stream");
}

// ---------------------------------------------------------------------------
// Musicloud
// ---------------------------------------------------------------------------

#[test]
fn parse_musiclouds_maps_tags_and_sorts() {
    let events = vec![
        event(
            "m1",
            "pk1",
            "https://fallback.mp3",
            vec![
                vec!["url".into(), "https://cdn/track1.mp3".into()],
                vec!["title".into(), "Song One".into()],
                vec!["duration".into(), "180".into()],
                vec!["thumb".into(), "https://x/thumb.png".into()],
                vec!["audience".into(), "public".into()],
                vec!["genre".into(), "rock".into()],
                vec!["waveform".into(), "0.1,0.5,1.0".into()],
            ],
            100.0,
        ),
        event("m2", "pk2", "https://cdn/track2.mp3", vec![], 200.0),
        event("", "pk3", "https://cdn/empty-id.mp3", vec![], 300.0),
        event("m4", "pk4", "", vec![], 400.0),
    ];
    let tracks = parse_musiclouds(events);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].id, "m2");
    assert_eq!(tracks[1].id, "m1");
    assert_eq!(tracks[1].url, "https://cdn/track1.mp3");
    assert_eq!(tracks[1].title.as_deref(), Some("Song One"));
    assert_eq!(tracks[1].duration, Some(180));
    assert_eq!(tracks[1].thumbnail.as_deref(), Some("https://x/thumb.png"));
    assert_eq!(tracks[1].genre.as_deref(), Some("rock"));
    assert_eq!(
        tracks[1].waveform_data.as_deref(),
        Some(vec![0.1, 0.5, 1.0]).as_deref()
    );
    assert_eq!(tracks[1].audience, "public");
    assert_eq!(tracks[0].url, "https://cdn/track2.mp3");
}

#[test]
fn parse_musiclouds_json_roundtrip() {
    let input = serde_json::json!({
        "events": [
            {"id": "t1", "pubkey": "pk", "content": "https://x/a.mp3",
             "tags": [["title", "A"]], "created_at": 10.0, "kind": 1}
        ]
    });
    let out = parse_musiclouds_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["id"], "t1");
    assert_eq!(v[0]["title"], "A");
    assert_eq!(v[0]["createdAt"], 10);
    assert_eq!(parse_musiclouds_json("garbage"), "[]");
}

// ---------------------------------------------------------------------------
// Live streams
// ---------------------------------------------------------------------------

#[test]
fn parse_live_streams_filters_by_category_and_status() {
    let events = vec![
        event(
            "s1",
            "pk1",
            "fallback title",
            vec![
                vec!["title".into(), "Music Stream".into()],
                vec!["status".into(), "live".into()],
                vec!["category".into(), "Music".into()],
                vec!["sfu".into(), "wss://sfu.example".into()],
            ],
            100.0,
        ),
        event(
            "s2",
            "pk2",
            "title2",
            vec![vec!["status".into(), "ended".into()]],
            200.0,
        ),
        event(
            "s3",
            "pk3",
            "title3",
            vec![vec!["status".into(), "live".into()]],
            50.0,
        ),
    ];
    let all = parse_live_streams(events.clone(), None);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "s1");
    assert_eq!(all[0].title, "Music Stream");
    assert_eq!(all[0].sfu_url, "wss://sfu.example");
    assert_eq!(all[1].id, "s3");

    let music = parse_live_streams(events.clone(), Some("music"));
    assert_eq!(music.len(), 1);
    assert_eq!(music[0].id, "s1");

    let gaming = parse_live_streams(events, Some("gaming"));
    assert_eq!(gaming.len(), 0);
}

#[test]
fn parse_live_streams_json_roundtrip() {
    let input = serde_json::json!({
        "events": [
            {"id": "s1", "pubkey": "pk", "content": "t", "tags": [["status", "live"], ["category", "Music"]], "created_at": 5.0, "kind": 30311}
        ],
        "category": "Music"
    });
    let out = parse_live_streams_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["startTime"], 5);
    assert_eq!(parse_live_streams_json("bad"), "[]");
}

#[test]
fn merge_chat_messages_dedupes_and_sorts() {
    let local =
        vec![serde_json::from_value(serde_json::json!({"id": "a", "createdAt": 10})).unwrap()];
    let relay = vec![
        serde_json::from_value(serde_json::json!({"id": "a", "createdAt": 10})).unwrap(),
        serde_json::from_value(serde_json::json!({"id": "b", "createdAt": 5})).unwrap(),
        serde_json::from_value(serde_json::json!({"id": "", "createdAt": 1})).unwrap(),
        serde_json::from_value(serde_json::json!({"content": "no id", "createdAt": 20})).unwrap(),
    ];
    let merged = merge_chat_messages(local, relay, "stream1");
    let ids: Vec<Option<&str>> = merged.iter().map(|m| m.id.as_deref()).collect();
    assert_eq!(ids, vec![Some(""), Some("b"), Some("a"), None]);
}

#[test]
fn merge_chat_messages_json_roundtrip() {
    let input = serde_json::json!({
        "local": [{"id": "x", "createdAt": 1}],
        "relay": [{"id": "x", "createdAt": 1}, {"id": "y", "createdAt": 2}],
        "streamId": "s1"
    });
    let out = merge_chat_messages_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Minis
// ---------------------------------------------------------------------------

#[test]
fn parse_minis_maps_tags_and_filters_empty() {
    let events = vec![
        event(
            "i1",
            "pk1",
            "https://fallback",
            vec![
                vec!["url".into(), "https://cdn/mini.mp4".into()],
                vec!["title".into(), "Mini Title".into()],
                vec!["thumb".into(), "https://x/thumb.png".into()],
            ],
            50.0,
        ),
        event("i2", "pk2", "", vec![], 100.0),
    ];
    let minis = parse_minis(events);
    assert_eq!(minis.len(), 1);
    assert_eq!(minis[0].url, "https://cdn/mini.mp4");
    assert_eq!(minis[0].text_overlay.as_deref(), Some("Mini Title"));
    assert_eq!(minis[0].thumbnail.as_deref(), Some("https://x/thumb.png"));
    assert_eq!(minis[0].audience, "public");
}

#[test]
fn parse_minis_json_roundtrip() {
    let input = serde_json::json!({
        "events": [
            {"id": "i1", "pubkey": "pk", "content": "https://x/m.mp4", "tags": [], "created_at": 7.0, "kind": 1}
        ]
    });
    let out = parse_minis_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["id"], "i1");
    assert_eq!(v[0]["createdAt"], 7);
    assert_eq!(parse_minis_json("nope"), "[]");
}

// ---------------------------------------------------------------------------
// Freenet media chunking
// ---------------------------------------------------------------------------

#[test]
fn chunk_media_splits_and_hashes() {
    let raw = b"hello freenet world, this is a test payload!";
    let b64 = soshal_crypto_core::base64::base64_encode_bytes(raw);
    let out = chunk_media(&b64, 8).unwrap();
    assert_eq!(out.total_size, raw.len());
    assert_eq!(out.chunk_hashes.len(), out.chunk_count);
    let expected = soshal_crypto_core::hash::sha256_hex(raw);
    assert_eq!(out.content_hash, expected);
    assert_eq!(
        out.chunk_hashes[0],
        soshal_crypto_core::hash::sha256_hex(&raw[0..8])
    );
}

#[test]
fn chunk_media_uses_default_chunk_size() {
    let raw = vec![7u8; 300_000];
    let b64 = soshal_crypto_core::base64::base64_encode_bytes(&raw);
    let out = chunk_media(&b64, 0).unwrap();
    assert_eq!(out.chunk_count, 2);
    assert_eq!(out.chunk_hashes[0].len(), 64);
    let raw_all = soshal_crypto_core::base64::base64_decode_bytes(&b64).unwrap();
    assert_eq!(
        out.chunk_hashes[0],
        soshal_crypto_core::hash::sha256_hex(&raw_all[0..CHUNK_SIZE])
    );
}

#[test]
fn chunk_media_rejects_empty_or_invalid() {
    assert!(chunk_media("", 0).is_none());
    assert!(chunk_media("!!!not-base64!!!", 0).is_none());
}

#[test]
fn verify_chunk_matches_hash() {
    let raw = b"chunk data";
    let b64 = soshal_crypto_core::base64::base64_encode_bytes(raw);
    let hash = soshal_crypto_core::hash::sha256_hex(raw);
    assert!(verify_chunk(&b64, &hash));
    assert!(!verify_chunk(&b64, &"0".repeat(64)));
    assert!(!verify_chunk("bad-base64", &hash));
}

#[test]
fn reconstruct_media_roundtrips_chunks() {
    let raw = b"roundtrip payload for media";
    let b64 = soshal_crypto_core::base64::base64_encode_bytes(raw);
    let out = chunk_media(&b64, 4).unwrap();
    let chunk_b64s = split_into_chunks(&b64, 4);
    let rec = reconstruct_media(&chunk_b64s).unwrap();
    assert_eq!(rec.total_size, raw.len());
    assert_eq!(rec.data_b64, b64);
    assert_eq!(
        soshal_crypto_core::hash::sha256_hex(
            &soshal_crypto_core::base64::base64_decode_bytes(&rec.data_b64).unwrap()
        ),
        out.content_hash
    );
}

fn split_into_chunks(b64: &str, size: usize) -> Vec<String> {
    b64.as_bytes()
        .chunks(size)
        .map(|c| String::from_utf8(c.to_vec()).unwrap())
        .collect()
}

#[test]
fn reconstruct_media_rejects_empty_or_oversized() {
    assert!(reconstruct_media(&[]).is_none());
    let too_many: Vec<String> = (0..200).map(|_| "aGVsbG8=".to_string()).collect();
    assert!(reconstruct_media(&too_many).is_none());
    let bad = vec!["not-b64!".to_string()];
    assert!(reconstruct_media(&bad).is_none());
}

// ---------------------------------------------------------------------------
// Freenet post contract
// ---------------------------------------------------------------------------

#[test]
fn build_freenet_post_contract_is_deterministic() {
    let a = build_freenet_post_contract("pk1", "hello", 100, None, None, Some("sig1".to_string()))
        .unwrap();
    let b = build_freenet_post_contract("pk1", "hello", 100, None, None, Some("sig1".to_string()))
        .unwrap();
    assert_eq!(a.contract_key, b.contract_key);
    assert_eq!(a.content_hash, b.content_hash);
    assert!(a.contract_key.starts_with("freenet://"));
    assert_eq!(a.content_hash.len(), 64);

    let different =
        build_freenet_post_contract("pk2", "hello", 100, None, None, Some("sig1".to_string()))
            .unwrap();
    assert_ne!(a.contract_key, different.contract_key);
}

#[test]
fn build_freenet_post_contract_rejects_empty_content() {
    assert!(build_freenet_post_contract("pk1", "", 100, None, None, None).is_err());
}

#[test]
fn verify_and_unpack_freenet_post_roundtrip() {
    let contract = build_freenet_post_contract(
        "pk1",
        "post body",
        123,
        Some("reply-id".to_string()),
        Some("root-id".to_string()),
        Some("sig".to_string()),
    )
    .unwrap();
    let payload = verify_and_unpack_freenet_post(&contract.payload_json).unwrap();
    assert_eq!(payload.author_pubkey, "pk1");
    assert_eq!(payload.content, "post body");
    assert_eq!(payload.created_at, 123);
    assert_eq!(payload.reply_to.as_deref(), Some("reply-id"));
    assert_eq!(payload.root_id.as_deref(), Some("root-id"));
    assert_eq!(payload.signature.as_deref(), Some("sig"));
}

#[test]
fn verify_and_unpack_freenet_post_rejects_bad_input() {
    assert!(verify_and_unpack_freenet_post("").is_err());
    assert!(verify_and_unpack_freenet_post("not json").is_err());
    assert!(verify_and_unpack_freenet_post(r#"{"content":"no author"}"#).is_err());
}

// ---------------------------------------------------------------------------
// Freenet hashes
// ---------------------------------------------------------------------------

#[test]
fn freenet_contract_hash_is_deterministic_and_domain_separated() {
    let h1 = freenet_contract_hash("key", "params");
    assert_eq!(h1.len(), 64);
    assert_eq!(h1, freenet_contract_hash("key", "params"));
    assert_ne!(h1, freenet_contract_hash("key", "other"));
    assert_ne!(h1, freenet_contract_hash("other", "params"));
}

#[test]
fn freenet_content_hash_hashes_decoded_bytes() {
    let raw = b"media bytes";
    let b64 = soshal_crypto_core::base64::base64_encode_bytes(raw);
    let h = freenet_content_hash(&b64);
    assert_eq!(h, soshal_crypto_core::hash::sha256_hex(raw));
    assert_eq!(freenet_content_hash("not base64!"), "");
}

#[test]
fn freenet_hash_json_apis() {
    let out = freenet_contract_hash_json(r#"{"key":"k","parameters":"p"}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["contract_hash"].as_str().unwrap().len(), 64);

    let out2 = freenet_content_hash_json(&serde_json::json!({"data": "aGVsbG8="}).to_string());
    let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
    assert_eq!(
        v2["content_hash"],
        soshal_crypto_core::hash::sha256_hex(b"hello")
    );

    assert_eq!(freenet_contract_hash_json("garbage"), "");
    assert_eq!(freenet_content_hash_json("garbage"), "");
}

// ---------------------------------------------------------------------------
// Media envelope
// ---------------------------------------------------------------------------

#[test]
fn dm_envelope_seal_open_roundtrip() {
    let (pk, sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let data = b"secret media payload";
    let (ct, nonce, payload) = seal_dm(data, &pk).unwrap();
    let opened = open_dm(&ct, &nonce, &payload, &sk).unwrap();
    assert_eq!(opened, data);
}

#[test]
fn dm_envelope_wrong_key_fails() {
    let (pk, _sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let (_, sk2) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let (ct, nonce, payload) = seal_dm(b"data", &pk).unwrap();
    assert!(open_dm(&ct, &nonce, &payload, &sk2).is_err());
}

#[test]
fn group_envelope_seal_open_roundtrip() {
    let key = "ab".repeat(32);
    let data = b"group media";
    let env = seal_group(data, &key).unwrap();
    let v: serde_json::Value = serde_json::from_str(&env).unwrap();
    assert_eq!(v["v"], 1);
    assert!(v["payload"].as_str().is_some());
    let opened = open_group(&env, &key).unwrap();
    assert_eq!(opened, data);
}

#[test]
fn group_envelope_wrong_key_or_bad_hex_fails() {
    let key = "ab".repeat(32);
    let env = seal_group(b"data", &key).unwrap();
    assert!(open_group(&env, &"cd".repeat(32)).is_err());
    assert!(seal_group(b"data", "badhex").is_err());
    assert!(seal_group(b"data", &"ab".repeat(31)).is_err());
    assert!(open_group("not json", &key).is_err());
    assert!(open_group(r#"{"v":1}"#, &key).is_err());
}

#[test]
fn dm_envelope_tampered_ciphertext_fails() {
    let (pk, sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let (ct, nonce, payload) = seal_dm(b"media", &pk).unwrap();
    let mut ct_bytes = hex::decode(&ct).unwrap();
    ct_bytes[0] ^= 0xFF;
    let tampered_ct = hex::encode(ct_bytes);
    assert!(open_dm(&tampered_ct, &nonce, &payload, &sk).is_err());
    let mut payload_chars: Vec<char> = payload.chars().collect();
    payload_chars[0] = if payload_chars[0] == 'A' { 'B' } else { 'A' };
    let tampered_payload: String = payload_chars.into_iter().collect();
    assert!(open_dm(&ct, &nonce, &tampered_payload, &sk).is_err());
}

#[test]
fn group_envelope_tampered_payload_fails() {
    let key = "ab".repeat(32);
    let env = seal_group(b"data", &key).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&env).unwrap();
    let payload = v["payload"].as_str().unwrap().to_string();
    let mut payload_bytes = payload.into_bytes();
    payload_bytes[3] ^= 0x01;
    v["payload"] = String::from_utf8(payload_bytes).unwrap().into();
    assert!(open_group(&v.to_string(), &key).is_err());
}

// ---------------------------------------------------------------------------
// Image decoder
// ---------------------------------------------------------------------------

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x60,
    0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0xA5, 0xF6, 0x45, 0x40, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[test]
fn decode_to_rgba_decodes_tiny_png() {
    let frame = decode_to_rgba(TINY_PNG, None, None).unwrap();
    assert_eq!(frame.width, 1);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.pixels.len(), 4);
    let frame = decode_to_rgba(TINY_PNG, Some(64), Some(64)).unwrap();
    assert_eq!((frame.width, frame.height), (1, 1));
}

#[test]
fn decode_to_rgba_rejects_junk_bytes() {
    assert!(decode_to_rgba(b"not an image at all", None, None).is_err());
    assert!(decode_to_rgba(&[], Some(64), Some(64)).is_err());
}

// ---------------------------------------------------------------------------
// Prefetcher
// ---------------------------------------------------------------------------

#[test]
fn prefetcher_prefetches_window_at_rest() {
    let prefetcher = Prefetcher::new();
    prefetcher.update_scroll_telemetry(0.0, 10, 20);
    assert!(prefetcher.should_prefetch_media(10));
    assert!(prefetcher.should_prefetch_media(20));
    assert!(prefetcher.should_prefetch_media(25));
    assert!(!prefetcher.should_prefetch_media(26));
    assert!(!prefetcher.should_prefetch_media(9));
}

#[test]
fn prefetcher_disables_on_fast_scroll() {
    let prefetcher = Prefetcher::new();
    prefetcher.update_scroll_telemetry(2000.0, 0, 5);
    assert!(!prefetcher.should_prefetch_media(0));
    prefetcher.update_scroll_telemetry(-1600.0, 0, 5);
    assert!(!prefetcher.should_prefetch_media(5));
    prefetcher.update_scroll_telemetry(100.0, 0, 5);
    assert!(prefetcher.should_prefetch_media(5));
}

#[test]
fn media_file_struct_derives() {
    use soshal_media_core::MediaFile;
    let mf = MediaFile {
        url: "https://x/a.png".into(),
        sha256: "sha256hex".into(),
        size: 1024,
        mime_type: "image/png".into(),
        created_at: 100,
    };
    let json_str = serde_json::to_string(&mf).unwrap();
    let deserialized: MediaFile = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.url, mf.url);
    assert_eq!(deserialized.sha256, mf.sha256);
}
