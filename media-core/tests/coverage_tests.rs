use soshal_media_core::chunking::{chunk_reader_with_data_params, ChunkingParams};
use soshal_media_core::decoder::detect_image_format;
use soshal_media_core::freenet_media::{
    chunk_media_json, reconstruct_media_json, verify_chunk_json,
};
use soshal_media_core::thumbhash::encode_thumbhash_from_bytes;
use soshal_media_core::trim_media_caches;

const TINY_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn tiny_png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(TINY_PNG_B64)
        .unwrap()
}

#[test]
fn trim_media_caches_noop_all_levels() {
    use soshal_common_core::memory::MemoryPressureLevel;
    trim_media_caches(MemoryPressureLevel::Normal);
    trim_media_caches(MemoryPressureLevel::Moderate);
    trim_media_caches(MemoryPressureLevel::Critical);
}

#[test]
fn detect_image_format_magic_bytes() {
    let png = tiny_png();
    assert_eq!(detect_image_format(&png), Some(image::ImageFormat::Png));
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    assert_eq!(detect_image_format(&jpeg), Some(image::ImageFormat::Jpeg));
    assert_eq!(detect_image_format(b""), None);
    assert_eq!(detect_image_format(b"plain text"), None);
}

#[test]
fn encode_thumbhash_from_bytes_valid_and_invalid() {
    let thumb = encode_thumbhash_from_bytes(&tiny_png()).unwrap();
    assert!(!thumb.is_empty(), "thumbhash encodes a tiny png");
    assert!(thumb.len() <= 64, "thumbhash stays small");
    let e = encode_thumbhash_from_bytes(b"not an image").unwrap_err();
    assert!(!e.is_empty());
    let e = encode_thumbhash_from_bytes(b"").unwrap_err();
    assert!(!e.is_empty());
}

#[test]
fn chunking_params_for_mime() {
    let video = ChunkingParams::for_mime("video/mp4");
    assert_eq!(video.min, 1024 * 1024);
    assert_eq!(video.avg, 4 * 1024 * 1024);
    assert_eq!(video.max, 16 * 1024 * 1024);
    let audio = ChunkingParams::for_mime("audio/ogg");
    assert_eq!(audio.min, 32 * 1024);
    assert_eq!(audio.avg, 128 * 1024);
    assert_eq!(audio.max, 512 * 1024);
    let other = ChunkingParams::for_mime("text/plain");
    assert_eq!(other, ChunkingParams::default());
    let no_mime = ChunkingParams::for_mime("");
    assert_eq!(no_mime, ChunkingParams::default());
}

#[test]
fn chunk_reader_with_data_params_roundtrip() {
    let data: Vec<u8> = (0..=255).cycle().take(300_000).collect();
    let (manifest, chunks) =
        chunk_reader_with_data_params(std::io::Cursor::new(&data), ChunkingParams::default())
            .unwrap();
    assert!(manifest.is_valid());
    assert_eq!(manifest.total_size, data.len() as u64);
    let mut rebuilt = Vec::with_capacity(data.len());
    for (_, chunk) in chunks {
        rebuilt.extend_from_slice(&chunk);
    }
    assert_eq!(rebuilt, data, "chunks reconstruct the source exactly");
    let mut expect_hex = [0u8; 32];
    expect_hex.copy_from_slice(blake3::hash(&data).as_bytes());
    assert_eq!(
        manifest.blob_hash,
        hex::encode(expect_hex),
        "manifest blob_hash matches source"
    );

    let small =
        chunk_reader_with_data_params(std::io::Cursor::new(b"tiny"), ChunkingParams::default())
            .unwrap();
    assert!(small.0.is_valid());
    assert_eq!(small.0.total_size, 4);
}

#[test]
fn freenet_chunk_json_roundtrip() {
    use soshal_crypto_core::base64::base64_encode_bytes;
    let raw = b"freenet json payload test";
    let b64 = base64_encode_bytes(raw);
    let chunked = chunk_media_json(&format!(r#"{{"dataB64":"{b64}"}}"#));
    let v: serde_json::Value = serde_json::from_str(&chunked).unwrap();
    assert_eq!(v["totalSize"], raw.len() as i64, "got {chunked}");
    assert!(v["contentHash"].is_string(), "got {chunked}");
    let content_hash = v["contentHash"].as_str().unwrap().to_string();
    let first_hash = v["chunkHashes"][0].as_str().unwrap().to_string();

    let verified = verify_chunk_json(&format!(
        r#"{{"dataB64":"{b64}","expectedHash":"{content_hash}"}}"#
    ));
    let v: serde_json::Value = serde_json::from_str(&verified).unwrap();
    assert_eq!(v["valid"], true, "got {verified}");

    let bad = verify_chunk_json(&format!(
        r#"{{"dataB64":"{b64}","expectedHash":"{}"}}"#,
        "a".repeat(64)
    ));
    let v: serde_json::Value = serde_json::from_str(&bad).unwrap();
    assert_eq!(v["valid"], false);

    let reconstructed = reconstruct_media_json(&format!(r#"{{"chunksB64":["{b64}"]}}"#));
    let v: serde_json::Value = serde_json::from_str(&reconstructed).unwrap();
    assert_eq!(v["dataB64"], b64, "got {reconstructed}");
    assert_eq!(v["totalSize"], raw.len() as i64);

    let bad_chunk = chunk_media_json("not-json");
    let v: serde_json::Value = serde_json::from_str(&bad_chunk).unwrap();
    assert!(v["error"].is_string(), "got {bad_chunk}");

    let bad_reconstruct = reconstruct_media_json("{}");
    let v: serde_json::Value = serde_json::from_str(&bad_reconstruct).unwrap();
    assert!(v["error"].is_string(), "got {bad_reconstruct}");
    assert_eq!(first_hash.len(), 64);
}
