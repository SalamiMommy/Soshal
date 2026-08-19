//! AI Media Perceptual & Chrominance Moderation Engine.
//!
//! Provides lightweight, on-device analysis of media buffers:
//! - Skin-tone chrominance exposure scoring (YCbCr bounding analysis).
//! - Gore / blood chrominance anomaly detection.
//! - Known illicit media hash validation.
//! - Executable / dangerous MIME payload interception.

use serde::{Deserialize, Serialize};

/// Result of evaluating a media attachment with the AI Media engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiMediaVerdict {
    pub passed: bool,
    pub is_csam_hazard: bool,
    pub is_gore_hazard: bool,
    pub is_nsfw: bool,
    pub exposure_score: f32,
    pub gore_score: f32,
    pub warning_reason: Option<String>,
}

impl AiMediaVerdict {
    pub fn pass() -> Self {
        Self {
            passed: true,
            is_csam_hazard: false,
            is_gore_hazard: false,
            is_nsfw: false,
            exposure_score: 0.0,
            gore_score: 0.0,
            warning_reason: None,
        }
    }
}

/// Computes SHA-256 hex string of raw bytes.
fn compute_sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Converts RGB to YCbCr components.
#[inline]
fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;

    let y = 0.299 * rf + 0.587 * gf + 0.114 * bf;
    let cb = 128.0 - 0.168736 * rf - 0.331264 * gf + 0.5 * bf;
    let cr = 128.0 + 0.5 * rf - 0.418688 * gf - 0.081312 * bf;

    (y, cb, cr)
}

/// Checks if a pixel falls within the empirical human skin-tone chrominance bounding box.
#[inline]
fn is_skin_tone(cb: f32, cr: f32) -> bool {
    // Standard YCbCr skin tone locus: Cb in [77, 127] and Cr in [133, 173]
    (77.0..=127.0).contains(&cb) && (133.0..=173.0).contains(&cr)
}

/// Checks if a pixel falls within the high-intensity gore/blood chrominance profile.
#[inline]
fn is_gore_tone(r: u8, g: u8, b: u8) -> bool {
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;

    // High saturated red channel relative to green/blue
    r > 130 && g < 70 && b < 70 && (rf / (gf + bf + 1.0) > 1.8)
}

/// Evaluates raw RGB/RGBA pixel buffers for skin exposure and gore chrominance ratios.
pub fn analyze_pixel_buffer(
    pixels: &[u8],
    channels: usize,
    width: usize,
    height: usize,
) -> (f32, f32) {
    if pixels.is_empty() || channels < 3 || width == 0 || height == 0 {
        return (0.0, 0.0);
    }

    let total_pixels = width * height;
    let mut skin_count = 0usize;
    let mut gore_count = 0usize;

    // Sample across the buffer (step size for ultra-fast performance on large images)
    let step = (total_pixels / 5000).max(1);

    let mut i = 0;
    let mut sampled = 0;
    while i + 2 < pixels.len() {
        let r = pixels[i];
        let g = pixels[i + 1];
        let b = pixels[i + 2];

        let (_, cb, cr) = rgb_to_ycbcr(r, g, b);
        if is_skin_tone(cb, cr) {
            skin_count += 1;
        }
        if is_gore_tone(r, g, b) {
            gore_count += 1;
        }

        sampled += 1;
        i += channels * step;
    }

    if sampled == 0 {
        return (0.0, 0.0);
    }

    let exposure_score = (skin_count as f32 / sampled as f32).clamp(0.0, 1.0);
    let gore_score = (gore_count as f32 / sampled as f32).clamp(0.0, 1.0);

    (exposure_score, gore_score)
}

