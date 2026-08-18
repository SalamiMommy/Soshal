use soshal_dating_core::filter::filter_dating_profiles;
use soshal_dating_core::icebreaker::generate_icebreakers_json;
use soshal_dating_core::scoring::{lifestyle, metrics};
use soshal_dating_core::sort::sort_dating_profiles;
use soshal_dating_core::{
    compute_compatibility_json, DatingProfileInput, FilterDatingProfilesInput, SortProfilesInput,
};

#[test]
fn test_compute_compatibility_json_invalid() {
    assert_eq!(compute_compatibility_json("invalid", "invalid"), "0.0");
}

#[test]
fn test_compute_compatibility_json_valid() {
    let p1 = r#"{"age": 25, "gender": "male"}"#;
    let p2 = r#"{"age": 27, "gender": "female"}"#;
    let res = compute_compatibility_json(p1, p2);
    assert_ne!(res, "0.0");
}

#[test]
fn test_filter_gender_and_seeking_match() {
    let profile1 = DatingProfileInput {
        event_id: Some("e1".to_string()),
        pubkey: "pk1".to_string(),
        age: Some(25.0),
        gender: Some("female".to_string()),
        seeking: Some("male".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: None,
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me: None,
        liker_total_likes: None,
    };

    let input = FilterDatingProfilesInput {
        profiles: vec![profile1],
        own_gender: Some("male".to_string()),
        own_seeking: Some("female".to_string()),
        own_location_geohash: None,
        own_max_distance_km: None,
        self_contacts: vec![],
        hide_friends: None,
        min_age: None,
        max_age: None,
        height_min_cm: None,
        height_max_cm: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        politics: None,
        education: None,
    };

    let res = filter_dating_profiles(input);
    assert_eq!(res.len(), 1);
    assert!(res[0].passes);
    assert_eq!(res[0].index, 0);
}

#[test]
fn test_filter_hide_friends_excludes_contacts() {
    let input = FilterDatingProfilesInput {
        profiles: vec![DatingProfileInput {
            pubkey: "friend_pk".to_string(),
            ..Default::default()
        }],
        own_gender: None,
        own_seeking: None,
        own_location_geohash: None,
        own_max_distance_km: None,
        self_contacts: vec!["friend_pk".to_string()],
        hide_friends: Some(true),
        min_age: None,
        max_age: None,
        height_min_cm: None,
        height_max_cm: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        politics: None,
        education: None,
    };
    let res = filter_dating_profiles(input);
    assert_eq!(res.len(), 1);
    assert!(!res[0].passes);
}

#[test]
fn test_generate_icebreakers_shared_interests() {
    let json_input = r#"{
        "self_profile": {
            "interests": ["Coding", "Chess"],
            "relationshipIntent": "serious",
            "language": ["English"]
        },
        "peer_profile": {
            "interests": ["Chess", "Hiking"],
            "relationshipIntent": "serious",
            "language": ["English", "Spanish"]
        }
    }"#;

    let res_json = generate_icebreakers_json(json_input);
    let prompts: Vec<String> = serde_json::from_str(&res_json).unwrap();
    assert!(prompts.iter().any(|p| p.contains("Chess")));
    assert!(prompts.iter().any(|p| p.contains("looking for serious")));
    assert!(prompts.iter().any(|p| p.contains("English")));
}

#[test]
fn test_generate_icebreakers_invalid_json() {
    let res_json = generate_icebreakers_json("invalid");
    assert_eq!(res_json, "[]");
}

