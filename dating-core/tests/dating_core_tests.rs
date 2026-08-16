//! Integration tests for soshal-dating-core: compatibility scoring,
//! profile filtering, sorting, icebreakers, JSON wrappers, and serde shapes.

use serde_json::json;
use soshal_dating_core::filter::filter_dating_profiles;
use soshal_dating_core::icebreaker::generate_icebreakers_json;
use soshal_dating_core::scoring::{compute_mutual_score, interests, lifestyle, metrics};
use soshal_dating_core::sort::sort_dating_profiles;
use soshal_dating_core::{
    compute_compatibility_json, compute_mutual_score_json, filter_profiles_json,
    sort_profiles_json, DatingProfile, DatingProfileInput, FilterDatingProfilesInput,
    FilteredDatingProfileOut, SortProfilesInput, SortedProfileOut, MAX_PROFILES,
};

fn prof(pk: &str) -> DatingProfileInput {
    DatingProfileInput {
        pubkey: pk.to_string(),
        ..Default::default()
    }
}

fn filter_input(profiles: Vec<DatingProfileInput>) -> FilterDatingProfilesInput {
    FilterDatingProfilesInput {
        profiles,
        own_gender: None,
        own_seeking: None,
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
    }
}

fn sort_input(profiles: Vec<DatingProfileInput>, sort_by: Option<&str>) -> SortProfilesInput {
    SortProfilesInput {
        profiles,
        self_profile: prof("self"),
        self_contacts: vec![],
        sort_by: sort_by.map(str::to_string),
    }
}

// ---------------------------------------------------------------------------
// JSON wrappers
// ---------------------------------------------------------------------------

#[test]
fn compute_compatibility_json_parse_fail_is_zero() {
    assert_eq!(compute_compatibility_json("{bad", "{}"), "0.0");
    assert_eq!(compute_compatibility_json("{}", "{bad"), "0.0");
    assert_eq!(compute_compatibility_json("null", "null"), "0.0");
}

#[test]
fn compute_compatibility_json_identical_full_profiles_is_100() {
    let p = r#"{"age":30,"height":170,"bodyType":"slim","interests":["hiking"],"smoking":"never","drinking":"socially","politics":"liberal","ethnicity":"european","education":"bachelor's","language":["English"],"relationshipIntent":"serious"}"#;
    assert_eq!(compute_compatibility_json(p, p), "100");
}

