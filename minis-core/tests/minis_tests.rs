//! Integration tests for soshal-minis-core: mini manifest parsing and
//! mini/musicloud event mapping.

use soshal_minis_core::events::{
    custom_profile_content, mini_event_out, mini_from_event, musicloud_comment_addr,
    musicloud_event_out, musicloud_from_event, sort_by_created_desc, sort_minis_desc,
    sort_musicloud_desc, MiniEventOut, MusicloudEventOut,
};
use soshal_minis_core::parse_mini_manifest;
use soshal_minis_core::runtime::{
    WasmComponentHost, WasmComponentPlugin, WasmComponentType, WasmFilterResult,
};
use soshal_nostr_core::models::{find_tag_value, NostrEvent};

fn ev(id: &str, kind: u32, content: &str, tags: Vec<Vec<String>>) -> NostrEvent {
    NostrEvent {
        id: id.into(),
        pubkey: "pk".into(),
        content: content.into(),
        tags,
        created_at: 100.0,
        kind,
    }
}

fn tag(k: &str, v: &str) -> Vec<String> {
    vec![k.to_string(), v.to_string()]
}

#[test]
fn parse_mini_manifest_ok() {
    let json = r#"{"id":"m1","name":"Quiz","description":"fun","entry_url":"https://x.dev/app","icon_url":"https://x.dev/icon.png","author_pubkey":"npub1test"}"#;
    let m = parse_mini_manifest(json).unwrap();
    assert_eq!(m.id, "m1");
    assert_eq!(m.name, "Quiz");
    assert_eq!(m.entry_url, "https://x.dev/app");
    assert_eq!(m.description, "fun");
}