#[test]
fn test_sort_dating_profiles_ordering() {
    let self_profile = DatingProfileInput {
        event_id: None,
        pubkey: "self_pk".to_string(),
        age: Some(25.0),
        gender: Some("male".to_string()),
        seeking: Some("female".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: Some(vec!["Music".to_string()]),
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me: None,
        liker_total_likes: None,
    };

    let p1 = DatingProfileInput {
        event_id: Some("e1".to_string()),
        pubkey: "pk1".to_string(),
        age: Some(25.0),
        gender: Some("female".to_string()),
        seeking: Some("male".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: Some(vec!["Hiking".to_string()]),
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me: Some(false),
        liker_total_likes: None,
    };

    let p2 = DatingProfileInput {
        event_id: Some("e2".to_string()),
        pubkey: "pk2".to_string(),
        age: Some(25.0),
        gender: Some("female".to_string()),
        seeking: Some("male".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: Some(vec!["Music".to_string()]),
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me: Some(true),
        liker_total_likes: None,
    };

    let input = SortProfilesInput {
        profiles: vec![p1, p2],
        self_profile,
        self_contacts: vec![],
        sort_by: None,
    };

    let sorted = sort_dating_profiles(input);
    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].pubkey, "pk2", "liked_me=true ranks first");
    assert_eq!(sorted[1].pubkey, "pk1");
    assert!(sorted[0].compatibility_score >= sorted[1].compatibility_score);
    assert!(sorted[0].liked_me);
    assert!(!sorted[1].liked_me);
}

fn min_profile(
    pubkey: &str,
    liked_me: Option<bool>,
    liker_total_likes: Option<u32>,
) -> DatingProfileInput {
    DatingProfileInput {
        event_id: Some(format!("e_{pubkey}")),
        pubkey: pubkey.to_string(),
        age: Some(27.0),
        gender: Some("female".to_string()),
        seeking: Some("male".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: None,
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me,
        liker_total_likes,
    }
}

fn min_self(pubkey: &str) -> DatingProfileInput {
    DatingProfileInput {
        event_id: None,
        pubkey: pubkey.to_string(),
        age: Some(30.0),
        gender: Some("male".to_string()),
        seeking: Some("female".to_string()),
        height: None,
        body_type: None,
        smoking: None,
        drinking: None,
        relationship_intent: None,
        location_geohash: None,
        max_distance_km: None,
        interests: None,
        politics: None,
        ethnicity: None,
        education: None,
        language: None,
        preference_weights: None,
        dealbreakers: None,
        verified_mutual_friends: None,
        liked_by_me: None,
        liked_me: None,
        liker_total_likes: None,
    }
}

#[test]
fn test_sort_tie_break_by_liker_total_likes() {
    let profiles = vec![
        min_profile("busy", Some(false), Some(500)),
        min_profile("quiet", Some(false), Some(2)),
    ];
    let sorted = sort_dating_profiles(SortProfilesInput {
        profiles,
        self_profile: min_self("self"),
        self_contacts: vec![],
        sort_by: None,
    });
    assert_eq!(sorted[0].pubkey, "quiet", "fewer likes ranked first on tie");
    assert_eq!(sorted[1].pubkey, "busy");
}

#[test]
fn test_sort_by_age_ascending() {
    let mut older = min_profile("older", None, None);
    older.age = Some(35.0);
    let mut younger = min_profile("younger", None, None);
    younger.age = Some(22.0);
    let sorted = sort_dating_profiles(SortProfilesInput {
        profiles: vec![older, younger],
        self_profile: min_self("self"),
        self_contacts: vec![],
        sort_by: Some("age".to_string()),
    });
    assert_eq!(sorted[0].pubkey, "younger");
    assert_eq!(sorted[1].pubkey, "older");
}

#[test]
fn test_compute_mutual_score_and_json_wrappers() {
    use soshal_dating_core::{compute_mutual_score_json, filter_profiles_json, sort_profiles_json};

    let p1 = r#"{"age": 25, "gender": "male"}"#;
    let p2 = r#"{"age": 27, "gender": "female"}"#;
    let score = compute_mutual_score_json(p1, p2);
    assert_ne!(score, "0.0");

    assert_eq!(compute_mutual_score_json("bad", "bad"), "0.0");
    assert_eq!(filter_profiles_json("bad"), "[]");
    assert_eq!(sort_profiles_json("bad"), "[]");
}

#[test]
fn score_age_boundaries() {
    assert_eq!(metrics::score_age(Some(30.0), Some(30.0)), 1.0);
    assert_eq!(metrics::score_age(Some(30.0), Some(33.0)), 1.0);
    assert_eq!(metrics::score_age(Some(30.0), Some(34.0)), 0.5);
    assert_eq!(metrics::score_age(Some(30.0), Some(35.0)), 0.5);
    assert_eq!(metrics::score_age(Some(30.0), Some(36.0)), 0.25);
    assert_eq!(metrics::score_age(Some(30.0), Some(40.0)), 0.25);
    assert_eq!(metrics::score_age(Some(30.0), Some(41.0)), 0.0);
    assert_eq!(metrics::score_age(Some(f64::NAN), Some(30.0)), 0.5);
    assert_eq!(metrics::score_age(Some(30.0), None), 0.5);
}

#[test]
fn score_height_bands() {
    assert_eq!(metrics::score_height(Some(170.0), Some(170.0)), 1.0);
    assert_eq!(metrics::score_height(Some(170.0), Some(180.0)), 1.0);
    assert_eq!(metrics::score_height(Some(170.0), Some(180.1)), 0.5);
    assert_eq!(metrics::score_height(Some(170.0), Some(190.0)), 0.5);
    assert_eq!(metrics::score_height(Some(170.0), Some(190.1)), 0.0);
    assert_eq!(metrics::score_height(Some(170.0), None), 0.5);
}

#[test]
fn score_body_type_table() {
    for t in ["slim", "athletic", "average", "curvy", "muscular"] {
        assert_eq!(metrics::score_body_type(Some(t), Some(t)), 1.0);
    }
    assert_eq!(
        metrics::score_body_type(Some("slim"), Some("athletic")),
        0.5
    );
    assert_eq!(
        metrics::score_body_type(Some("average"), Some("curvy")),
        0.5
    );
    assert_eq!(
        metrics::score_body_type(Some("slim"), Some("muscular")),
        0.0
    );
    assert_eq!(metrics::score_body_type(Some("bulky"), Some("slim")), 0.0);
}

#[test]
fn score_ethnicity_only_exact_match() {
    assert_eq!(
        metrics::score_ethnicity(Some("european"), Some("european")),
        1.0
    );
    assert_eq!(
        metrics::score_ethnicity(Some("european"), Some("american")),
        0.5
    );
    assert_eq!(metrics::score_ethnicity(Some("european"), None), 0.5);
    assert_eq!(metrics::score_ethnicity(None, None), 0.5);
}

#[test]
fn score_education_tier_diff() {
    assert_eq!(
        metrics::score_education(Some("bachelor's"), Some("bachelor's")),
        1.0
    );
    assert_eq!(
        metrics::score_education(Some("high school"), Some("associate")),
        0.5
    );
    assert_eq!(
        metrics::score_education(Some("master's"), Some("doctorate")),
        0.5
    );
    assert_eq!(
        metrics::score_education(Some("high school"), Some("bachelor's")),
        0.25
    );
    assert_eq!(
        metrics::score_education(Some("bachelor's"), Some("doctorate")),
        0.25
    );
    assert_eq!(
        metrics::score_education(Some("unknown"), Some("high school")),
        0.5
    );
    assert_eq!(metrics::score_education(None, Some("high school")), 0.5);
}

#[test]
fn score_smoking_and_drinking_tiers() {
    assert_eq!(lifestyle::score_smoking(Some("never"), Some("never")), 1.0);
    assert_eq!(
        lifestyle::score_smoking(Some("never"), Some("occasionally")),
        0.5
    );
    assert_eq!(
        lifestyle::score_smoking(Some("occasionally"), Some("regularly")),
        0.5
    );
    assert_eq!(
        lifestyle::score_smoking(Some("never"), Some("regularly")),
        0.0
    );
    assert_eq!(lifestyle::score_smoking(Some("vape"), Some("never")), 0.0);
    assert_eq!(lifestyle::score_smoking(Some("never"), None), 0.5);
    assert_eq!(lifestyle::score_smoking(None, None), 0.5);
    assert_eq!(lifestyle::score_drinking(Some("never"), Some("never")), 1.0);
    assert_eq!(
        lifestyle::score_drinking(Some("never"), Some("socially")),
        0.5
    );
    assert_eq!(
        lifestyle::score_drinking(Some("socially"), Some("regularly")),
        0.5
    );
    assert_eq!(
        lifestyle::score_drinking(Some("never"), Some("regularly")),
        0.0
    );
    assert_eq!(lifestyle::score_drinking(Some("sip"), Some("never")), 0.0);
}

#[test]
fn score_politics_prefer_not_to_say() {
    assert_eq!(
        lifestyle::score_politics(Some("liberal"), Some("liberal")),
        1.0
    );
    assert_eq!(
        lifestyle::score_politics(Some("prefer not to say"), Some("liberal")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("liberal"), Some("prefer not to say")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("prefer not to say"), Some("prefer not to say")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("liberal"), Some("moderate")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("liberal"), Some("conservative")),
        0.0
    );
    assert_eq!(
        lifestyle::score_politics(Some("libertarian"), Some("conservative")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("anarchist"), Some("liberal")),
        0.0
    );
}

#[test]
fn score_relationship_intent_clash() {
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), Some("serious")),
        1.0
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), Some("casual")),
        0.0
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("casual"), Some("serious")),
        0.0
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("still figuring out"), Some("serious")),
        0.5
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("marriage"), Some("serious")),
        0.5
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), None),
        0.5
    );
}