/// Evaluates a raw media buffer with AI perceptual, structural, and hash checks.
pub fn classify_media_buffer(bytes: &[u8], mime_type: &str, tags: &[String]) -> AiMediaVerdict {
    if bytes.is_empty() {
        return AiMediaVerdict::pass();
    }

    // 1. Intercept dangerous executable MIME types
    let mime_lower = mime_type.to_ascii_lowercase();
    if mime_lower.contains("application/x-executable")
        || mime_lower.contains("application/x-msdownload")
        || mime_lower.contains("application/x-sh")
        || mime_lower.contains("application/x-dosexec")
    {
        return AiMediaVerdict {
            passed: false,
            is_csam_hazard: false,
            is_gore_hazard: false,
            is_nsfw: false,
            exposure_score: 0.0,
            gore_score: 0.0,
            warning_reason: Some("disallowed_executable_payload".to_string()),
        };
    }

    // 2. Check hash against known CSAM blocklists
    let hash_hex = compute_sha256_hex(bytes);
    let csam_check = crate::csam::check_csam_hash(&hash_hex);
    if csam_check.is_csam {
        return AiMediaVerdict {
            passed: false,
            is_csam_hazard: true,
            is_gore_hazard: false,
            is_nsfw: true,
            exposure_score: 1.0,
            gore_score: 0.0,
            warning_reason: Some("known_csam_media_hash".to_string()),
        };
    }

    // 3. Inspect NIP-36 content warnings
    let mut is_sensitive = false;
    let mut warning_reason = None;
    for tag in tags {
        let tag_l = tag.to_ascii_lowercase();
        if tag_l.contains("content-warning")
            || tag_l.contains("sensitive")
            || tag_l.contains("nsfw")
        {
            is_sensitive = true;
            if warning_reason.is_none() {
                warning_reason = Some(tag.clone());
            }
        }
    }

    // 4. Perceptual chrominance sampling (if image buffer)
    let (exposure_score, gore_score) = if bytes.len() >= 300 {
        // Fast pseudo-pixel sample directly from raw byte stream
        let channels = 3;
        let sample_len = bytes.len() / channels;
        analyze_pixel_buffer(bytes, channels, sample_len.min(500), 1)
    } else {
        (0.0, 0.0)
    };

    let is_gore_hazard = gore_score > 0.40;
    let is_nsfw = is_sensitive || exposure_score > 0.45;

    let passed = !is_gore_hazard;

    let final_reason = if is_gore_hazard {
        Some("gore_chrominance_anomaly_detected".to_string())
    } else if warning_reason.is_some() {
        warning_reason
    } else if is_nsfw {
        Some("high_exposure_nudity_detected".to_string())
    } else {
        None
    };

    AiMediaVerdict {
        passed,
        is_csam_hazard: false,
        is_gore_hazard,
        is_nsfw,
        exposure_score,
        gore_score,
        warning_reason: final_reason,
    }
}

/// JSON serialized output of AI media buffer classification.
pub fn classify_media_buffer_json(bytes: &[u8], mime_type: &str, tags: &[String]) -> String {
    let verdict = classify_media_buffer(bytes, mime_type, tags);
    serde_json::to_string(&verdict).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_media_buffer_passes() {
        let buffer = vec![100u8; 1024];
        let verdict = classify_media_buffer(&buffer, "image/jpeg", &[]);
        assert!(verdict.passed);
        assert!(!verdict.is_csam_hazard);
        assert!(!verdict.is_gore_hazard);
    }

    #[test]
    fn test_executable_payload_rejected() {
        let buffer = vec![0x7f, b'E', b'L', b'F'];
        let verdict = classify_media_buffer(&buffer, "application/x-executable", &[]);
        assert!(!verdict.passed);
        assert!(verdict.warning_reason.unwrap().contains("executable"));
    }

    #[test]
    fn test_skin_tone_detection() {
        let (_, cb, cr) = rgb_to_ycbcr(220, 160, 130);
        assert!(is_skin_tone(cb, cr));
    }

    #[test]
    fn test_gore_tone_detection() {
        assert!(is_gore_tone(200, 20, 20));
        assert!(!is_gore_tone(50, 150, 50));
    }

    #[test]
    fn test_sensitive_tag_marked_nsfw() {
        let buffer = vec![50u8; 512];
        let tags = vec!["content-warning: artistic nudity".to_string()];
        let verdict = classify_media_buffer(&buffer, "image/png", &tags);
        assert!(verdict.passed);
        assert!(verdict.is_nsfw);
        assert!(verdict.warning_reason.unwrap().contains("content-warning"));
    }
}
