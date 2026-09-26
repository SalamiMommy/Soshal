//! Perceptual color analysis, dominant palette extraction, and WCAG contrast calculations.

use palette::color_difference::Wcag21RelativeContrast;
use palette::Srgb;
use serde::{Deserialize, Serialize};

/// Dominant color palette extracted from media images/thumbnails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DominantPalette {
    /// Dominant primary color as (R, G, B) [0..255].
    pub primary: (u8, u8, u8),
    /// Hex string representation (e.g. `#RRGGBB`).
    pub primary_hex: String,
    /// Light/dark classification.
    pub is_dark: bool,
    /// Suggested accessible text color (black or white) to render on top of primary.
    pub suggested_text_color: (u8, u8, u8),
    /// Suggested accessible text hex (`#000000` or `#ffffff`).
    pub suggested_text_hex: String,
}

impl DominantPalette {
    /// Extract a dominant palette from RGB or RGBA pixel buffer.
    ///
    /// `pixel_bytes` contains sequential RGB (3 bytes per pixel) or RGBA (4 bytes per pixel).
    /// `channels` must be 3 or 4.
    pub fn from_pixels(pixel_bytes: &[u8], channels: usize) -> Option<Self> {
        if pixel_bytes.is_empty() || (channels != 3 && channels != 4) {
            return None;
        }

        let pixel_count = pixel_bytes.len() / channels;
        if pixel_count == 0 {
            return None;
        }

        // Fast subsampling for large images (sample up to 10,000 pixels uniformly)
        let step = (pixel_count / 10000).max(1);
        let mut r_total: u64 = 0;
        let mut g_total: u64 = 0;
        let mut b_total: u64 = 0;
        let mut sampled_count: u64 = 0;

        for i in (0..pixel_count).step_by(step) {
            let offset = i * channels;
            if offset + 2 < pixel_bytes.len() {
                // If RGBA, ignore completely transparent pixels
                if channels == 4 && pixel_bytes[offset + 3] < 32 {
                    continue;
                }
                r_total += pixel_bytes[offset] as u64;
                g_total += pixel_bytes[offset + 1] as u64;
                b_total += pixel_bytes[offset + 2] as u64;
                sampled_count += 1;
            }
        }

        if sampled_count == 0 {
            return None;
        }

        let r = (r_total / sampled_count).min(255) as u8;
        let g = (g_total / sampled_count).min(255) as u8;
        let b = (b_total / sampled_count).min(255) as u8;

        let primary_hex = format!("#{:02x}{:02x}{:02x}", r, g, b);

        let contrast_with_white = wcag_contrast_ratio((r, g, b), (255, 255, 255));
        let contrast_with_black = wcag_contrast_ratio((r, g, b), (0, 0, 0));

        let is_dark = contrast_with_white >= contrast_with_black;
        let (suggested_text_color, suggested_text_hex) = if is_dark {
            ((255, 255, 255), "#ffffff".to_string())
        } else {
            ((0, 0, 0), "#000000".to_string())
        };

        Some(Self {
            primary: (r, g, b),
            primary_hex,
            is_dark,
            suggested_text_color,
            suggested_text_hex,
        })
    }
}

/// Calculate the WCAG 2.1 relative contrast ratio between two sRGB colors.
///
/// Returns a value between 1.0 (identical) and 21.0 (black vs white).
pub fn wcag_contrast_ratio(rgb1: (u8, u8, u8), rgb2: (u8, u8, u8)) -> f64 {
    let c1 = Srgb::new(
        rgb1.0 as f32 / 255.0,
        rgb1.1 as f32 / 255.0,
        rgb1.2 as f32 / 255.0,
    );
    let c2 = Srgb::new(
        rgb2.0 as f32 / 255.0,
        rgb2.1 as f32 / 255.0,
        rgb2.2 as f32 / 255.0,
    );
    c1.relative_contrast(c2) as f64
}

/// Check if two colors meet WCAG accessibility standards.
///
/// - AA requirement: contrast >= 4.5:1 (normal text) or 3:1 (large text)
/// - AAA requirement: contrast >= 7.0:1 (normal text) or 4.5:1 (large text)
///
/// Here `level_aaa` checks for 7.0:1 (AAA) or 4.5:1 (AA).
pub fn is_wcag_accessible(
    text_color: (u8, u8, u8),
    bg_color: (u8, u8, u8),
    level_aaa: bool,
) -> bool {
    let ratio = wcag_contrast_ratio(text_color, bg_color);
    if level_aaa {
        ratio >= 7.0
    } else {
        ratio >= 4.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wcag_contrast_black_white() {
        let white = (255, 255, 255);
        let black = (0, 0, 0);
        let contrast = wcag_contrast_ratio(white, black);
        assert!((contrast - 21.0).abs() < 0.1);
        assert!(is_wcag_accessible(black, white, true));
        assert!(is_wcag_accessible(black, white, false));
    }

    #[test]
    fn test_wcag_contrast_identical() {
        let color = (100, 150, 200);
        let contrast = wcag_contrast_ratio(color, color);
        assert!((contrast - 1.0).abs() < 0.05);
        assert!(!is_wcag_accessible(color, color, false));
    }

    #[test]
    fn test_dominant_palette_extraction() {
        // Red image pixels (RGB)
        let pixels = vec![255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0];
        let palette = DominantPalette::from_pixels(&pixels, 3).expect("palette");
        assert_eq!(palette.primary, (255, 0, 0));
        assert_eq!(palette.primary_hex, "#ff0000");
        // On pure red, white text has ~4.0 contrast, black has ~5.25. Black is better.
        assert_eq!(palette.suggested_text_hex, "#000000");
    }

    #[test]
    fn test_dominant_palette_dark_mode() {
        // Dark blue image pixels
        let pixels = vec![10, 10, 40, 15, 12, 45];
        let palette = DominantPalette::from_pixels(&pixels, 3).expect("palette");
        assert!(palette.is_dark);
        assert_eq!(palette.suggested_text_hex, "#ffffff");
    }

    #[test]
    fn test_dominant_palette_empty() {
        assert!(DominantPalette::from_pixels(&[], 3).is_none());
        assert!(DominantPalette::from_pixels(&[255, 255], 3).is_none());
        assert!(DominantPalette::from_pixels(&[255, 255, 255], 5).is_none());
    }
}
