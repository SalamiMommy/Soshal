// ─── Magic byte sniffing ─────────────────────────────────────────────

use crate::json_util::json_in;

const MAX_HEADER_BYTES: usize = 12;

/// Magic byte patterns: (prefix_bytes, mime_type).
const MAGIC_PATTERNS: &[(&[u8], &str)] = &[
    (&[0xff, 0xd8, 0xff], "image/jpeg"),
    (
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
        "image/png",
    ),
    (&[0x47, 0x49, 0x46, 0x38], "image/gif"), // GIF87a or GIF89a
    (&[0x49, 0x44, 0x33], "audio/mpeg"),      // ID3
    (&[0xff, 0xfb], "audio/mpeg"),            // MPEG audio
    (&[0x4f, 0x67, 0x67, 0x53], "audio/ogg"), // OggS
    (&[0x66, 0x4c, 0x61, 0x43], "audio/flac"), // fLaC
];

/// Sniffs MIME type from raw bytes header (first 12 bytes max).
/// Falls back to the `fallback` param for image/video/audio, otherwise
/// returns `application/octet-stream`.
pub fn sniff_mime_type(bytes: &[u8], fallback: &str) -> String {
    if bytes.is_empty() {
        return "application/octet-stream".to_string();
    }
    let header_len = bytes.len().min(MAX_HEADER_BYTES);
    let header = &bytes[..header_len];

    for &(magic, mime) in MAGIC_PATTERNS {
        if header.starts_with(magic) {
            return mime.to_string();
        }
    }

    // Check WEBP (RIFF....WEBP)
    if header.len() >= 12
        && header[0..4] == [0x52, 0x49, 0x46, 0x46] // RIFF
        && header[8..12] == [0x57, 0x45, 0x42, 0x50]
    // WEBP
    {
        return "image/webp".to_string();
    }

    // Check MP4 (....ftyp)
    if header.len() >= 8 && &header[4..8] == b"ftyp" {
        return "video/mp4".to_string();
    }

    // Check WebM (matroska)
    if header.len() >= 4 && header[0..4] == [0x1a, 0x45, 0xdf, 0xa3] {
        return "video/webm".to_string();
    }

    // Check QuickTime (moov or mdat at offset 4)
    if header.len() >= 8 {
        let fourcc = &header[4..8];
        if fourcc == b"moov" || fourcc == b"mdat" {
            return "video/quicktime".to_string();
        }
    }

    // Check WAV (RIFF....WAVE)
    if header.len() >= 12
        && header[0..4] == [0x52, 0x49, 0x46, 0x46] // RIFF
        && header[8..12] == [0x57, 0x41, 0x56, 0x45]
    // WAVE
    {
        return "audio/wav".to_string();
    }

    // Fallback validation
    if let Some(cat) = fallback.split('/').next() {
        if (cat == "image" || cat == "video" || cat == "audio") && bytes.len() >= 4 {
            return fallback.to_string();
        }
    }

    "application/octet-stream".to_string()
}

/// Input for sniff_mime_type_wasm.
#[derive(serde::Deserialize)]
struct SniffInput {
    bytes: Vec<u8>,
    fallback: String,
}

pub fn sniff_mime_type_json(input: &str) -> String {
    let input = json_in(
        input,
        SniffInput {
            bytes: Vec::new(),
            fallback: String::new(),
        },
    );
    sniff_mime_type(&input.bytes, &input.fallback)
}

// ─── Extension-based detection ────────────────────────────────────────

/// Resolves a MIME type from a file extension.
pub fn detect_mime_type(filename: &str) -> String {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "gif" => "image/gif".to_string(),
        "webp" => "image/webp".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "avif" => "image/avif".to_string(),
        "heic" | "heif" => "image/heic".to_string(),
        "bmp" => "image/bmp".to_string(),
        "ico" => "image/x-icon".to_string(),
        "mp4" => "video/mp4".to_string(),
        "webm" => "video/webm".to_string(),
        "mov" => "video/quicktime".to_string(),
        "avi" => "video/x-msvideo".to_string(),
        "mkv" => "video/x-matroska".to_string(),
        "ogg" => "video/ogg".to_string(),
        "mp3" => "audio/mpeg".to_string(),
        "wav" => "audio/wav".to_string(),
        "flac" => "audio/flac".to_string(),
        "aac" => "audio/aac".to_string(),
        "m4a" => "audio/mp4".to_string(),
        "opus" => "audio/opus".to_string(),
        "wma" => "audio/x-ms-wma".to_string(),
        "pdf" => "application/pdf".to_string(),
        "json" => "application/json".to_string(),
        "txt" => "text/plain".to_string(),
        "html" | "htm" => "text/html".to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn via_json(bytes: &[u8], fallback: &str) -> String {
        sniff_mime_type_json(
            &serde_json::to_string(&serde_json::json!({"bytes": bytes, "fallback": fallback}))
                .unwrap(),
        )
    }

    #[test]
    fn test_json_png() {
        assert_eq!(
            via_json(
                &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01],
                ""
            ),
            "image/png"
        );
    }

    #[test]
    fn test_json_jpeg() {
        assert_eq!(via_json(&[0xff, 0xd8, 0xff, 0xe0], ""), "image/jpeg");
    }

    #[test]
    fn test_json_gif() {
        assert_eq!(via_json(b"GIF89a", ""), "image/gif");
    }

    #[test]
    fn test_json_html_text() {
        assert_eq!(via_json(b"<html>", "text/html"), "application/octet-stream");
        assert_eq!(
            via_json(b"<html>pretty long", "text/html"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_json_empty_garbage() {
        assert_eq!(via_json(&[], "image/png"), "application/octet-stream");
        assert_eq!(via_json(b"\x00\x01\x02garbage", "image/png"), "image/png");
    }

    #[test]
    fn test_json_fallback_category() {
        assert_eq!(
            via_json(b"\x01\x02\x03\x04", "audio/x-custom"),
            "audio/x-custom"
        );
        assert_eq!(
            via_json(b"\x01", "audio/x-custom"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_json_invalid_input() {
        assert_eq!(sniff_mime_type_json("not json"), "application/octet-stream");
    }
}
