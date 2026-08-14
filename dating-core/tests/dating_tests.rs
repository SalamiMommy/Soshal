use soshal_dating_core::filter::filter_dating_profiles;
use soshal_dating_core::icebreaker::generate_icebreakers_json;
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
    assert_eq!(sorted[0].pubkey, "pk2");
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
