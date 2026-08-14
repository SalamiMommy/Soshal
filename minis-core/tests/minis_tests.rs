//! Integration tests for soshal-minis-core: mini manifest parsing and
//! mini/musicloud event mapping.

use soshal_minis_core::events::{
    custom_profile_content, find_tag_str, mini_event_out, mini_from_event, musicloud_comment_addr,
    musicloud_event_out, musicloud_from_event, sort_by_created_desc, sort_minis_desc,
    sort_musicloud_desc, MiniEventOut, MusicloudEventOut,
};
use soshal_minis_core::parse_mini_manifest;
use soshal_nostr_core::models::NostrEvent;

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
    assert_eq!(find_tag_str(&[tag("a", "1"), tag("b", "2")], "b"), "2");
    assert_eq!(find_tag_str(&[tag("a", "1")], "missing"), "");
    assert_eq!(find_tag_str(&[vec!["a".into()]], "a"), "");
}

#[test]
fn sorts_by_created_desc() {
    let mut minis = vec![
        MiniEventOut {
            id: "old".into(),
            pubkey: "".into(),
            video_url: "".into(),
            text_overlay: "".into(),
            thumbnail: "".into(),
            audience: "".into(),
            created_at: 1,
        },
        MiniEventOut {
            id: "new".into(),
            pubkey: "".into(),
            video_url: "".into(),
            text_overlay: "".into(),
            thumbnail: "".into(),
            audience: "".into(),
            created_at: 9,
        },
        MiniEventOut {
            id: "mid".into(),
            pubkey: "".into(),
            video_url: "".into(),
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
