use serde::{Deserialize, Serialize};

/// High-level media category determined by magic-byte signature analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlobMediaType {
    Image(String),
    Video(String),
    Audio(String),
    Binary(String),
    Unknown,
}

/// Detect the precise MIME type of raw data by probing magic bytes.
/// Returns standard MIME string (e.g. `"image/jpeg"`, `"video/mp4"`).
pub fn detect_mime_type(bytes: &[u8]) -> Option<&'static str> {
    infer::get(bytes).map(|t| t.mime_type())
}

/// Returns the suggested file extension (e.g. `"jpg"`, `"png"`, `"mp4"`) based on magic bytes.
pub fn detect_extension(bytes: &[u8]) -> Option<&'static str> {
    infer::get(bytes).map(|t| t.extension())
}

/// Check if raw data starts with a recognized image magic-byte signature.
pub fn is_image(bytes: &[u8]) -> bool {
    infer::is_image(bytes)
}

/// Check if raw data starts with a recognized video magic-byte signature.
pub fn is_video(bytes: &[u8]) -> bool {
    infer::is_video(bytes)
}

/// Check if raw data starts with a recognized audio magic-byte signature.
pub fn is_audio(bytes: &[u8]) -> bool {
    infer::is_audio(bytes)
}

/// Classify a raw byte payload into a high-level `BlobMediaType`.
pub fn classify_blob(bytes: &[u8]) -> BlobMediaType {
    if let Some(kind) = infer::get(bytes) {
        let mime = kind.mime_type().to_string();
        let matcher_type = kind.matcher_type();
        match matcher_type {
            infer::MatcherType::Image => BlobMediaType::Image(mime),
            infer::MatcherType::Video => BlobMediaType::Video(mime),
            infer::MatcherType::Audio => BlobMediaType::Audio(mime),
            _ => BlobMediaType::Binary(mime),
        }
    } else {
        BlobMediaType::Unknown
    }
}

/// Validates that a byte payload does not contradict a claimed MIME type.
///
/// Returns `true` if:
/// - The detected MIME matches `claimed_mime` (case-insensitive)
/// - Or the payload is too short/unrecognized but does NOT match a known contradictory format
///
/// Returns `false` if:
/// - A dangerous spoofing attempt is detected (e.g. claimed "image/png" but bytes are an ELF or WASM)
/// - A detected image/video/audio format mismatches the claimed category
pub fn verify_claimed_mime(bytes: &[u8], claimed_mime: &str) -> bool {
    let clean_claimed = claimed_mime.trim().to_ascii_lowercase();
    if clean_claimed.is_empty() {
        return false;
    }

    if let Some(detected) = detect_mime_type(bytes) {
        if detected.eq_ignore_ascii_case(&clean_claimed) {
            return true;
        }

        // Check if both are in the same media category (e.g. image/jpg vs image/jpeg)
        let detected_cat = detected.split('/').next().unwrap_or("");
        let claimed_cat = clean_claimed.split('/').next().unwrap_or("");
        if !detected_cat.is_empty() && detected_cat == claimed_cat {
            // E.g. image/jpg vs image/jpeg is acceptable
            if (clean_claimed == "image/jpg" && detected == "image/jpeg")
                || (clean_claimed == "image/jpeg" && detected == "image/jpg")
            {
                return true;
            }
        }

        // Definite mismatch where detected type is known
        return false;
    }

    // If infer cannot identify the type, ensure it doesn't match an executable/archive when claiming image
    if clean_claimed.starts_with("image/")
        || clean_claimed.starts_with("audio/")
        || clean_claimed.starts_with("video/")
    {
        // If claimed media but infer says it's an app or executable, reject
        if infer::is_app(bytes) || infer::is_archive(bytes) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_image_png() {
        // PNG 8-byte magic: 89 50 4E 47 0D 0A 1A 0A
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        assert_eq!(detect_mime_type(&png_bytes), Some("image/png"));
        assert_eq!(detect_extension(&png_bytes), Some("png"));
        assert!(is_image(&png_bytes));
        assert!(!is_video(&png_bytes));
        assert!(!is_audio(&png_bytes));

        assert_eq!(
            classify_blob(&png_bytes),
            BlobMediaType::Image("image/png".to_string())
        );

        assert!(verify_claimed_mime(&png_bytes, "image/png"));
        // Reject spoofed claim
        assert!(!verify_claimed_mime(&png_bytes, "application/pdf"));
    }

    #[test]
    fn test_detect_image_jpeg() {
        // JPEG magic: FF D8 FF
        let jpeg_bytes = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00,
        ];
        assert_eq!(detect_mime_type(&jpeg_bytes), Some("image/jpeg"));
        assert!(is_image(&jpeg_bytes));
        assert!(verify_claimed_mime(&jpeg_bytes, "image/jpeg"));
        assert!(verify_claimed_mime(&jpeg_bytes, "image/jpg"));
        assert!(!verify_claimed_mime(&jpeg_bytes, "video/mp4"));
    }

    #[test]
    fn test_spoofing_detection() {
        // WASM binary header: \0 a s m 01 00 00 00
        let wasm_bytes = [0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(detect_mime_type(&wasm_bytes), Some("application/wasm"));
        // Claimed to be an image, but it's a wasm binary
        assert!(!verify_claimed_mime(&wasm_bytes, "image/jpeg"));
        assert!(!verify_claimed_mime(&wasm_bytes, "image/png"));

        // PDF document: %PDF-
        let pdf_bytes = b"%PDF-1.5 some pdf data here";
        assert_eq!(detect_mime_type(pdf_bytes), Some("application/pdf"));
        assert!(!verify_claimed_mime(pdf_bytes, "image/png"));
        assert!(!verify_claimed_mime(pdf_bytes, "video/mp4"));
    }

    #[test]
    fn test_empty_buffer() {
        assert_eq!(detect_mime_type(&[]), None);
        assert!(!is_image(&[]));
        assert_eq!(classify_blob(&[]), BlobMediaType::Unknown);
        assert!(!verify_claimed_mime(&[], ""));
    }
}
