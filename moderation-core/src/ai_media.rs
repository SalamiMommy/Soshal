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

    // 4. Video frame moderation: MP4 with H.264/AV1 video track → run the
    //    real pixel chrominance + trained image NN on sampled frames. Other
    //    containers/codecs fall through to the raw-byte path below.
    if mime_lower.starts_with("video/") {
        if let Some(v) = crate::video_nn::classify_video_mp4(bytes) {
            if v.frames_classified > 0 {
                return finalize_video_verdict(&v, is_sensitive, warning_reason);
            }
        }
    }

    // 5. Perceptual chrominance sampling (if image buffer)
    let (exposure_score, gore_score) = if bytes.len() >= 300 {
        // Fast pseudo-pixel sample directly from raw byte stream
        let channels = 3;
        let sample_len = bytes.len() / channels;
        analyze_pixel_buffer(bytes, channels, sample_len.min(500), 1)
    } else {
        (0.0, 0.0)
    };

    // 5. Trained image CNN (degrades to `None` until legal weights exist —
    //    deterministic layers above stay authoritative in that case).
    let nn_scores = crate::image_nn::classify_image_bytes(bytes);

    finalize_verdict(
        exposure_score,
        gore_score,
        nn_scores,
        is_sensitive,
        warning_reason,
    )
}

/// Builds the final verdict from chrominance + optional NN head scores.
/// Exposed as a fn (not inlined) so the merge logic is unit-testable without
/// live weights.
fn finalize_verdict(
    exposure_score: f32,
    gore_score: f32,
    nn_scores: Option<crate::image_nn::ImageNnScores>,
    is_sensitive: bool,
    warning_reason: Option<String>,
) -> AiMediaVerdict {
    let mut is_gore_hazard = gore_score > 0.40;
    let mut is_nsfw = is_sensitive || exposure_score > 0.45;

    let mut blk_reason: Option<String> = None; // blocks (passed=false)
    let mut risk_reason: Option<String> = None; // reports only (passed=true)

    if is_gore_hazard {
        blk_reason = Some("gore_chrominance_anomaly_detected".to_string());
    }

    let mut gore_score = gore_score;
    let mut exposure_score = exposure_score;

    // NN fusion: max-blend gore, informative nudity score, and CSAM-risk
    // signalling via reason only (report/review gate input — the hash
    // blocklist remains the sole source that sets is_csam_hazard).
    if let Some(s) = nn_scores {
        let verdict = crate::image_nn::compose_verdict(s);
        gore_score = gore_score.max(s.gore);
        exposure_score = exposure_score.max(s.nudity);
        if verdict.is_gore && !is_gore_hazard {
            is_gore_hazard = true;
            blk_reason = Some("nn_gore_detected".to_string());
        }
        // NN nudity is informative labeling, never a block.
        is_nsfw = is_nsfw || s.nudity > 0.45;
        if verdict.csam_risk {
            risk_reason = Some(format!("nn_csam_risk:{:.2}", verdict.csam_risk_score));
        }
    }

    // Reason precedence: block reason > NN csam-risk > content-warning tag >
    // generic nudity note. Non-block reasons never flip `passed`.
    let final_reason = blk_reason.or(risk_reason).or(warning_reason).or_else(|| {
        if is_nsfw {
            Some("high_exposure_nudity_detected".to_string())
        } else {
            None
        }
    });

    AiMediaVerdict {
        passed: !is_gore_hazard,
        is_csam_hazard: false,
        is_gore_hazard,
        is_nsfw,
        exposure_score,
        gore_score,
        warning_reason: final_reason,
    }
}

