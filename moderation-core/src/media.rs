//! Media Moderation Engine.
//!
//! Evaluates media attachments (blob hashes, URLs, MIME types, and NIP-36 tags)
//! against known illegal CSAM blocklists, gore/shock-site domains, and sensitive content warnings.

use crate::csam::check_csam_hash;
use crate::gore::check_gore_text;

/// Result of evaluating a media attachment.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MediaModerationVerdict {
    pub passed: bool,
    pub is_csam: bool,
    pub is_gore: bool,
    pub is_sensitive: bool,
    pub warning_reason: Option<String>,
}

/// Evaluates a media attachment by its hash, URL, MIME type, and Nostr tags.
pub fn check_media_item(
    blob_hash: &str,
    url: Option<&str>,
    mime_type: &str,
    tags: &[String],
) -> MediaModerationVerdict {
    // 1. Check illegal CSAM hash blocklist
    let csam_res = check_csam_hash(blob_hash);
    if csam_res.is_csam {
        return MediaModerationVerdict {
            passed: false,
            is_csam: true,
            is_gore: false,
            is_sensitive: true,
            warning_reason: Some("csam_media_hash_blocklist".to_string()),
        };
    }

    // 2. Check URL against CSAM and Gore shock domains
    if let Some(media_url) = url {
        let csam_url_res = crate::csam::check_csam_text(media_url);
        if csam_url_res.is_csam {
            return MediaModerationVerdict {
                passed: false,
                is_csam: true,
                is_gore: false,
                is_sensitive: true,
                warning_reason: csam_url_res.rule,
            };
        }

        let gore_url_res = check_gore_text(media_url);
        if gore_url_res.is_gore {
            return MediaModerationVerdict {
                passed: false,
                is_csam: false,
                is_gore: true,
                is_sensitive: true,
                warning_reason: gore_url_res.rule,
            };
        }
    }

    // 3. Inspect NIP-36 content-warning / sensitive tags
    let mut is_sensitive = false;
    let mut warning_reason = None;

    for tag in tags {
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.contains("content-warning")
            || tag_lower.contains("sensitive")
            || tag_lower.contains("nsfw")
        {
            is_sensitive = true;
            if warning_reason.is_none() {
                warning_reason = Some(tag.clone());
            }
        }
    }

    // Disallow dangerous executable MIME types masquerading as media
    let mime_lower = mime_type.to_ascii_lowercase();
    if mime_lower.contains("application/x-executable")
        || mime_lower.contains("application/x-msdownload")
        || mime_lower.contains("application/x-sh")
    {
        return MediaModerationVerdict {
            passed: false,
            is_csam: false,
            is_gore: false,
            is_sensitive: false,
            warning_reason: Some("disallowed_mime_type".to_string()),
        };
    }

    MediaModerationVerdict {
        passed: true,
        is_csam: false,
        is_gore: false,
        is_sensitive,
        warning_reason,
    }
}

/// Evaluates raw media buffer with AI perceptual, structural, and hash checks.
pub fn check_media_buffer_ai(
    bytes: &[u8],
    mime_type: &str,
    tags: &[String],
) -> crate::ai_media::AiMediaVerdict {
    crate::ai_media::classify_media_buffer(bytes, mime_type, tags)
}

/// JSON serialized output of AI media buffer evaluation.
pub fn check_media_buffer_ai_json(bytes: &[u8], mime_type: &str, tags: &[String]) -> String {
    crate::ai_media::classify_media_buffer_json(bytes, mime_type, tags)
}

/// 2-Tier Hybrid media evaluation returning JSON string.
pub fn check_media_hybrid_json(bytes: &[u8], mime_type: &str, tags: &[String]) -> String {
    crate::hybrid::evaluate_media_hybrid_json(bytes, mime_type, tags)
}

/// Compute 256-bit PDQ perceptual hash from raw image bytes.
pub fn compute_image_pdq_hash(bytes: &[u8]) -> Option<crate::pdq::PdqHashResult> {
    crate::pdq::evaluate_media_pdq(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_media_passes() {
        let v = check_media_item(
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            Some("https://example.com/photo.jpg"),
            "image/jpeg",
            &[],
        );
        assert!(v.passed);
        assert!(!v.is_csam);
        assert!(!v.is_gore);
    }

    #[test]
    fn test_csam_hash_blocked() {
        let v = check_media_item(
            "c27a20ff44e8bc1a3b1a8d052d9a6c4df103c80a2b0e8b1ef380b0b8e8f85f31",
            Some("https://example.com/blob.bin"),
            "image/png",
            &[],
        );
        assert!(!v.passed);
        assert!(v.is_csam);
    }

    #[test]
    fn test_shock_domain_blocked() {
        let v = check_media_item(
            "1111111111111111111111111111111111111111111111111111111111111111",
            Some("https://bestgore.com/video.mp4"),
            "video/mp4",
            &[],
        );
        assert!(!v.passed);
        assert!(v.is_gore);
    }

    #[test]
    fn test_sensitive_tag_detected() {
        let v = check_media_item(
            "2222222222222222222222222222222222222222222222222222222222222222",
            Some("https://example.com/art.png"),
            "image/png",
            &["content-warning: artistic nudity".to_string()],
        );
        assert!(v.passed);
        assert!(v.is_sensitive);
        assert_eq!(
            v.warning_reason.as_deref(),
            Some("content-warning: artistic nudity")
        );
    }
}
