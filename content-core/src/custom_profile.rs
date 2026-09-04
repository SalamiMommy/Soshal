//! Custom profile (kind 30085) node schema: strict, sanitized models.
//! Authority for node types, defaults, and validation. Dart keeps thin
//! DTOs for rendering only.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const CUSTOM_PROFILE_KIND: u64 = soshal_common_core::consts::KIND_CUSTOM_PROFILE as u64;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SanitizedStyles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex_direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_items: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<f64>,
}

impl SanitizedStyles {
    pub fn sanitize(&mut self) -> Result<(), String> {
        if let Some(s) = &self.border_style {
            if !matches!(s.as_str(), "solid" | "dashed" | "dotted" | "none") {
                return Err(format!("invalid borderStyle: {s}"));
            }
        }
        if let Some(s) = &self.flex_direction {
            if !matches!(
                s.as_str(),
                "row" | "column" | "row-reverse" | "column-reverse"
            ) {
                return Err(format!("invalid flexDirection: {s}"));
            }
        }
        if let Some(s) = &self.text_align {
            if !matches!(s.as_str(), "left" | "right" | "center" | "justify") {
                return Err(format!("invalid textAlign: {s}"));
            }
        }
        if let Some(s) = &self.font_size {
            let n = s
                .parse::<f64>()
                .map_err(|_| "invalid fontSize".to_string())?;
            if !(4.0..=200.0).contains(&n) {
                return Err("fontSize out of range 4..=200".to_string());
            }
        }
        for (name, v) in [
            ("padding", &mut self.padding),
            ("margin", &mut self.margin),
            ("borderRadius", &mut self.border_radius),
            ("borderWidth", &mut self.border_width),
        ] {
            if let Some(n) = v {
                if !(0.0..=10_000.0).contains(n) {
                    return Err(format!("{name} out of range 0..=10000"));
                }
            }
        }
        for (name, v) in [
            ("offsetX", &mut self.offset_x),
            ("offsetY", &mut self.offset_y),
        ] {
            if let Some(n) = v {
                if !(-2_000.0..=2_000.0).contains(n) {
                    return Err(format!("{name} out of range -2000..=2000"));
                }
            }
        }
        if let Some(Value::Number(n)) = &self.height {
            if n.as_f64().is_none_or(|f| !(0.0..=10_000.0).contains(&f)) {
                return Err("height out of range".to_string());
            }
        }
        if let Some(Value::Number(n)) = &self.width {
            if n.as_f64().is_none_or(|f| !(0.0..=10_000.0).contains(&f)) {
                return Err("width out of range".to_string());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BaseWidgetProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeProperties {
    pub theme_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_blur: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_overlay: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextBlockProperties {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub markdown_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMediaItem {
    pub id: String,
    pub url: String,
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaGalleryProperties {
    #[serde(default)]
    pub items: Vec<ProfileMediaItem>,
    #[serde(default = "default_grid")]
    pub layout_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

fn default_grid() -> String {
    "grid".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FriendGridProperties {
    #[serde(default = "default_friend_limit")]
    pub limit: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_order: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub show_online_status: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

fn default_friend_limit() -> i64 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlayerProperties {
    #[serde(default)]
    pub tracks: Vec<AudioTrack>,
    #[serde(default)]
    pub autoplay: bool,
    #[serde(default, rename = "loop")]
    pub loop_: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContactCardProperties {
    #[serde(default = "default_true")]
    pub enable_message: bool,
    #[serde(default)]
    pub enable_vouch: bool,
    #[serde(default = "default_true")]
    pub enable_add_friend: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_links: Option<Vec<HashMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QAPair {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QAListProperties {
    #[serde(default)]
    pub pairs: Vec<QAPair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub items: Vec<ProfileMediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabContainerProperties {
    #[serde(default)]
    pub tabs: Vec<TabDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GuestbookEntry {
    pub id: String,
    pub pubkey: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub content: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GuestbookProperties {
    #[serde(default)]
    pub entries: Vec<GuestbookEntry>,
    #[serde(default)]
    pub allow_anonymous: bool,
    #[serde(default = "default_guestbook_max")]
    pub max_entries: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

fn default_guestbook_max() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLinksProperties {
    #[serde(default = "default_true")]
    pub show_minis: bool,
    #[serde(default = "default_true")]
    pub show_musicloud: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryProperties {
    #[serde(default = "default_true")]
    pub show_reposts: bool,
    #[serde(default = "default_history_max")]
    pub max_entries: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_true")]
    pub is_visible: bool,
}

fn default_history_max() -> i64 {
    50
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodePosition {
    #[serde(default)]
    pub row: i64,
    #[serde(default)]
    pub column: i64,
    #[serde(default)]
    pub order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfileNode {
    pub id: String,
    pub r#type: String,
    #[serde(default)]
    pub styles: SanitizedStyles,
    #[serde(default)]
    pub position: NodePosition,
    #[serde(default)]
    pub properties: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfile {
    #[serde(default = "default_theme_id")]
    pub theme_id: String,
    #[serde(default)]
    pub nodes: Vec<CustomProfileNode>,
}

fn default_theme_id() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeTypeInfo {
    pub r#type: String,
    pub label: String,
    pub icon: String,
}

pub fn node_types() -> Vec<NodeTypeInfo> {
    vec![
        NodeTypeInfo {
            r#type: "theme".into(),
            label: "Theme".into(),
            icon: "🎨".into(),
        },
        NodeTypeInfo {
            r#type: "container".into(),
            label: "Container".into(),
            icon: "📦".into(),
        },
        NodeTypeInfo {
            r#type: "text_block".into(),
            label: "Text Block".into(),
            icon: "📝".into(),
        },
        NodeTypeInfo {
            r#type: "media_gallery".into(),
            label: "Media Gallery".into(),
            icon: "🖼".into(),
        },
        NodeTypeInfo {
            r#type: "friend_grid".into(),
            label: "Friend Grid".into(),
            icon: "👥".into(),
        },
        NodeTypeInfo {
            r#type: "music_player".into(),
            label: "Music Player".into(),
            icon: "🎵".into(),
        },
        NodeTypeInfo {
            r#type: "contact_card".into(),
            label: "Contact Card".into(),
            icon: "📇".into(),
        },
        NodeTypeInfo {
            r#type: "qa_list".into(),
            label: "Q&A List".into(),
            icon: "❓".into(),
        },
        NodeTypeInfo {
            r#type: "tab_container".into(),
            label: "Tab Container".into(),
            icon: "📑".into(),
        },
        NodeTypeInfo {
            r#type: "guestbook".into(),
            label: "Guestbook".into(),
            icon: "📖".into(),
        },
        NodeTypeInfo {
            r#type: "profile_links".into(),
            label: "Profile Links".into(),
            icon: "🔗".into(),
        },
        NodeTypeInfo {
            r#type: "post_history".into(),
            label: "Post History".into(),
            icon: "📜".into(),
        },
    ]
}

fn valid_node_types() -> &'static [&'static str] {
    &[
        "theme",
        "container",
        "text_block",
        "media_gallery",
        "friend_grid",
        "music_player",
        "contact_card",
        "qa_list",
        "tab_container",
        "guestbook",
        "profile_links",
        "post_history",
    ]
}

/// Parse + strictly validate a full profile. Returns canonical JSON.
pub fn parse_and_validate(profile_json: &str) -> Result<String, String> {
    let profile: CustomProfile = serde_json::from_str(profile_json)
        .map_err(|e| format!("invalid custom profile JSON: {e}"))?;
    validate(&profile)?;
    serde_json::to_string(&profile).map_err(|e| format!("serialize failed: {e}"))
}

/// Strict validation: node-type whitelist, id/string caps, numeric ranges.
pub fn validate(profile: &CustomProfile) -> Result<(), String> {
    if profile.nodes.len() > 64 {
        return Err("too many nodes (max 64)".to_string());
    }
    for node in &profile.nodes {
        validate_node(node)?;
    }
    Ok(())
}

fn validate_node(node: &CustomProfileNode) -> Result<(), String> {
    if !valid_node_types().contains(&node.r#type.as_str()) {
        return Err(format!("unknown node type: {}", node.r#type));
    }
    if node.id.is_empty() || node.id.len() > 128 {
        return Err("node id must be 1..=128 chars".to_string());
    }
    if !node
        .id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("node id must be alphanumeric/_/-".to_string());
    }
    for (name, v) in [
        ("row", node.position.row),
        ("column", node.position.column),
        ("order", node.position.order),
    ] {
        if !(0..=10_000).contains(&v) {
            return Err(format!("position {name} out of range 0..=10000"));
        }
    }
    for (name, v) in [
        ("offsetX", node.styles.offset_x.unwrap_or(0.0)),
        ("offsetY", node.styles.offset_y.unwrap_or(0.0)),
    ] {
        if !(-2_000.0..=2_000.0).contains(&v) {
            return Err(format!("{name} out of range -2000..=2000"));
        }
    }
    let mut styles = node.styles.clone();
    styles.sanitize()?;
    validate_properties(&node.r#type, &node.properties)
}

fn check_url(name: &str, url: &str) -> Result<(), String> {
    if url.len() > 4_000 {
        return Err(format!("{name} exceeds 4000 chars"));
    }
    if !soshal_common_core::url::is_valid_media_url(url) {
        return Err(format!("{name} is not an allowed public http(s) URL"));
    }
    Ok(())
}

fn validate_properties(node_type: &str, props: &Map<String, Value>) -> Result<(), String> {
    let cap = |name: &str, s: &str, max: usize| -> Result<(), String> {
        if s.len() > max {
            return Err(format!("{name} exceeds {max} chars"));
        }
        Ok(())
    };
    match node_type {
        "theme" => {
            let p: ThemeProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("theme properties: {e}"))?;
            if p.theme_name.len() > 100 {
                return Err("themeName exceeds 100 chars".to_string());
            }
            if let Some(b) = p.background_blur {
                if !(0.0..=100.0).contains(&b) {
                    return Err("backgroundBlur out of range 0..=100".to_string());
                }
            }
            if let Some(u) = &p.background_image_url {
                check_url("backgroundImageUrl", u)?;
            }
        }
        "text_block" => {
            let p: TextBlockProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("text_block properties: {e}"))?;
            cap("content", &p.content, 10_000)?;
        }
        "media_gallery" => {
            let p: MediaGalleryProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("media_gallery properties: {e}"))?;
            if p.items.len() > 100 {
                return Err("too many media items (max 100)".to_string());
            }
            if let Some(c) = p.columns {
                if !(1..=12).contains(&c) {
                    return Err("columns out of range 1..=12".to_string());
                }
            }
            if !matches!(p.layout_type.as_str(), "grid" | "list") {
                return Err("invalid layoutType".to_string());
            }
            for item in &p.items {
                check_url("item url", &item.url)?;
                cap("item id", &item.id, 128)?;
                if let Some(c) = &item.caption {
                    cap("caption", c, 500)?;
                }
            }
        }
        "friend_grid" => {
            let p: FriendGridProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("friend_grid properties: {e}"))?;
            if !(1..=100).contains(&p.limit) {
                return Err("limit out of range 1..=100".to_string());
            }
            if let Some(order) = &p.custom_order {
                if order.len() > 100 {
                    return Err("customOrder exceeds 100 entries".to_string());
                }
            }
        }
        "music_player" => {
            let p: MusicPlayerProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("music_player properties: {e}"))?;
            if p.tracks.len() > 200 {
                return Err("too many tracks (max 200)".to_string());
            }
            for t in &p.tracks {
                cap("track title", &t.title, 200)?;
                check_url("track url", &t.url)?;
            }
        }
        "contact_card" => {
            let p: ContactCardProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("contact_card properties: {e}"))?;
            if let Some(links) = &p.custom_links {
                if links.len() > 20 {
                    return Err("customLinks exceeds 20 entries".to_string());
                }
            }
        }
        "qa_list" => {
            let p: QAListProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("qa_list properties: {e}"))?;
            if p.pairs.len() > 50 {
                return Err("too many Q&A pairs (max 50)".to_string());
            }
            for pair in &p.pairs {
                cap("question", &pair.question, 500)?;
                cap("answer", &pair.answer, 2_000)?;
            }
        }
        "tab_container" => {
            let p: TabContainerProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("tab_container properties: {e}"))?;
            if p.tabs.len() > 20 {
                return Err("too many tabs (max 20)".to_string());
            }
            for tab in &p.tabs {
                cap("tab id", &tab.id, 128)?;
                cap("tab label", &tab.label, 200)?;
                for item in &tab.items {
                    check_url("tab item url", &item.url)?;
                }
            }
        }
        "guestbook" => {
            let p: GuestbookProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("guestbook properties: {e}"))?;
            if !(1..=1000).contains(&p.max_entries) {
                return Err("maxEntries out of range 1..=1000".to_string());
            }
            if p.entries.len() > p.max_entries as usize {
                return Err("entries exceed maxEntries".to_string());
            }
            for e in &p.entries {
                cap("guestbook name", &e.name, 200)?;
                cap("guestbook content", &e.content, 2_000)?;
            }
        }
        "container" | "profile_links" | "post_history" => {
            let _: BaseWidgetProperties = serde_json::from_value(Value::Object(props.clone()))
                .map_err(|e| format!("{node_type} properties: {e}"))?;
        }
        _ => {}
    }
    if let Some(title) = props.get("title").and_then(|v| v.as_str()) {
        cap("title", title, 200)?;
    }
    Ok(())
}

/// Default node for `node_type` at `index` (mirrors legacy Dart factory).
pub fn default_node(node_type: &str, index: u64) -> Result<String, String> {
    if !valid_node_types().contains(&node_type) {
        return Err(format!("unknown node type: {node_type}"));
    }
    let millis = soshal_common_core::util::now_ms();
    let id = format!("widget_{millis}_{index}");
    let mut props = Map::new();
    let title = |t: &str| {
        let mut m = Map::new();
        m.insert("title".to_string(), Value::String(t.to_string()));
        m
    };
    match node_type {
        "theme" => {
            props.insert(
                "themeName".to_string(),
                Value::String("default".to_string()),
            );
            props.insert("title".to_string(), Value::String("Theme".to_string()));
        }
        "container" => props = title("Section"),
        "text_block" => {
            props.insert("content".to_string(), Value::String(String::new()));
            props.insert("title".to_string(), Value::String("About Me".to_string()));
        }
        "media_gallery" => {
            props.insert("items".to_string(), Value::Array(vec![]));
            props.insert("layoutType".to_string(), Value::String("grid".to_string()));
            props.insert("columns".to_string(), Value::from(3));
            props.insert("title".to_string(), Value::String("Gallery".to_string()));
        }
        "friend_grid" => {
            props.insert("limit".to_string(), Value::from(8));
            props.insert("showOnlineStatus".to_string(), Value::Bool(true));
            props.insert(
                "title".to_string(),
                Value::String("Top Friends".to_string()),
            );
        }
        "music_player" => {
            props.insert("tracks".to_string(), Value::Array(vec![]));
            props.insert("autoplay".to_string(), Value::Bool(false));
            props.insert("loop".to_string(), Value::Bool(false));
            props.insert("title".to_string(), Value::String("My Music".to_string()));
        }
        "contact_card" => {
            props.insert("enableMessage".to_string(), Value::Bool(true));
            props.insert("enableVouch".to_string(), Value::Bool(false));
            props.insert("enableAddFriend".to_string(), Value::Bool(true));
            props.insert("title".to_string(), Value::String("Contact".to_string()));
        }
        "qa_list" => {
            props.insert("pairs".to_string(), Value::Array(vec![]));
            props.insert("title".to_string(), Value::String("Q&A".to_string()));
        }
        "tab_container" => {
            let mut tab = Map::new();
            tab.insert("id".to_string(), Value::String(format!("tab_{millis}")));
            tab.insert("label".to_string(), Value::String("Photos".to_string()));
            tab.insert("items".to_string(), Value::Array(vec![]));
            props.insert("tabs".to_string(), Value::Array(vec![Value::Object(tab)]));
            props.insert("title".to_string(), Value::String("Media Tabs".to_string()));
        }
        "guestbook" => {
            props.insert("entries".to_string(), Value::Array(vec![]));
            props.insert("allowAnonymous".to_string(), Value::Bool(false));
            props.insert("maxEntries".to_string(), Value::from(20));
            props.insert("title".to_string(), Value::String("Guestbook".to_string()));
        }
        "profile_links" => {
            props.insert("showMinis".to_string(), Value::Bool(true));
            props.insert("showMusicloud".to_string(), Value::Bool(true));
            props.insert(
                "title".to_string(),
                Value::String("Profile Links".to_string()),
            );
        }
        "post_history" => {
            props.insert("showReposts".to_string(), Value::Bool(true));
            props.insert("maxEntries".to_string(), Value::from(50));
            props.insert(
                "title".to_string(),
                Value::String("Post History".to_string()),
            );
        }
        _ => {}
    }
    let node = CustomProfileNode {
        id,
        r#type: node_type.to_string(),
        styles: SanitizedStyles::default(),
        position: NodePosition {
            row: 0,
            column: 0,
            order: index as i64,
        },
        properties: props,
    };
    serde_json::to_string(&node).map_err(|e| format!("serialize failed: {e}"))
}

/// Empty default profile with no nodes.
pub fn default_profile() -> String {
    serde_json::to_string(&CustomProfile {
        theme_id: "default".to_string(),
        nodes: vec![],
    })
    .unwrap_or_else(|_| r#"{"themeId":"default","nodes":[]}"#.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> String {
        r##"{"themeId":"default","nodes":[{"id":"widget_1","type":"text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}}]}"##
            .to_string()
    }

    #[test]
    fn roundtrip_valid() {
        let canonical = parse_and_validate(&valid_profile()).unwrap();
        assert!(canonical.contains("hello"));
        let parsed: CustomProfile = serde_json::from_str(&canonical).unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.theme_id, "default");
    }

    #[test]
    fn rejects_unknown_type() {
        let bad = valid_profile().replace("text_block", "evil");
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_bad_id() {
        let bad = valid_profile().replace("widget_1", "widget 1; rm -rf");
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_bad_style_enum() {
        let bad = valid_profile().replace(
            "\"textColor\":\"#fff\"",
            "\"textColor\":\"#fff\",\"borderStyle\":\"wavy\"",
        );
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_huge_content() {
        let huge = "x".repeat(10_001);
        let bad = valid_profile().replace("hello", &huge);
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_too_many_nodes() {
        let mut nodes = String::new();
        for i in 0..65 {
            nodes.push_str(&format!(
                r#"{{"id":"n{i}","type":"container","styles":{{}},"position":{{"row":0,"column":0,"order":{i}}},"properties":{{"title":"x"}}}},"#
            ));
        }
        let bad = format!(r#"{{"themeId":"default","nodes":[{nodes}]}}"#);
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn defaults_match_legacy_shape() {
        for t in [
            "theme",
            "container",
            "text_block",
            "media_gallery",
            "friend_grid",
            "music_player",
            "contact_card",
            "qa_list",
            "tab_container",
            "guestbook",
            "profile_links",
            "post_history",
        ] {
            let json = default_node(t, 0).unwrap();
            let node: CustomProfileNode = serde_json::from_str(&json).unwrap();
            assert_eq!(node.r#type, t);
            assert_eq!(node.position.order, 0);
            assert!(node.id.starts_with("widget_"));
            assert!(
                parse_and_validate(&format!(r#"{{"themeId":"default","nodes":[{json}]}}"#)).is_ok()
            );
        }
    }

    #[test]
    fn node_types_has_twelve() {
        assert_eq!(node_types().len(), 12);
        assert!(node_types().iter().any(|t| t.r#type == "guestbook"));
    }

    #[test]
    fn default_profile_empty() {
        let p: CustomProfile = serde_json::from_str(&default_profile()).unwrap();
        assert!(p.nodes.is_empty());
    }

    #[test]
    fn container_accepts_empty_properties() {
        let json = r#"{"themeId":"default","nodes":[{"id":"a1","type":"container","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{}}]}"#;
        assert!(parse_and_validate(json).is_ok());
    }

    #[test]
    fn rejects_private_media_url() {
        let bad = valid_profile().replace(
            r##""text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}"##,
            r#" "media_gallery","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{"items":[{"id":"m1","url":"http://127.0.0.1/x.jpg","type":"image"}],"layoutType":"grid"}"#,
        );
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_ssrf_private_range_url() {
        let bad = valid_profile().replace(
            r##""text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}"##,
            r#" "media_gallery","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{"items":[{"id":"m1","url":"http://192.168.1.1/x.jpg","type":"image"}],"layoutType":"grid"}"#,
        );
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn accepts_public_media_url() {
        let ok = valid_profile().replace(
            r##""text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}"##,
            r#" "media_gallery","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{"items":[{"id":"m1","url":"https://cdn.example.com/x.jpg","type":"image"}],"layoutType":"grid"}"#,
        );
        assert!(parse_and_validate(&ok).is_ok());
    }

    #[test]
    fn rejects_non_http_url() {
        let bad = valid_profile().replace(
            r##""text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}"##,
            r#" "media_gallery","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{"items":[{"id":"m1","url":"javascript:alert(1)","type":"image"}],"layoutType":"grid"}"#,
        );
        assert!(parse_and_validate(&bad).is_err());
    }

    #[test]
    fn rejects_offset_out_of_bounds() {
        let bad = valid_profile().replace(
            r##""styles":{"textColor":"#fff"}"##,
            r##""styles":{"offsetY":5000}"##,
        );
        assert!(parse_and_validate(&bad).is_err());
        let ok = valid_profile().replace(
            r##""styles":{"textColor":"#fff"}"##,
            r##""styles":{"offsetY":-2000}"##,
        );
        assert!(parse_and_validate(&ok).is_ok());
    }

    #[test]
    fn rejects_private_track_url() {
        let bad = valid_profile().replace(
            r##""text_block","styles":{"textColor":"#fff"},"position":{"row":0,"column":0,"order":0},"properties":{"content":"hello","title":"About"}"##,
            r#" "music_player","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{"tracks":[{"id":"t1","title":"x","artist":"y","url":"http://localhost/a.mp3"}]}"#,
        );
        assert!(parse_and_validate(&bad).is_err());
    }
}
