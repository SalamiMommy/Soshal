//! Integration tests for soshal-spatial-core: geohash encoding/expansion and
//! haversine distance math.

use soshal_spatial_core::distance::{haversine_batch_km, haversine_distance, haversine_km};
use soshal_spatial_core::geohash::{
    compute_spatial_matrix_json, encode_geohash, expand_geohash_prefix_json,
    filter_geohash_presence_events_json, get_nearby_prefixes, precision_for_distance,
};

#[test]
fn geohash_encode_known_points() {
    assert_eq!(encode_geohash(52.52, 13.405, 5).as_deref(), Some("u33dc"));
    assert_eq!(encode_geohash(48.8566, 2.3522, 5).as_deref(), Some("u09tv"));
    assert!(encode_geohash(37.7749, -122.4194, 5)
        .as_deref()
        .unwrap()
        .starts_with("9q8"));
}

#[test]
fn geohash_encode_guards() {
    assert_eq!(encode_geohash(91.0, 0.0, 6), None);
    assert_eq!(encode_geohash(0.0, 181.0, 6), None);
    assert_eq!(encode_geohash(f64::NAN, 0.0, 6), None);
    assert_eq!(encode_geohash(10.0, 10.0, 0), None);
    assert_eq!(encode_geohash(10.0, 10.0, 99).unwrap().len(), 12);
}

#[test]
fn geohash_roundtrip_precision() {
    let gh = encode_geohash(51.5074, -0.1278, 8).unwrap();
    assert_eq!(gh.len(), 8);
    let near = encode_geohash(51.5075, -0.1279, 8).unwrap();
    assert_eq!(gh, near);
    let far = encode_geohash(52.0, 0.0, 8).unwrap();
    assert_ne!(gh, far);
}

#[test]
fn nearby_prefixes() {
    let center = encode_geohash(51.5074, -0.1278, 6).unwrap();
    let prefixes = get_nearby_prefixes(&center, 6);
    assert_eq!(prefixes.len(), 9);
    assert_eq!(prefixes[0], center);
    for p in &prefixes {
        assert!(geohash::decode(p).is_ok());
        assert_eq!(p.len(), 6);
    }
    assert_eq!(get_nearby_prefixes("", 6).len(), 0);
    assert_eq!(get_nearby_prefixes(&center, 0).len(), 0);
    assert_eq!(get_nearby_prefixes(&"x".repeat(17), 6).len(), 0);
    assert_eq!(get_nearby_prefixes(&center, 999).len(), 9);
}

#[test]
fn precision_from_distance() {
    assert_eq!(precision_for_distance(-1.0), 4);
    assert_eq!(precision_for_distance(0.0), 4);
    assert_eq!(precision_for_distance(501.0), 3);
    assert_eq!(precision_for_distance(150.0), 4);
    assert_eq!(precision_for_distance(50.0), 5);
    assert_eq!(precision_for_distance(5.0), 6);
}

#[test]
fn haversine_known_distances() {
    let sf_n = 37.7749;
    let sf_e = -122.4194;
    assert_eq!(haversine_km(sf_n, sf_e, sf_n, sf_e), 0.0);
    let nyc_n = 40.7128;
    let nyc_e = -74.0060;
    let sf_nyc = haversine_km(sf_n, sf_e, nyc_n, nyc_e);
    assert!((4120.0..4140.0).contains(&sf_nyc), "sf-nyc = {sf_nyc}");
    let london_paris = haversine_km(51.5074, -0.1278, 48.8566, 2.3522);
    assert!(
        (340.0..350.0).contains(&london_paris),
        "lon-par = {london_paris}"
    );
    let antipode = haversine_km(0.0, 0.0, 0.0, 180.0);
    assert!(
        (19995.0..20040.0).contains(&antipode),
        "antipode = {antipode}"
    );
}

#[test]
fn haversine_batch_matches_scalar() {
    let targets = [(40.7128, -74.0060), (48.8566, 2.3522), (51.5074, -0.1278)];
    let mut results = [0.0f64; 3];
    haversine_batch_km(37.7749, -122.4194, &targets, &mut results);
    for (i, &(lat, lon)) in targets.iter().enumerate() {
        let expected = haversine_km(37.7749, -122.4194, lat, lon);
        assert!((results[i] - expected).abs() < 1e-9);
    }
}

#[test]
fn haversine_distance_via_geohash() {
    let g1 = encode_geohash(51.5074, -0.1278, 7).unwrap();
    let g2 = encode_geohash(48.8566, 2.3522, 7).unwrap();
    let d = haversine_distance(&g1, &g2);
    assert!((340.0..350.0).contains(&d));
    assert_eq!(haversine_distance("", &g2), f64::INFINITY);
    assert_eq!(haversine_distance(&g1, "!!!!!!"), f64::INFINITY);
    assert_eq!(haversine_distance(&"a".repeat(17), &g2), f64::INFINITY);
    assert_eq!(haversine_distance(&g1, &g1), 0.0);
}

#[test]
fn spatial_matrix_json() {
    let input = r#"{"center_lat":37.7749,"center_lon":-122.4194,
        "points":[{"id":"p1","lat":40.7128,"lon":-74.0060},{"id":"p2","lat":37.79,"lon":-122.42}],
        "max_distance_km":50.0}"#;
    let out = compute_spatial_matrix_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["id"], "p2");
    assert!(v[0]["distance_km"].as_f64().unwrap() < 5.0);
    assert_eq!(compute_spatial_matrix_json("garbage"), "[]");
}

#[test]
fn expand_prefix_json() {
    let input = r#"{"geohash":"u33dc","precision":5}"#;
    let out = expand_geohash_prefix_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["prefixes"].as_array().unwrap().len(), 9);
    assert_eq!(expand_geohash_prefix_json("garbage"), "{\"prefixes\":[]}");
}

#[test]
fn filter_presence_json() {
    let input = r#"{"target_prefix":"u33","now_sec":1000,"max_age_sec":600,"events":[
        {"id":"e1","pubkey":"pk","geohash":"u33dc4","created_at":900},
        {"id":"e1","pubkey":"pk","geohash":"u33dc4","created_at":900},
        {"id":"e2","pubkey":"pk","geohash":"zzzzzz","created_at":1}
    ]}"#;
    let out = filter_geohash_presence_events_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["id"], "e1");
    assert_eq!(filter_geohash_presence_events_json("garbage"), "[]");
}

#[test]
fn unicode_geohash_prefix_handling() {
    let unicode_str = "u33dc😊test";
    let prefixes = get_nearby_prefixes(unicode_str, 5);
    assert_eq!(prefixes[0], "u33dc");
}