#[test]
fn compute_compatibility_json_dealbreaker_zeroes() {
    assert_eq!(
        compute_compatibility_json(r#"{"age":30,"dealbreakers":["age"]}"#, r#"{"age":60}"#),
        "0"
    );
    assert_eq!(
        compute_compatibility_json(
            r#"{"height":190,"dealbreakers":["height"]}"#,
            r#"{"height":150}"#
        ),
        "0"
    );
}

#[test]
fn compute_compatibility_json_weight_zero_drops_field() {
    let self_p = r#"{"age":25,"height":170,"preferenceWeights":{"age":0}}"#;
    let other = r#"{"age":25,"height":140}"#;
    assert_eq!(compute_compatibility_json(self_p, other), "45");
}

#[test]
fn compute_mutual_score_json_averages_directions() {
    let a = r#"{"age":25,"preferenceWeights":{"age":0}}"#;
    let b = r#"{"age":27,"preferenceWeights":{"age":1}}"#;
    assert_eq!(compute_mutual_score_json(a, b), "53");
    assert_eq!(
        compute_mutual_score_json(a, a),
        compute_compatibility_json(a, a)
    );
    assert_eq!(compute_mutual_score_json("x", "{}"), "0.0");
}

#[test]
fn compute_mutual_score_direct_structs() {
    let a: DatingProfile = serde_json::from_str(r#"{"age":25}"#).unwrap();
    let b: DatingProfile = serde_json::from_str(r#"{"age":60}"#).unwrap();
    assert_eq!(compute_mutual_score(&a, &b), 45);
    assert_eq!(compute_mutual_score(&a, &b), compute_mutual_score(&b, &a));
    let same: DatingProfile = serde_json::from_str(r#"{"age":30}"#).unwrap();
    assert_eq!(compute_mutual_score(&same, &same), 55);
}

#[test]
fn sort_profiles_json_parse_fail_empty() {
    assert_eq!(sort_profiles_json("bad"), "[]");
    assert_eq!(sort_profiles_json("null"), "[]");
}

#[test]
fn sort_profiles_json_camelcase_roundtrip() {
    let input = json!({
        "profiles": [
            {"pubkey": "old", "age": 40},
            {"pubkey": "young", "age": 20}
        ],
        "selfProfile": {"pubkey": "self", "age": 30},
        "selfContacts": [],
        "sortBy": "age"
    });
    let out = sort_profiles_json(&input.to_string());
    let sorted: Vec<SortedProfileOut> = serde_json::from_str(&out).unwrap();
    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].pubkey, "young");
    assert_eq!(sorted[1].pubkey, "old");
    assert_eq!(sorted[0].event_id, None);
}

#[test]
fn filter_profiles_json_parse_fail_empty() {
    assert_eq!(filter_profiles_json("bad"), "[]");
    assert_eq!(filter_profiles_json("null"), "[]");
}

#[test]
fn filter_profiles_json_camelcase_and_numeric_string_bounds() {
    let input = json!({
        "profiles": [
            {"pubkey": "a", "age": 24, "gender": "female"},
            {"pubkey": "b", "age": 27, "gender": "female"},
            {"pubkey": "c", "age": 31, "gender": "female"}
        ],
        "ownGender": "male",
        "ownSeeking": "female",
        "selfContacts": [],
        "minAge": "25",
        "maxAge": 30
    });
    let out = filter_profiles_json(&input.to_string());
    let res: Vec<FilteredDatingProfileOut> = serde_json::from_str(&out).unwrap();
    assert_eq!(res.len(), 3);
    assert!(!res[0].passes);
    assert!(res[1].passes);
    assert!(!res[2].passes);
    assert_eq!(res[1].index, 1);
}

#[test]
fn filter_profiles_json_bad_min_age_is_empty() {
    let input = json!({"profiles": [], "selfContacts": [], "minAge": "abc"});
    assert_eq!(filter_profiles_json(&input.to_string()), "[]");
}

// ---------------------------------------------------------------------------
// Icebreakers
// ---------------------------------------------------------------------------

#[test]
fn icebreakers_invalid_json_empty() {
    assert_eq!(generate_icebreakers_json("bad"), "[]");
    assert_eq!(generate_icebreakers_json("null"), "[]");
}

#[test]
fn icebreakers_base_prompts_when_no_overlap() {
    let input = json!({
        "self_profile": {"interests": ["Chess"]},
        "peer_profile": {"interests": ["Hiking"]}
    });
    let prompts: Vec<String> =
        serde_json::from_str(&generate_icebreakers_json(&input.to_string())).unwrap();
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0], "Hey! Nice to match with you.");
    assert_eq!(prompts[1], "What's your favorite way to spend a weekend?");
}

#[test]
fn icebreakers_single_shared_interest() {
    let input = json!({
        "self_profile": {"interests": ["Coding", "Chess"]},
        "peer_profile": {"interests": ["Chess", "Hiking"]}
    });
    let prompts: Vec<String> =
        serde_json::from_str(&generate_icebreakers_json(&input.to_string())).unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains("both love Chess!"));
}

