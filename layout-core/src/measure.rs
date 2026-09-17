//! rustybuzz text measurement: shape real glyphs, wrap with
//! unicode-linebreak, return exact line/wrap metrics.

use rustybuzz::{Direction, Face, UnicodeBuffer};
use std::sync::OnceLock;
use unicode_linebreak::{linebreaks, BreakOpportunity};

use crate::{TextBlockLayout, TextStyleSpec};

const FONT_REGULAR: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/NotoSans-Bold.ttf");

struct FacePair<'a> {
    regular: Face<'a>,
    bold: Face<'a>,
    metrics: FaceMetrics,
}

struct FaceMetrics {
    units_per_em: f32,
    ascent: f32,
    descent: f32,
    line_gap: f32,
}

fn faces() -> &'static FacePair<'static> {
    static FACES: OnceLock<FacePair<'static>> = OnceLock::new();
    FACES.get_or_init(|| {
        let regular = Face::from_slice(FONT_REGULAR, 0).expect("Noto Sans regular");
        let bold = Face::from_slice(FONT_BOLD, 0).expect("Noto Sans bold");
        let metrics = metrics_of(&regular);
        FacePair {
            regular,
            bold,
            metrics,
        }
    })
}

fn metrics_of(face: &Face) -> FaceMetrics {
    FaceMetrics {
        units_per_em: face.units_per_em() as f32,
        ascent: face.ascender() as f32,
        descent: face.descender() as f32,
        line_gap: face.line_gap() as f32,
    }
}

/// Shape one run; returns total advance width at font_size_px.
fn shape_width(face: &Face, text: &str, font_size_px: f32, upem: f32) -> f32 {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(Direction::LeftToRight);
    let glyphs = rustybuzz::shape(face, &[], buffer);
    let scale = font_size_px / upem;
    glyphs
        .glyph_positions()
        .iter()
        .map(|p| p.x_advance as f32)
        .sum::<f32>()
        * scale
}

/// Line height for a font size (ascent - descent + line gap, times factor).
fn line_height_px(font_size_px: f32, line_height_factor: f32, m: &FaceMetrics) -> f32 {
    let scale = font_size_px / m.units_per_em;
    let h = (m.ascent - m.descent + m.line_gap) * scale;
    (h * line_height_factor.max(1.0)).ceil()
}

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

struct MeasureCache {
    map: HashMap<u64, TextBlockLayout>,
    queue: std::collections::VecDeque<u64>,
}

static MEASURE_CACHE: Mutex<Option<MeasureCache>> = Mutex::new(None);

fn hash_spec(spec: &TextStyleSpec) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    spec.content.hash(&mut hasher);
    spec.font_size_px.to_bits().hash(&mut hasher);
    spec.line_height_factor.to_bits().hash(&mut hasher);
    spec.max_width_px.to_bits().hash(&mut hasher);
    spec.bold.hash(&mut hasher);
    spec.max_lines.hash(&mut hasher);
    hasher.finish()
}

