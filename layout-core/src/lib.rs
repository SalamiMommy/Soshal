//! Pre-calculated viewport extents. Card layout math runs in Rust so Flutter
//! lists can use `itemExtentBuilder` (zero layout passes while scrolling).
//!
//! Text heights are EXACT, not heuristics: rustybuzz (harbuzz port) shapes
//! real glyphs of the bundled Noto Sans, wraps with unicode-linebreak, and
//! sums ascent/descent/line-gap. Callers pass the same font size + text scale
//! the UI uses; results match Flutter's TextPainter to within the font
//! feature set (no OpenType features beyond kerning).

use serde::{Deserialize, Serialize};

mod measure;

pub use measure::measure_text;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TextStyleSpec {
    #[serde(default)]
    pub content: String,
    /// Logical font size in px (already multiplied by textScaler).
    pub font_size_px: f32,
    pub line_height_factor: f32,
    pub max_width_px: f32,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub max_lines: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TextBlockLayout {
    pub lines: usize,
    pub height_px: f32,
    pub last_line_width_px: f32,
    pub elided: bool,
    pub max_width_px: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MediaSpec {
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MediaBoxLayout {
    pub height_px: f32,
    pub width_px: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChromeSpec {
    /// Header row (avatar + name row) height.
    #[serde(default)]
    pub header_px: f32,
    /// Action bar (like/reply/zap row) height.
    #[serde(default)]
    pub action_px: f32,
    /// Vertical padding, top + bottom combined.
    #[serde(default)]
    pub padding_px: f32,
    /// Gap between media and text blocks.
    #[serde(default)]
    pub gap_px: f32,
    /// Max rendered media height; taller media is letterboxed by the UI.
    #[serde(default)]
    pub max_media_height_px: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CardLayoutRequest {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: Option<TextStyleSpec>,
    #[serde(default)]
    pub media: Vec<MediaSpec>,
    #[serde(default)]
    pub chrome: ChromeSpec,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CardLayoutResult {
    pub id: String,
    /// Total deterministic height in px — feed of this size for
    /// `itemExtentBuilder`.
    pub height_px: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextBlockLayout>,
    pub media: Vec<MediaBoxLayout>,
    pub media_height_px: f32,
}

impl Default for ChromeSpec {
    fn default() -> Self {
        ChromeSpec {
            header_px: 48.0,
            action_px: 40.0,
            padding_px: 16.0,
            gap_px: 8.0,
            max_media_height_px: 480.0,
        }
    }
}

pub fn compute_card_layout(req: &CardLayoutRequest) -> CardLayoutResult {
    let mut height = req.chrome.header_px + req.chrome.padding_px;
    let text_block = req.text.as_ref().map(measure_text);
    let mut media_boxes = Vec::with_capacity(req.media.len());
    let mut media_height = 0.0;
    for m in &req.media {
        let box_w = text_block
            .as_ref()
            .map(|t| t.max_width_px)
            .unwrap_or(req.chrome.max_media_height_px);
        let box_h = if m.h > 0 {
            (box_w * m.h as f32 / m.w.max(1) as f32).min(req.chrome.max_media_height_px)
        } else {
            0.0
        };
        media_height += box_h;
        media_boxes.push(MediaBoxLayout {
            height_px: box_h,
            width_px: box_w,
        });
    }
    if media_height > 0.0 {
        height += media_height + req.chrome.gap_px;
    }
    let text_height = text_block.as_ref().map(|t| t.height_px).unwrap_or(0.0);
    if text_height > 0.0 {
        height += text_height;
    }
    height += req.chrome.action_px;
    CardLayoutResult {
        id: req.id.clone(),
        height_px: height,
        text: text_block,
        media: media_boxes,
        media_height_px: media_height,
    }
}

/// JSON contract surface for FFI: {"id","text":{"content",...},"media":
/// [{"w","h"}],"chrome":{...}} -> result JSON.
pub fn compute_card_layout_json(request: &str) -> String {
    match serde_json::from_str::<CardLayoutRequest>(request) {
        Ok(req) => serde_json::to_string(&compute_card_layout(&req)).unwrap_or_default(),
        Err(_) => String::new(),
    }
}