#[test]
fn icebreakers_shared_interests_intent_language() {
    let input = json!({
        "self_profile": {
            "interests": ["Coding", "Chess", "Hiking"],
            "relationshipIntent": "serious",
            "language": ["English"]
        },
        "peer_profile": {
            "interests": ["Chess", "Hiking"],
            "relationshipIntent": "serious",
            "language": ["English", "Spanish"]
        }
    });
    let prompts: Vec<String> =
        serde_json::from_str(&generate_icebreakers_json(&input.to_string())).unwrap();
    assert_eq!(prompts.len(), 5);
    assert!(prompts[0].contains("Chess and Hiking"));
    assert!(prompts.iter().any(|p| p.contains("looking for serious")));
    assert!(prompts.iter().any(|p| p.contains("We both speak English")));
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

#[test]
fn filter_gender_seeking_and_trait_match() {
    let mut p = prof("pk1");
    p.gender = Some("female".to_string());
    p.seeking = Some("male".to_string());
    p.body_type = Some("slim".to_string());
    let mut input = filter_input(vec![p]);
    input.own_gender = Some("male".to_string());
    input.own_seeking = Some("female".to_string());
    input.body_type = Some("slim".to_string());
    let res = filter_dating_profiles(input);
    assert_eq!(res.len(), 1);
    assert!(res[0].passes);
    assert_eq!(res[0].index, 0);
}

#[test]
fn filter_rejects_gender_mismatch() {
    let mut p = prof("pk1");
    p.gender = Some("male".to_string());
    let mut input = filter_input(vec![p]);
    input.own_seeking = Some("female".to_string());
    assert!(!filter_dating_profiles(input)[0].passes);
}

#[test]
fn filter_other_seeking_checks_own_gender() {
    let mut p = prof("pk1");
    p.seeking = Some("male".to_string());
    let mut match_input = filter_input(vec![p.clone()]);
    match_input.own_gender = Some("male".to_string());
    assert!(filter_dating_profiles(match_input)[0].passes);
    let mut mismatch = filter_input(vec![p]);
    mismatch.own_gender = Some("female".to_string());
    assert!(!filter_dating_profiles(mismatch)[0].passes);
}

#[test]
fn filter_seeking_all_skips_gender_checks() {
    let mut p = prof("pk1");
    p.gender = Some("male".to_string());
    p.seeking = Some("All".to_string());
    let mut input = filter_input(vec![p]);
    input.own_gender = Some("female".to_string());
    input.own_seeking = Some("All".to_string());
    assert!(filter_dating_profiles(input)[0].passes);
}

#[test]
fn filter_hide_friends_and_is_contact() {
    let mut input = filter_input(vec![prof("friend_pk")]);
    input.self_contacts = vec!["friend_pk".to_string()];
    input.hide_friends = Some(true);
    let res = filter_dating_profiles(input);
    assert!(res[0].is_contact);
    assert!(!res[0].passes);
}

#[test]
fn filter_mutual_friends_only_contacts() {
    let mut p = prof("pk1");
    p.verified_mutual_friends = Some(vec!["c1".to_string(), "stranger".to_string()]);
    let mut input = filter_input(vec![p]);
    input.self_contacts = vec!["c1".to_string()];
    let res = filter_dating_profiles(input);
    assert_eq!(res[0].mutual_friends, vec!["c1"]);
    assert!(!res[0].is_contact);
}

#[test]
fn filter_age_bounds() {
    let mut young = prof("y");
    young.age = Some(24.0);
    let mut old = prof("o");
    old.age = Some(31.0);
    let mut ok = prof("ok");
    ok.age = Some(28.0);
    let mut input = filter_input(vec![young, old, ok]);
    input.min_age = Some(25.0);
    input.max_age = Some(30.0);
    let res = filter_dating_profiles(input);
    assert!(!res[0].passes);
    assert!(!res[1].passes);
    assert!(res[2].passes);
}

#[test]
fn filter_age_missing_rejected_when_bounds_set() {
    let mut input = filter_input(vec![prof("no_age")]);
    input.min_age = Some(18.0);
    assert!(!filter_dating_profiles(input)[0].passes);
}

#[test]
fn filter_trait_any_and_empty_pass() {
    let mut p = prof("pk1");
    p.smoking = Some("regularly".to_string());
    p.body_type = Some("muscular".to_string());
    let mut any = filter_input(vec![p.clone()]);
    any.smoking = Some("any".to_string());
    any.body_type = Some("".to_string());
    assert!(filter_dating_profiles(any)[0].passes);
    let mut strict = filter_input(vec![p]);
    strict.smoking = Some("never".to_string());
    let res = filter_dating_profiles(strict);
    assert!(!res[0].passes);
}

#[test]
fn filter_distance_geohash() {
    let mut same = prof("same");
    same.location_geohash = Some("abc".to_string());
    let mut far = prof("far");
    far.location_geohash = Some("def".to_string());
    let mut input = filter_input(vec![same, far]);
    input.own_location_geohash = Some("abc".to_string());
    input.own_max_distance_km = Some(10.0);
    let res = filter_dating_profiles(input);
    assert!(res[0].passes);
    assert!(!res[1].passes);
}

#[test]
fn filter_respects_profile_own_max_distance() {
    let mut p = prof("pk1");
    p.location_geohash = Some("abc".to_string());
    p.max_distance_km = Some(1.0);
    let mut input = filter_input(vec![p]);
    input.own_location_geohash = Some("def".to_string());
    assert!(!filter_dating_profiles(input)[0].passes);
}

#[test]
fn filter_caps_profile_count() {
    let input = filter_input(vec![prof("x"); MAX_PROFILES + 1]);
    assert!(filter_dating_profiles(input).is_empty());
}

// ---------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------

#[test]
fn sort_default_score_desc() {
    let mut self_p = prof("self");
    self_p.interests = Some(vec!["Music".to_string()]);
    let mut a = prof("a");
    a.interests = Some(vec!["Music".to_string()]);
    let mut b = prof("b");
    b.interests = Some(vec!["Hiking".to_string()]);
    let sorted = sort_dating_profiles(SortProfilesInput {
        profiles: vec![a, b],
        self_profile: self_p,
        self_contacts: vec![],
        sort_by: None,
    });
    assert_eq!(sorted[0].pubkey, "a");
    assert_eq!(sorted[1].pubkey, "b");
    assert!(sorted[0].compatibility_score > sorted[1].compatibility_score);
}

#[test]
fn sort_tiebreak_liked_me_likes_distance() {
    let mut a = prof("a");
    a.liked_me = Some(false);
    a.liker_total_likes = Some(100);
    let mut b = prof("b");
    b.liked_me = Some(true);
    let mut c = prof("c");
    c.liked_me = Some(false);
    c.liker_total_likes = Some(10);
    let mut d = prof("d");
    d.liker_total_likes = Some(10);
    let sorted = sort_dating_profiles(sort_input(vec![a, b, c, d], None));
    assert_eq!(sorted[0].pubkey, "b");
    assert_eq!(sorted[1].pubkey, "c");
    assert_eq!(sorted[2].pubkey, "d");
    assert_eq!(sorted[3].pubkey, "a");
}

#[test]
fn sort_by_age_height_distance() {
    let mut older = prof("older");
    older.age = Some(35.0);
    let mut younger = prof("younger");
    younger.age = Some(22.0);
    let age_sorted = sort_dating_profiles(sort_input(
        vec![older.clone(), younger.clone()],
        Some("age"),
    ));
    assert_eq!(age_sorted[0].pubkey, "younger");
    assert_eq!(age_sorted[1].pubkey, "older");

    let mut tall = prof("tall");
    tall.height = Some(190.0);
    let mut short = prof("short");
    short.height = Some(160.0);
    let height_sorted = sort_dating_profiles(sort_input(vec![short, tall], Some("height")));
    assert_eq!(height_sorted[0].pubkey, "tall");
    assert_eq!(height_sorted[1].pubkey, "short");

    let mut near = prof("near");
    near.location_geohash = Some("abc".to_string());
    let mut far = prof("far");
    far.location_geohash = Some("def".to_string());
    let mut self_p = prof("self");
    self_p.location_geohash = Some("abc".to_string());
    let distance_sorted = sort_dating_profiles(SortProfilesInput {
        profiles: vec![far, near],
        self_profile: self_p,
        self_contacts: vec![],
        sort_by: Some("distance".to_string()),
    });
    assert_eq!(distance_sorted[0].pubkey, "near");
    assert_eq!(distance_sorted[1].pubkey, "far");
}

#[test]
fn sort_age_missing_last() {
    let mut with_age = prof("with_age");
    with_age.age = Some(30.0);
    let sorted = sort_dating_profiles(sort_input(vec![prof("no_age"), with_age], Some("age")));
    assert_eq!(sorted[0].pubkey, "with_age");
    assert_eq!(sorted[1].pubkey, "no_age");
}

#[test]
fn sort_contact_distance_and_mutual_friends() {
    let mut p = prof("pk1");
    p.verified_mutual_friends = Some(vec!["c1".to_string(), "x".to_string()]);
    let sorted = sort_dating_profiles(SortProfilesInput {
        profiles: vec![p],
        self_profile: prof("self"),
        self_contacts: vec!["c1".to_string()],
        sort_by: None,
    });
    assert_eq!(sorted[0].distance, 2);
    assert_eq!(sorted[0].mutual_friends, vec!["c1"]);
    let contact = sort_dating_profiles(SortProfilesInput {
        profiles: vec![prof("c1")],
        self_profile: prof("self"),
        self_contacts: vec!["c1".to_string()],
        sort_by: None,
    });
    assert_eq!(contact[0].distance, 1);
}

#[test]
fn sort_liker_total_likes_defaults_max() {
    let sorted = sort_dating_profiles(sort_input(vec![prof("pk1")], None));
    assert!(!sorted[0].liked_by_me);
    assert!(!sorted[0].liked_me);
    assert_eq!(sorted[0].liker_total_likes, u32::MAX);
}

#[test]
fn sort_caps_profile_count() {
    let input = sort_input(vec![prof("x"); MAX_PROFILES + 1], None);
    assert!(sort_dating_profiles(input).is_empty());
}

// ---------------------------------------------------------------------------
// Scoring metrics
// ---------------------------------------------------------------------------

#[test]
fn score_age_boundaries_and_non_finite() {
    assert_eq!(metrics::score_age(Some(30.0), Some(33.0)), 1.0);
    assert_eq!(metrics::score_age(Some(30.0), Some(35.0)), 0.5);
    assert_eq!(metrics::score_age(Some(30.0), Some(40.0)), 0.25);
    assert_eq!(metrics::score_age(Some(30.0), Some(41.0)), 0.0);
    assert_eq!(metrics::score_age(Some(f64::NAN), Some(30.0)), 0.5);
    assert_eq!(metrics::score_age(Some(30.0), None), 0.5);
    assert_eq!(metrics::score_age(None, None), 0.5);
}

#[test]
fn score_height_bands() {
    assert_eq!(metrics::score_height(Some(170.0), Some(180.0)), 1.0);
    assert_eq!(metrics::score_height(Some(170.0), Some(190.0)), 0.5);
    assert_eq!(metrics::score_height(Some(170.0), Some(190.1)), 0.0);
    assert_eq!(metrics::score_height(Some(170.0), None), 0.5);
    assert_eq!(metrics::score_height(Some(f64::INFINITY), Some(170.0)), 0.5);
}

#[test]
fn score_body_type_similarity() {
    assert_eq!(metrics::score_body_type(Some("slim"), Some("slim")), 1.0);
    assert_eq!(
        metrics::score_body_type(Some("slim"), Some("athletic")),
        0.5
    );
    assert_eq!(
        metrics::score_body_type(Some("slim"), Some("muscular")),
        0.0
    );
    assert_eq!(metrics::score_body_type(Some("bulky"), Some("slim")), 0.0);
    assert_eq!(metrics::score_body_type(None, Some("slim")), 0.5);
}

#[test]
fn score_ethnicity_exact_only() {
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
fn score_education_tiers() {
    assert_eq!(
        metrics::score_education(Some("bachelor's"), Some("bachelor's")),
        1.0
    );
    assert_eq!(
        metrics::score_education(Some("high school"), Some("associate")),
        0.5
    );
    assert_eq!(
        metrics::score_education(Some("high school"), Some("bachelor's")),
        0.25
    );
    assert_eq!(
        metrics::score_education(Some("unknown"), Some("high school")),
        0.5
    );
    assert_eq!(metrics::score_education(None, Some("high school")), 0.5);
}

#[test]
fn score_lifestyle_adjacency() {
    assert_eq!(lifestyle::score_smoking(Some("never"), Some("never")), 1.0);
    assert_eq!(
        lifestyle::score_smoking(Some("never"), Some("occasionally")),
        0.5
    );
    assert_eq!(
        lifestyle::score_smoking(Some("never"), Some("regularly")),
        0.0
    );
    assert_eq!(lifestyle::score_smoking(Some("never"), None), 0.5);
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
fn score_politics_adjacency() {
    assert_eq!(
        lifestyle::score_politics(Some("liberal"), Some("liberal")),
        1.0
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
        lifestyle::score_politics(Some("prefer not to say"), Some("liberal")),
        0.5
    );
    assert_eq!(
        lifestyle::score_politics(Some("anarchist"), Some("liberal")),
        0.0
    );
    assert_eq!(lifestyle::score_politics(None, Some("liberal")), 0.5);
}

#[test]
fn score_relationship_intent() {
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), Some("serious")),
        1.0
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), Some("casual")),
        0.0
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("still figuring out"), Some("serious")),
        0.5
    );
    assert_eq!(
        lifestyle::score_relationship_intent(Some("serious"), None),
        0.5
    );
}