/// Measure wrapped text. Spaces are shaped separately so word boundaries
/// carry correct inter-word advance.
pub fn measure_text(spec: &TextStyleSpec) -> TextBlockLayout {
    let max_width = if spec.max_width_px.is_finite() && spec.max_width_px > 0.0 {
        spec.max_width_px
    } else {
        0.0
    };
    let mut out = TextBlockLayout {
        lines: 0,
        height_px: 0.0,
        last_line_width_px: 0.0,
        elided: false,
        max_width_px: max_width,
    };
    if max_width <= 0.0 || !spec.font_size_px.is_finite() || spec.font_size_px <= 0.0 {
        return out;
    }

    let cache_key = hash_spec(spec);
    if let Ok(guard) = MEASURE_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if let Some(cached) = cache.map.get(&cache_key) {
                return cached.clone();
            }
        }
    }
    let f = faces();
    let face = if spec.bold { &f.bold } else { &f.regular };
    let upem = f.metrics.units_per_em;
    let line_h = line_height_px(spec.font_size_px, spec.line_height_factor, &f.metrics);
    let space_width = shape_width(face, " ", spec.font_size_px, upem);

    let content = spec.content.as_str();
    if content.is_empty() {
        return out;
    }
    let max_lines = spec.max_lines.unwrap_or(u32::MAX) as usize;
    let mut line_count = 0usize;
    let mut line_width = 0.0f32;
    let mut last_width = 0.0f32;
    let mut word_start = 0usize;

    // unicode-linebreak yields (byte_index, kind): byte_index is where the
    // NEXT line segment starts. A Mandatory break ends the current line.
    for (idx, kind) in linebreaks(content) {
        if idx > word_start {
            let word = &content[word_start..idx];
            word_start = idx;
            let w = shape_width(face, word, spec.font_size_px, upem);
            let add = if line_width > 0.0 { space_width + w } else { w };
            if line_width > 0.0 && line_width + add > spec.max_width_px {
                last_width = line_width;
                line_count += 1;
                if line_count > max_lines {
                    out.elided = true;
                    break;
                }
                line_width = w;
            } else {
                line_width += add;
            }
        }
        if kind == BreakOpportunity::Mandatory {
            last_width = line_width;
            line_count += 1;
            if line_count > max_lines {
                out.elided = true;
                break;
            }
            line_width = 0.0;
        }
    }

    out.lines = line_count.min(max_lines);
    out.height_px = line_h * out.lines as f32;
    out.last_line_width_px = last_width;

    if let Ok(mut guard) = MEASURE_CACHE.lock() {
        let cache = guard.get_or_insert_with(|| MeasureCache {
            map: HashMap::with_capacity(1024),
            queue: std::collections::VecDeque::with_capacity(1024),
        });
        if cache.map.contains_key(&cache_key) {
            return out;
        }
        if cache.map.len() >= 1024 {
            // Evict oldest 25% rather than wiping the entire cache.
            for _ in 0..256 {
                if let Some(oldest) = cache.queue.pop_front() {
                    cache.map.remove(&oldest);
                }
            }
        }
        cache.map.insert(cache_key, out.clone());
        cache.queue.push_back(cache_key);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(content: &str) -> TextStyleSpec {
        TextStyleSpec {
            content: content.to_string(),
            font_size_px: 16.0,
            line_height_factor: 1.35,
            max_width_px: 300.0,
            bold: false,
            max_lines: None,
        }
    }

    #[test]
    fn empty_text_zero_lines() {
        let m = measure_text(&spec(""));
        assert_eq!(m.lines, 0);
        assert_eq!(m.height_px, 0.0);
    }

    #[test]
    fn single_short_line() {
        let m = measure_text(&spec("hello"));
        assert_eq!(m.lines, 1);
        assert!(m.height_px > 10.0 && m.height_px < 60.0);
        assert!(m.last_line_width_px > 10.0);
    }

    #[test]
    fn long_text_wraps() {
        let long = "word ".repeat(60);
        let m = measure_text(&spec(&long));
        assert!(m.lines > 2, "expected wraps, got {}", m.lines);
        assert!(m.last_line_width_px <= 300.0 + 0.5);
    }

    #[test]
    fn max_lines_elides() {
        let long = "word ".repeat(60);
        let mut s = spec(&long);
        s.max_lines = Some(2);
        let m = measure_text(&s);
        assert_eq!(m.lines, 2);
        assert!(m.elided);
    }

    #[test]
    fn bold_is_wider() {
        let t = "waffles and honey butter";
        let reg = measure_text(&spec(t));
        let mut bold = spec(t);
        bold.bold = true;
        let m = measure_text(&bold);
        assert!(m.last_line_width_px > reg.last_line_width_px);
    }

    #[test]
    fn newlines_make_lines() {
        let m = measure_text(&spec("a\nb\nc\n"));
        assert_eq!(m.lines, 3);
    }

    #[test]
    fn cjk_wraps() {
        let cjk = "云龙风虎天际真文".repeat(20);
        let m = measure_text(&spec(&cjk));
        assert!(m.lines > 1);
    }
}