#[test]
fn parse_mini_manifest_errors() {
    assert!(parse_mini_manifest("").is_err());
    assert!(parse_mini_manifest("not json").is_err());
    assert!(parse_mini_manifest(r#"{"id":123}"#).is_err());
}

#[test]
fn mini_mapping_and_defaults() {
    let e = ev(
        "e1",
        31020,
        "overlay text",
        vec![
            tag("url", "https://x/v.mp4"),
            tag("image", "https://x/t.png"),
        ],
    );
    let out = mini_from_event(&e).unwrap();
    assert_eq!(out["videoUrl"], "https://x/v.mp4");
    assert_eq!(out["audience"], "public");
    assert_eq!(out["textOverlay"], "overlay text");

    let typed = mini_event_out(&e).unwrap();
    assert_eq!(typed.video_url, "https://x/v.mp4");
    assert_eq!(typed.thumbnail, "https://x/t.png");
    assert_eq!(typed.audience, "public");

    let private = ev(
        "e2",
        31020,
        "",
        vec![tag("url", "https://x/v.mp4"), tag("audience", "private")],
    );
    assert_eq!(mini_event_out(&private).unwrap().audience, "private");

    assert!(mini_event_out(&ev("e3", 31020, "", vec![])).is_none());
    assert!(mini_from_event(&ev("e4", 31020, "", vec![])).is_none());
}

#[test]
fn musicloud_mapping() {
    let e = ev(
        "e1",
        31022,
        "",
        vec![
            tag("url", "https://x/a.mp3"),
            tag("title", "Song One"),
            tag("d", "song-1"),
            tag("t", "jazz"),
            tag("t", "late-night"),
        ],
    );
    let out = musicloud_from_event(&e).unwrap();
    assert_eq!(out["audioUrl"], "https://x/a.mp3");
    assert_eq!(out["title"], "Song One");
    assert_eq!(out["d"], "song-1");
    assert_eq!(out["hashtags"][0], "jazz");
    assert_eq!(out["hashtags"][1], "late-night");

    let typed = musicloud_event_out(&e).unwrap();
    assert_eq!(typed.title, "Song One");
    assert_eq!(typed.hashtags.len(), 2);
    assert_eq!(typed.audience, "public");

    assert!(musicloud_event_out(&ev("e2", 31022, "", vec![tag("title", "no url")])).is_none());
}

#[test]
fn tiny_helpers() {
    assert_eq!(
        musicloud_comment_addr(31337, "npubx", "song-9"),
        "31337:npubx:song-9"
    );
    let cp = custom_profile_content(&serde_json::json!({"style":{}}), "theme-dark").unwrap();
    let v: serde_json::Value = serde_json::from_str(&cp).unwrap();
    assert_eq!(v["themeId"], "theme-dark");
    assert_eq!(v["nodes"]["style"], serde_json::json!({}));
    assert!(custom_profile_content(&serde_json::Value::Null, "").is_ok());
    assert_eq!(
        find_tag_value(&[tag("a", "1"), tag("b", "2")], "b"),
        Some("2")
    );
    assert_eq!(find_tag_value(&[tag("a", "1")], "missing"), None);
    assert_eq!(find_tag_value(&[vec!["a".into()]], "a"), None);
}

#[test]
fn sorts_by_created_desc() {
    let mut minis = vec![
        MiniEventOut {
            id: "old".into(),
            pubkey: "".into(),
            video_url: "".into(),
            blob_hash: "".into(),
            media_size: 0,
            text_overlay: "".into(),
            thumbnail: "".into(),
            audience: "".into(),
            created_at: 1,
        },
        MiniEventOut {
            id: "new".into(),
            pubkey: "".into(),
            video_url: "".into(),
            blob_hash: "".into(),
            media_size: 0,
            text_overlay: "".into(),
            thumbnail: "".into(),
            audience: "".into(),
            created_at: 9,
        },
        MiniEventOut {
            id: "mid".into(),
            pubkey: "".into(),
            video_url: "".into(),
            blob_hash: "".into(),
            media_size: 0,
            text_overlay: "".into(),
            thumbnail: "".into(),
            audience: "".into(),
            created_at: 5,
        },
    ];
    sort_minis_desc(&mut minis);
    assert_eq!(minis[0].id, "new");
    assert_eq!(minis[2].id, "old");

    let mut mus = vec![
        MusicloudEventOut {
            id: "b".into(),
            pubkey: "".into(),
            audio_url: "".into(),
            blob_hash: "".into(),
            media_size: 0,
            title: "".into(),
            thumbnail: "".into(),
            hashtags: vec![],
            d: "".into(),
            audience: "".into(),
            created_at: 2,
        },
        MusicloudEventOut {
            id: "a".into(),
            pubkey: "".into(),
            audio_url: "".into(),
            blob_hash: "".into(),
            media_size: 0,
            title: "".into(),
            thumbnail: "".into(),
            hashtags: vec![],
            d: "".into(),
            audience: "".into(),
            created_at: 20,
        },
    ];
    sort_musicloud_desc(&mut mus);
    assert_eq!(mus[0].id, "a");

    let mut vals = vec![
        serde_json::json!({"createdAt": 3}),
        serde_json::json!({"createdAt": 30}),
    ];
    sort_by_created_desc(&mut vals);
    assert_eq!(vals[0]["createdAt"], 30);
}

#[test]
fn mini_manifest_derives() {
    use soshal_minis_core::MiniManifest;
    let manifest = MiniManifest {
        id: "app-1".into(),
        name: "Test App".into(),
        description: "Desc".into(),
        entry_url: "https://entry".into(),
        icon_url: Some("https://icon".into()),
        author_pubkey: "pubkey1".into(),
    };
    assert_eq!(manifest.clone(), manifest);
    let str_repr = format!("{:?}", manifest);
    assert!(str_repr.contains("Test App"));
}

fn wasm_plugin(kind: WasmComponentType, id: &str) -> WasmComponentPlugin {
    WasmComponentPlugin {
        plugin_id: id.into(),
        name: "Plugin".into(),
        component_type: kind,
        author_pubkey: "npub_author".into(),
        binary_bytes: vec![0x00, 0x61, 0x73, 0x6d],
    }
}

#[test]
fn wasm_rank_posts_wasm_host_unavailable() {
    let plugin = wasm_plugin(WasmComponentType::FeedRanker, "ranker_1");
    let err = WasmComponentHost::rank_posts(&plugin, vec!["a".into(), "ccc".into(), "bb".into()])
        .unwrap_err();
    assert!(err.contains("unavailable"), "err: {err}");
    let err = WasmComponentHost::rank_posts(&plugin, vec![]).unwrap_err();
    assert!(err.contains("unavailable"), "err: {err}");
}

#[test]
fn wasm_rank_posts_rejects_wrong_type() {
    let plugin = wasm_plugin(WasmComponentType::ContentFilter, "ranker_2");
    let err = WasmComponentHost::rank_posts(&plugin, vec!["post".into()]).unwrap_err();
    assert_eq!(err, "invalid plugin component type for feed ranking");
}

#[test]
fn wasm_filter_content_unavailable() {
    let plugin = wasm_plugin(WasmComponentType::ContentFilter, "filter_1");
    let err = WasmComponentHost::filter_content(&plugin, "Hello safe text").unwrap_err();
    assert!(err.contains("unavailable"));
}

#[test]
fn wasm_filter_content_rejects_wrong_type() {
    let plugin = wasm_plugin(WasmComponentType::ThemeGenerator, "filter_2");
    let err = WasmComponentHost::filter_content(&plugin, "text").unwrap_err();
    assert_eq!(err, "invalid plugin component type for content filter");
}

#[test]
fn wasm_plugin_serde_roundtrip() {
    let plugin = wasm_plugin(WasmComponentType::FeedRanker, "serde_1");
    let json = serde_json::to_string(&plugin).unwrap();
    let back: WasmComponentPlugin = serde_json::from_str(&json).unwrap();
    assert_eq!(back.plugin_id, "serde_1");
    assert_eq!(back.component_type, WasmComponentType::FeedRanker);
    assert_eq!(back.binary_bytes, vec![0x00, 0x61, 0x73, 0x6d]);

    let result = WasmFilterResult {
        allow: true,
        score: 0.05,
        reason: "Clean".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: WasmFilterResult = serde_json::from_str(&json).unwrap();
    assert!(back.allow);
    assert_eq!(back.score, 0.05);
    assert_eq!(back.reason, "Clean");
}

#[test]
fn musicloud_duplicate_tags_first_wins() {
    let e = ev(
        "dup",
        31022,
        "",
        vec![
            tag("url", "blob://aaaa"),
            tag("url", "blob://bbbb"),
            tag("title", "First Title"),
            tag("title", "Second Title"),
            tag("t", "a"),
            tag("t", "b"),
        ],
    );
    let out = musicloud_from_event(&e).unwrap();
    assert_eq!(out["audioUrl"], "blob://aaaa");
    assert_eq!(out["title"], "First Title");
    assert_eq!(out["hashtags"], serde_json::json!(["a", "b"]));

    let typed = musicloud_event_out(&e).unwrap();
    assert_eq!(typed.audio_url, "blob://aaaa");
    assert_eq!(typed.title, "First Title");
    assert_eq!(typed.hashtags, vec!["a", "b"]);
}

#[test]
fn musicloud_hashtag_cap_enforced() {
    // Source breaks once len exceeds 10_000, so the effective cap is 10_001.
    let mut tags = vec![tag("url", "https://x/a.mp3")];
    for i in 0..10_002 {
        tags.push(tag("t", &format!("h{i}")));
    }
    let e = ev("cap", 31022, "", tags);
    let out = musicloud_from_event(&e).unwrap();
    assert_eq!(out["hashtags"].as_array().unwrap().len(), 10_001);
    assert_eq!(musicloud_event_out(&e).unwrap().hashtags.len(), 10_001);
}

#[test]
fn sort_by_created_desc_missing_dates_last() {
    let mut vals = vec![
        serde_json::json!({}),
        serde_json::json!({"createdAt": 7}),
        serde_json::json!({"createdAt": 2}),
    ];
    sort_by_created_desc(&mut vals);
    assert_eq!(vals[0]["createdAt"], 7);
    assert_eq!(vals[1]["createdAt"], 2);
    assert!(vals[2].get("createdAt").is_none());
}

#[test]
fn event_out_serde_camelcase() {
    let mini = mini_event_out(&ev(
        "s1",
        31020,
        "overlay",
        vec![tag("url", "https://x/v.mp4")],
    ))
    .unwrap();
    let json = serde_json::to_value(&mini).unwrap();
    assert_eq!(json["videoUrl"], "https://x/v.mp4");
    assert_eq!(json["textOverlay"], "overlay");
    assert!(json.get("video_url").is_none());

    let mus = musicloud_event_out(&ev(
        "s2",
        31022,
        "",
        vec![tag("url", "https://x/a.mp3"), tag("t", "jazz")],
    ))
    .unwrap();
    let json = serde_json::to_value(&mus).unwrap();
    assert_eq!(json["audioUrl"], "https://x/a.mp3");
    assert_eq!(json["hashtags"], serde_json::json!(["jazz"]));
    assert!(json.get("audio_url").is_none());
}

#[test]
fn mini_manifest_serde_roundtrip() {
    use soshal_minis_core::MiniManifest;
    let manifest = MiniManifest {
        id: "m".into(),
        name: "N".into(),
        description: "D".into(),
        entry_url: "https://e".into(),
        icon_url: None,
        author_pubkey: "pk".into(),
    };
    let json = serde_json::to_string(&manifest).unwrap();
    assert_eq!(parse_mini_manifest(&json).unwrap(), manifest);

    let with_icon = MiniManifest {
        icon_url: Some("https://i".into()),
        ..manifest
    };
    let json = serde_json::to_string(&with_icon).unwrap();
    assert_eq!(
        parse_mini_manifest(&json).unwrap().icon_url.as_deref(),
        Some("https://i")
    );
}

#[test]
fn mini_audience_preserved() {
    let e = ev(
        "a1",
        31020,
        "",
        vec![tag("url", "https://x/v.mp4"), tag("audience", "followers")],
    );
    assert_eq!(mini_from_event(&e).unwrap()["audience"], "followers");
    assert_eq!(mini_event_out(&e).unwrap().audience, "followers");
}

#[test]
fn media_blob_tag_parsed() {
    let hash = "ab".repeat(32);
    let media_tag = vec![
        "media".to_string(),
        "video".to_string(),
        format!("blob://{hash}"),
        hash.clone(),
        "12345".to_string(),
    ];
    let blob_url = format!("blob://{hash}");
    let e = ev(
        "m1",
        31020,
        "overlay",
        vec![tag("url", &blob_url), media_tag],
    );
    let out = mini_from_event(&e).unwrap();
    assert_eq!(out["blobHash"], hash);
    assert_eq!(out["mediaSize"], 12345);
    let typed = mini_event_out(&e).unwrap();
    assert_eq!(typed.blob_hash, hash);
    assert_eq!(typed.media_size, 12345);

    let mus = ev(
        "m2",
        31022,
        "",
        vec![
            tag("url", "https://x/a.mp3"),
            tag("title", "song"),
            vec![
                "media".to_string(),
                "audio".to_string(),
                format!("blob://{hash}"),
                hash.clone(),
                "999".to_string(),
            ],
        ],
    );
    let mout = musicloud_from_event(&mus).unwrap();
    assert_eq!(mout["blobHash"], hash);
    assert_eq!(mout["mediaSize"], 999);
    let mtyped = musicloud_event_out(&mus).unwrap();
    assert_eq!(mtyped.blob_hash, hash);
    assert_eq!(mtyped.media_size, 999);
}

#[test]
fn media_blob_tag_malformed_ignored() {
    let e = ev(
        "m3",
        31020,
        "",
        vec![
            tag("url", "https://x/v.mp4"),
            vec![
                "media".to_string(),
                "video".to_string(),
                "blob://nothex".to_string(),
                "zz".to_string(),
                "1".to_string(),
            ],
        ],
    );
    let typed = mini_event_out(&e).unwrap();
    assert_eq!(typed.blob_hash, "");
    assert_eq!(typed.media_size, 0);
    assert_eq!(mini_from_event(&e).unwrap()["blobHash"], "");
}

#[test]
fn test_mini_rejects_private_ip_url() {
    let e = ev(
        "m_ssrf",
        31020,
        "",
        vec![
            tag("url", "http://127.0.0.1:8080/exploit.mp4"),
            tag("image", "http://169.254.169.254/thumb.png"),
        ],
    );
    assert!(mini_event_out(&e).is_none());
}