#[test]
fn score_interests_jaccard() {
    let a = vec!["Hiking".to_string(), "Coding".to_string()];
    let b = vec!["hiking".to_string(), "Chess".to_string()];
    let disjoint = vec!["Chess".to_string(), "Music".to_string()];
    assert_eq!(interests::score_interests(Some(&a), Some(&a)), 1.0);
    assert_eq!(interests::score_interests(Some(&a), Some(&b)), 1.0 / 3.0);
    assert_eq!(interests::score_interests(Some(&a), Some(&disjoint)), 0.0);
    assert_eq!(interests::score_interests(Some(&[]), Some(&a)), 0.5);
    assert_eq!(interests::score_interests(None, Some(&a)), 0.5);
}

#[test]
fn score_language_same_as_interests() {
    let a = vec!["English".to_string()];
    let b = vec!["English".to_string(), "Spanish".to_string()];
    assert_eq!(interests::score_language(Some(&a), Some(&b)), 0.5);
    assert_eq!(interests::score_language(None, Some(&b)), 0.5);
}

// ---------------------------------------------------------------------------
// Serde roundtrips
// ---------------------------------------------------------------------------

#[test]
fn dating_profile_serde_roundtrip_camelcase() {
    let p: DatingProfile = serde_json::from_str(
        r#"{"maxDistanceKm":50.5,"verifiedMutualFriends":["a"],"preferenceWeights":{"age":2.0},"dealbreakers":["age"]}"#,
    )
    .unwrap();
    assert_eq!(p.max_distance_km, Some(50.5));
    assert_eq!(p.verified_mutual_friends, Some(vec!["a".to_string()]));
    assert_eq!(p.preference_weights.as_ref().unwrap().age, Some(2.0));
    assert_eq!(p.dealbreakers, Some(vec!["age".to_string()]));
    let re: DatingProfile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(re.max_distance_km, Some(50.5));
    assert_eq!(re.preference_weights.as_ref().unwrap().age, Some(2.0));
}

#[test]
fn sorted_and_filtered_out_serde_roundtrip() {
    let s = SortedProfileOut {
        event_id: Some("e".to_string()),
        pubkey: "pk".to_string(),
        compatibility_score: 77,
        mutual_friends: vec!["c".to_string()],
        distance: 1,
        liked_by_me: true,
        liked_me: false,
        liker_total_likes: 3,
    };
    let s2: SortedProfileOut = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(s2.compatibility_score, 77);
    assert_eq!(s2.mutual_friends, vec!["c".to_string()]);
    assert!(s2.liked_by_me);

    let f = FilteredDatingProfileOut {
        index: 2,
        passes: true,
        is_contact: false,
        mutual_friends: vec![],
    };
    let f2: FilteredDatingProfileOut =
        serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
    assert_eq!(f2.index, 2);
    assert!(f2.passes);
}
