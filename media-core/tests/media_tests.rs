//! Integration tests for soshal-media-core.

use soshal_media_core::decoder::decode_to_rgba;
use soshal_media_core::freenet_media::{chunk_media, reconstruct_media, verify_chunk, CHUNK_SIZE};
use soshal_media_core::media::guess_mime_type;
use soshal_media_core::prefetcher::Prefetcher;

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
