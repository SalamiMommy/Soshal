use serde_json::json;
use soshal_layout_core::{
    compute_card_layout, compute_card_layout_json, ChromeSpec, TextStyleSpec,
};

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
fn total_height_accounts_for_chrome() {
    let chrome = ChromeSpec {
        header_px: 48.0,
        action_px: 40.0,
        padding_px: 16.0,
        gap_px: 8.0,
        max_media_height_px: 480.0,
    };
    let req = soshal_layout_core::CardLayoutRequest {
        id: "n1".into(),
        text: Some(spec("short")),
        media: vec![],
        chrome,
    };
    let r = compute_card_layout(&req);
    let text_h = r.text.as_ref().unwrap().height_px;
    assert!((r.height_px - (48.0 + 16.0 + text_h + 40.0)).abs() < 0.01);
}

#[test]
fn media_box_scales_and_caps() {
    let req = soshal_layout_core::CardLayoutRequest {
        id: "n2".into(),
        text: None,
        media: vec![
            soshal_layout_core::MediaSpec { w: 1200, h: 800 },
            soshal_layout_core::MediaSpec { w: 100, h: 10000 },
        ],
        chrome: ChromeSpec {
            max_media_height_px: 200.0,
            ..Default::default()
        },
    };
    let r = compute_card_layout(&req);
    assert_eq!(r.media.len(), 2);
    let base = 200.0f32; // max_media_height doubles as width base when textless
    assert!((r.media[0].height_px - (base * 800.0 / 1200.0)).abs() < 0.01);
    assert!((r.media[1].height_px - 200.0).abs() < 0.01); // capped
    assert!(r.height_px > 0.0);
}

#[test]
fn json_contract_roundtrip() {
    let req = json!({
        "id": "n3",
        "text": {"content": "hello world", "font_size_px": 16.0,
                 "line_height_factor": 1.35, "max_width_px": 300.0},
        "media": [{"w": 640, "h": 360}],
        "chrome": {"header_px": 48.0, "action_px": 40.0, "padding_px": 16.0,
                   "gap_px": 8.0, "max_media_height_px": 480.0}
    });
    let res = compute_card_layout_json(&req.to_string());
    let parsed: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert_eq!(parsed["id"], "n3");
    assert!(parsed["height_px"].as_f64().unwrap() > 0.0);
    assert_eq!(parsed["media"][0]["height_px"].as_f64().unwrap(), 168.75);
}

#[test]
fn json_bad_input_empty() {
    assert_eq!(compute_card_layout_json("not json"), "");
}

#[test]
fn deterministic_across_calls() {
    let a = compute_card_layout(&soshal_layout_core::CardLayoutRequest {
        id: "x".into(),
        text: Some(spec("determinism is the contract")),
        media: vec![],
        chrome: ChromeSpec::default(),
    });
    let b = compute_card_layout(&soshal_layout_core::CardLayoutRequest {
        id: "x".into(),
        text: Some(spec("determinism is the contract")),
        media: vec![],
        chrome: ChromeSpec::default(),
    });
    assert_eq!(a.height_px, b.height_px);
}

#[test]
fn zero_width_media_yields_zero_height() {
    let req = soshal_layout_core::CardLayoutRequest {
        id: "zero".into(),
        text: None,
        media: vec![soshal_layout_core::MediaSpec { w: 0, h: 500 }],
        chrome: ChromeSpec::default(),
    };
    let r = compute_card_layout(&req);
    assert_eq!(r.media[0].height_px, 0.0);
}

#[test]
fn non_finite_chrome_and_text_safe_json() {
    let req = json!({
        "id": "nan_test",
        "text": {"content": "hello", "font_size_px": 16.0, "line_height_factor": 1.2, "max_width_px": -10.0},
        "media": [{"w": 0, "h": 0}],
        "chrome": {"header_px": -5.0}
    });
    let res = compute_card_layout_json(&req.to_string());
    assert!(!res.is_empty());
    let parsed: serde_json::Value = serde_json::from_str(&res).unwrap();
    assert!(parsed["height_px"].as_f64().unwrap() >= 0.0);
}