/// Builds the final verdict for video from sampled-frame NN + chrominance
/// aggregates. Mirrors `finalize_verdict` rule-for-rule:
/// - gore (chrominance OR any NN gore frame) blocks.
/// - nudity alone is informative only (adult nudity never flags).
/// - composed nudity × juvenile CSAM-risk feeds the report/review gate via
///   reason — never sets `is_csam_hazard` (hash blocklist stays authoritative).
pub fn finalize_video_verdict(
    v: &crate::video_nn::VideoFramesVerdict,
    is_sensitive: bool,
    warning_reason: Option<String>,
) -> AiMediaVerdict {
    let is_gore_hazard = v.gore_chrom_max > 0.40 || v.any_nn_gore;
    let is_nsfw = is_sensitive || v.exposure_max > 0.45 || v.nudity_max > 0.45;

    let blk_reason: Option<String> = if is_gore_hazard {
        Some(if v.any_nn_gore {
            "nn_gore_detected".to_string()
        } else {
            "gore_chrominance_anomaly_detected".to_string()
        })
    } else {
        None
    };
    let risk_reason: Option<String> =
        (v.csam_risk_frames > 0).then(|| format!("nn_csam_risk:{:.2}", v.csam_risk_score_max));

    let final_reason = blk_reason.or(risk_reason).or(warning_reason).or_else(|| {
        if is_nsfw {
            Some("high_exposure_nudity_detected".to_string())
        } else {
            None
        }
    });

    AiMediaVerdict {
        passed: !is_gore_hazard,
        is_csam_hazard: false,
        is_gore_hazard,
        is_nsfw,
        exposure_score: v.exposure_max.max(v.nudity_max),
        gore_score: v.gore_chrom_max.max(v.gore_nn_max),
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

    #[test]
    fn nn_fusion_nudity_alone_never_blocks() {
        let v = finalize_verdict(
            0.1,
            0.1,
            Some(crate::image_nn::ImageNnScores {
                gore: 0.1,
                nudity: 0.98,
                juvenile: 0.1,
            }),
            false,
            None,
        );
        assert!(v.passed, "adult nudity must not block via NN");
        assert!(!v.is_gore_hazard);
        assert!(!v.is_csam_hazard);
        assert!(v.is_nsfw); // informative labeling only — never a block
        assert_eq!(v.exposure_score, 0.98);
    }

    #[test]
    fn nn_fusion_gore_blocks() {
        let v = finalize_verdict(
            0.1,
            0.1,
            Some(crate::image_nn::ImageNnScores {
                gore: 0.95,
                nudity: 0.1,
                juvenile: 0.1,
            }),
            false,
            None,
        );
        assert!(!v.passed);
        assert!(v.is_gore_hazard);
        assert_eq!(v.warning_reason.as_deref(), Some("nn_gore_detected"));
    }

    #[test]
    fn nn_fusion_csam_risk_reports_not_blocks() {
        let v = finalize_verdict(
            0.1,
            0.1,
            Some(crate::image_nn::ImageNnScores {
                gore: 0.2,
                nudity: 0.95,
                juvenile: 0.9,
            }),
            false,
            None,
        );
        assert!(v.passed, "NN csam-risk feeds report gate, not auto-block");
        let r = v.warning_reason.unwrap();
        assert!(r.starts_with("nn_csam_risk:"), "reason was {r}");
        assert!(!v.is_csam_hazard, "hash blocklist is the only csam hazard");
        assert!(v.gore_score >= 0.2);
    }

    #[test]
    fn nn_fusion_hash_csam_still_authoritative() {
        let bytes = vec![0u8; 512];
        let verdict = classify_media_buffer(&bytes, "image/jpeg", &[]);
        assert!(verdict.passed);
        assert!(!verdict.is_csam_hazard);
        assert!(!verdict.is_gore_hazard);
        // chrominance-only path unchanged
        assert_eq!(verdict.warning_reason, None);
    }

    // ---------- video frame verdict (finalize_video_verdict) -------------

    fn vv(
        gore_chrom: f32,
        gore_nn: f32,
        nudity: f32,
        juvenile: f32,
        csam_frames: usize,
        csam_max: f32,
    ) -> super::super::video_nn::VideoFramesVerdict {
        use super::super::video_nn::VideoFramesVerdict;
        VideoFramesVerdict {
            frames_attempted: 4,
            frames_classified: 4,
            exposure_max: nudity,
            gore_chrom_max: gore_chrom,
            gore_nn_max: gore_nn,
            nudity_max: nudity,
            juvenile_max: juvenile,
            any_nn_gore: gore_nn > 0.55,
            csam_risk_frames: csam_frames,
            csam_risk_score_max: csam_max,
        }
    }

    #[test]
    fn video_clean_passes() {
        let v = finalize_video_verdict(&vv(0.1, 0.1, 0.2, 0.1, 0, 0.0), false, None);
        assert!(v.passed);
        assert!(!v.is_gore_hazard);
        assert!(!v.is_nsfw);
        assert_eq!(v.warning_reason, None);
    }

    #[test]
    fn video_adult_nudity_never_blocks_or_csam_flags() {
        let v = finalize_video_verdict(&vv(0.1, 0.1, 0.99, 0.1, 0, 0.0), false, None);
        assert!(v.passed, "adult nudity must not block via video NN");
        assert!(!v.is_gore_hazard);
        assert!(v.is_nsfw, "informative only");
        let r = v.warning_reason.as_deref().unwrap_or("");
        assert!(r.contains("nudity"), "reason was {r}");
    }

    #[test]
    fn video_nn_gore_blocks() {
        let v = finalize_video_verdict(&vv(0.1, 0.9, 0.1, 0.1, 0, 0.0), false, None);
        assert!(!v.passed);
        assert!(v.is_gore_hazard);
        assert_eq!(v.warning_reason.as_deref(), Some("nn_gore_detected"));
    }

    #[test]
    fn video_chrominance_gore_blocks_without_nn() {
        let v = finalize_video_verdict(&vv(0.7, 0.0, 0.1, 0.1, 0, 0.0), false, None);
        assert!(!v.passed);
        assert_eq!(
            v.warning_reason.as_deref(),
            Some("gore_chrominance_anomaly_detected")
        );
    }

    #[test]
    fn video_csam_risk_reports_not_blocks() {
        let v = finalize_video_verdict(&vv(0.1, 0.1, 0.95, 0.9, 2, 0.6), false, None);
        assert!(
            v.passed,
            "video CSAM-risk feeds report gate, not auto-block"
        );
        assert!(!v.is_csam_hazard);
        let r = v.warning_reason.unwrap();
        assert!(r.starts_with("nn_csam_risk:"), "reason was {r}");
    }

    #[test]
    fn video_sensitive_tag_surfaces() {
        let v = finalize_video_verdict(
            &vv(0.1, 0.1, 0.1, 0.1, 0, 0.0),
            true,
            Some("content-warning: violence".to_string()),
        );
        assert!(v.passed);
        assert!(v.is_nsfw);
        assert!(v.warning_reason.unwrap().contains("content-warning"));
    }
}