#[test]
fn compute_compatibility_score_dealbreaker_zeroes_early() {
    let with_dealbreaker = r#"{"age":30,"height":190,"dealbreakers":["height"]}"#;
    let tall = r#"{"age":30,"height":150}"#;
    assert_eq!(compute_compatibility_json(with_dealbreaker, tall), "0");
    assert_eq!(
        compute_compatibility_json(r#"{"age":30,"height":190}"#, tall),
        "50"
    );
    let age_gap = r#"{"age":25,"dealbreakers":["age"],"height":150}"#;
    assert_eq!(
        compute_compatibility_json(age_gap, r#"{"age":60,"height":150}"#),
        "0"
    );
}

#[test]
fn compute_compatibility_score_all_unknown_is_50() {
    assert_eq!(compute_compatibility_json("{}", "{}"), "50");
}

#[test]
fn compute_mutual_score_averages_directions() {
    use soshal_dating_core::compute_mutual_score_json;

    let a = r#"{"age":25,"preferenceWeights":{"age":0}}"#;
    let b = r#"{"age":27,"preferenceWeights":{"age":1}}"#;
    assert_eq!(compute_compatibility_json(a, b), "50");
    assert_eq!(compute_compatibility_json(b, a), "55");
    assert_eq!(compute_mutual_score_json(a, b), "53");
}

#[test]
fn compute_mutual_score_json_parse_failure_is_zero_string() {
    use soshal_dating_core::compute_mutual_score_json;

    assert_eq!(compute_mutual_score_json("{bad-json", "{}"), "0.0");
    assert_eq!(compute_mutual_score_json("{}", "{bad-json"), "0.0");
}
