//! Integration tests for soshal-spatial-core: geohash encoding/expansion,
//! haversine distance math, json util re-exports, and the wgpu engine session
//! registry.

use soshal_spatial_core::distance::{haversine_batch_km, haversine_distance, haversine_km};
use soshal_spatial_core::geohash::{
    compute_spatial_matrix_json, encode_geohash, expand_geohash_prefix_json,
    filter_geohash_presence_events_json, get_nearby_prefixes, precision_for_distance,
    MAX_PRECISION,
};
use soshal_spatial_core::wgpu_engine::{
    get_wgpu_manager, wgpu_create_session_json, WgpuEngineManager, WgpuMeshEngineSession,
    WgpuMeshNode, WgpuSessionConfig,
};
use soshal_spatial_core::{json_in, json_out};

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

#[test]
fn geohash_boundary_coordinates() {
    // Inclusive range edges encode and decode cleanly. NOTE: the geohash
    // crate's f64 bit-trick misplaces coordinates exactly ON the boundary
    // (lat 90 -> "h000000", a mid-latitude band), so no location bound here.
    let cases = [
        (90.0, 0.0),
        (-90.0, 0.0),
        (0.0, 180.0),
        (0.0, -180.0),
        (90.0, 180.0),
        (-90.0, -180.0),
        (-0.0, -0.0),
    ];
    for (lat, lon) in cases {
        let gh = encode_geohash(lat, lon, 7).unwrap();
        assert_eq!(gh.len(), 7);
        assert!(geohash::decode(&gh).is_ok(), "{lat},{lon} -> {gh}");
    }
    // Near-edge coordinates roundtrip within cell error.
    let near_cases = [
        (89.9999, 0.0),
        (-89.9999, 179.9999),
        (45.0, 179.9999),
        (45.0, -179.9999),
    ];
    for (lat, lon) in near_cases {
        let gh = encode_geohash(lat, lon, 7).unwrap();
        let (c, lon_err, lat_err) = geohash::decode(&gh).unwrap();
        assert!((c.y - lat).abs() <= lat_err + 1e-9, "{lat},{lon} -> {gh}");
        assert!((c.x - lon).abs() <= lon_err + 1e-9, "{lat},{lon} -> {gh}");
    }
    // Just past the edges and non-finite inputs are rejected.
    assert_eq!(encode_geohash(90.000_000_1, 0.0, 6), None);
    assert_eq!(encode_geohash(-90.000_000_1, 0.0, 6), None);
    assert_eq!(encode_geohash(0.0, 180.000_000_1, 6), None);
    assert_eq!(encode_geohash(f64::INFINITY, 0.0, 6), None);
    assert_eq!(encode_geohash(0.0, f64::NEG_INFINITY, 6), None);
    // Precision clamps to MAX_PRECISION.
    assert_eq!(
        encode_geohash(10.0, 10.0, MAX_PRECISION + 5).unwrap().len(),
        MAX_PRECISION
    );
}

#[test]
fn precision_for_distance_boundaries() {
    assert_eq!(precision_for_distance(500.0), 3);
    assert_eq!(precision_for_distance(100.0), 4);
    assert_eq!(precision_for_distance(10.0), 5);
    assert_eq!(precision_for_distance(99.999), 5);
    assert_eq!(precision_for_distance(9.999), 6);
    assert_eq!(precision_for_distance(0.001), 6);
    assert_eq!(precision_for_distance(f64::INFINITY), 3);
}

#[test]
fn spatial_matrix_empty_and_unfiltered() {
    let empty = r#"{"center_lat":0.0,"center_lon":0.0,"points":[],"max_distance_km":10.0}"#;
    assert_eq!(compute_spatial_matrix_json(empty), "[]");
    // Missing max_distance_km fails deserialization -> fallback.
    let missing = r#"{"center_lat":0.0,"center_lon":0.0,"points":[]}"#;
    assert_eq!(compute_spatial_matrix_json(missing), "[]");
    // Non-positive max_distance_km disables the filter entirely.
    let unfiltered = r#"{"center_lat":37.7749,"center_lon":-122.4194,
        "points":[{"id":"far","lat":40.7128,"lon":-74.0060},{"id":"near","lat":37.79,"lon":-122.42}],
        "max_distance_km":0.0}"#;
    let v: serde_json::Value =
        serde_json::from_str(&compute_spatial_matrix_json(unfiltered)).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    let negative = unfiltered.replace("0.0}", "-5.0}");
    let v: serde_json::Value =
        serde_json::from_str(&compute_spatial_matrix_json(&negative)).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn spatial_matrix_sorted_by_distance() {
    let input = r#"{"center_lat":37.7749,"center_lon":-122.4194,
        "points":[{"id":"far","lat":40.7128,"lon":-74.0060},{"id":"near","lat":37.79,"lon":-122.42}],
        "max_distance_km":0.0}"#;
    let v: serde_json::Value = serde_json::from_str(&compute_spatial_matrix_json(input)).unwrap();
    assert_eq!(v[0]["id"], "near");
    assert_eq!(v[1]["id"], "far");
    let near_km = v[0]["distance_km"].as_f64().unwrap();
    let far_km = v[1]["distance_km"].as_f64().unwrap();
    assert!(near_km < far_km);
}

#[test]
fn expand_prefix_variants() {
    // Default precision of 5 when omitted.
    let out = expand_geohash_prefix_json(r#"{"geohash":"u33dc"}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let prefixes = v["prefixes"].as_array().unwrap();
    assert_eq!(prefixes.len(), 9);
    assert!(prefixes.iter().all(|p| p.as_str().unwrap().len() == 5));
    // Shorter precision than the hash truncates the neighbor set.
    let out = expand_geohash_prefix_json(r#"{"geohash":"u33dc","precision":3}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["prefixes"].as_array().unwrap().len(), 9);
    assert!(v["prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p.as_str().unwrap().len() == 3));
    // Precision 0 yields no prefixes.
    assert_eq!(
        expand_geohash_prefix_json(r#"{"geohash":"u33dc","precision":0}"#),
        "{\"prefixes\":[]}"
    );
    // Undecodable geohash returns just the center (no neighbors). 'a' is
    // not in the base32 alphabet (0-9, b-z minus a/i/l/o).
    let out = expand_geohash_prefix_json(r#"{"geohash":"za","precision":5}"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["prefixes"].as_array().unwrap(), &["za"]);
}

#[test]
fn filter_presence_age_and_prefix() {
    let input = r#"{"target_prefix":"u33","now_sec":1000,"max_age_sec":600,"events":[
        {"id":"old","pubkey":"pk","geohash":"u33dc4","created_at":100},
        {"id":"future","pubkey":"pk","geohash":"u33dc4","created_at":2000},
        {"id":"ok","pubkey":"pk","geohash":"u33dc4","created_at":900},
        {"id":"ok","pubkey":"pk","geohash":"u33dc4","created_at":900}
    ]}"#;
    let out = filter_geohash_presence_events_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[0]["id"], "future");
    assert_eq!(v[1]["id"], "ok");
    // Empty target prefix disables the prefix filter; age still applies.
    let input = r#"{"target_prefix":"","now_sec":1000,"max_age_sec":600,"events":[
        {"id":"e1","pubkey":"pk","geohash":"zzzzzz","created_at":900},
        {"id":"e2","pubkey":"pk","geohash":"zzzzzz","created_at":100}
    ]}"#;
    let out = filter_geohash_presence_events_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["id"], "e1");
    // Everything aged out -> empty list.
    let input = r#"{"target_prefix":"","now_sec":1000,"max_age_sec":10,"events":[
        {"id":"e1","pubkey":"pk","geohash":"u33dc4","created_at":1}
    ]}"#;
    assert_eq!(filter_geohash_presence_events_json(input), "[]");
    // Missing target_prefix field fails deserialization -> fallback.
    let input = r#"{"now_sec":1000,"max_age_sec":10,"events":[]}"#;
    assert_eq!(filter_geohash_presence_events_json(input), "[]");
}

#[test]
fn haversine_edges_and_symmetry() {
    assert_eq!(haversine_km(37.7, -122.4, 37.7, -122.4), 0.0);
    let a = haversine_km(37.7749, -122.4194, 40.7128, -74.0060);
    let b = haversine_km(40.7128, -74.0060, 37.7749, -122.4194);
    assert!((a - b).abs() < 1e-9, "haversine not symmetric: {a} vs {b}");
    // One degree of longitude at the equator ~ 111.19 km; same on a meridian.
    let eq = haversine_km(0.0, 0.0, 0.0, 1.0);
    assert!((111.0..111.5).contains(&eq), "equator deg = {eq}");
    let mer = haversine_km(0.0, 0.0, 1.0, 0.0);
    assert!((111.0..111.5).contains(&mer), "meridian deg = {mer}");
    // Crossing the antimeridian takes the short path (~2 deg at 10 N).
    let anti = haversine_km(10.0, 179.0, 10.0, -179.0);
    assert!((218.0..220.0).contains(&anti), "antimeridian = {anti}");
    // Both poles: cos(90) ~ 0 numerically collapses the arc (haversine artifact).
    assert!(haversine_km(90.0, 0.0, 90.0, 180.0) < 1e-6);
    // NaN propagates without panicking.
    assert!(haversine_km(f64::NAN, 0.0, 0.0, 0.0).is_nan());
}

#[test]
fn haversine_batch_buffer_lengths() {
    let targets = [(40.7128, -74.0060), (48.8566, 2.3522), (51.5074, -0.1278)];
    let origin = (37.7749, -122.4194);
    // Results longer than targets: tail untouched.
    let mut long = [42.0f64; 5];
    haversine_batch_km(origin.0, origin.1, &targets, &mut long);
    for (i, &(lat, lon)) in targets.iter().enumerate() {
        let expected = haversine_km(origin.0, origin.1, lat, lon);
        assert!((long[i] - expected).abs() < 1e-9);
    }
    assert_eq!(long[3], 42.0);
    assert_eq!(long[4], 42.0);
    // Results shorter than targets: only the leading slots are written.
    let mut short = [0.0f64; 2];
    haversine_batch_km(origin.0, origin.1, &targets, &mut short);
    for (i, &(lat, lon)) in targets.iter().take(2).enumerate() {
        let expected = haversine_km(origin.0, origin.1, lat, lon);
        assert!((short[i] - expected).abs() < 1e-9);
    }
    // Empty target list leaves the buffer alone.
    let mut untouched = [7.0f64; 2];
    haversine_batch_km(0.0, 0.0, &[], &mut untouched);
    assert_eq!(untouched, [7.0, 7.0]);
    let mut zero: [f64; 0] = [];
    haversine_batch_km(0.0, 0.0, &[], &mut zero);
}

#[test]
fn haversine_distance_edge_cases() {
    // Uppercase is not in the base32 alphabet -> decode fails.
    assert_eq!(haversine_distance("U33DC", "u33dc4"), f64::INFINITY);
    // 16 valid chars exceed the crate's 12-char decode limit.
    assert_eq!(haversine_distance(&"u".repeat(16), "u33dc"), f64::INFINITY);
    // Parent and sub-cell centers are close but distinct.
    let near = haversine_distance("u33d", "u33dc");
    assert!((1.0..30.0).contains(&near), "parent/subcell = {near}");
    // Equal non-empty strings short-circuit to zero even when undecodable.
    assert_eq!(haversine_distance("!!!!", "!!!!"), 0.0);
}

#[test]
fn json_util_reexports() {
    assert_eq!(json_out(&vec![1u8, 2, 3], "[]"), "[1,2,3]");
    assert_eq!(json_in::<Vec<u8>>("[1,2]", vec![]), vec![1, 2]);
    assert_eq!(json_in::<Vec<u8>>("garbage", vec![9]), vec![9]);
}

fn test_node(id: &str) -> WgpuMeshNode {
    WgpuMeshNode {
        id: id.to_string(),
        x: 0.0,
        y: 0.0,
        z: 0.0,
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        latency_ms: 10,
        connections: Vec::new(),
    }
}

#[test]
fn wgpu_mesh_node_serde_roundtrip() {
    let node = WgpuMeshNode {
        id: "n1".to_string(),
        x: 1.5,
        y: -2.5,
        z: 3.25,
        vx: 0.1,
        vy: -0.2,
        vz: 0.3,
        latency_ms: 42,
        connections: vec!["n2".to_string(), "n3".to_string()],
    };
    let json = serde_json::to_string(&node).unwrap();
    let back: WgpuMeshNode = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "n1");
    assert_eq!(back.x, 1.5);
    assert_eq!(back.y, -2.5);
    assert_eq!(back.z, 3.25);
    assert_eq!(back.vx, 0.1);
    assert_eq!(back.vy, -0.2);
    assert_eq!(back.vz, 0.3);
    assert_eq!(back.latency_ms, 42);
    assert_eq!(back.connections, vec!["n2".to_string(), "n3".to_string()]);
}

#[test]
fn wgpu_session_config_serde_roundtrip() {
    let cfg = WgpuSessionConfig {
        session_id: 7,
        width: 320,
        height: 240,
        node_count: 12,
        frame_rate: 30,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: WgpuSessionConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.session_id, 7);
    assert_eq!(back.width, 320);
    assert_eq!(back.height, 240);
    assert_eq!(back.node_count, 12);
    assert_eq!(back.frame_rate, 30);
}

#[test]
fn wgpu_session_step_physics() {
    let session = WgpuMeshEngineSession::new(1, 100, 100);
    let n0 = test_node("n0");
    let mut n1 = test_node("n1");
    n1.x = 1.0;
    session.set_nodes(vec![n0, n1]);
    session.step_simulation(0.1);
    {
        let nodes = session.nodes.lock().unwrap();
        assert!((nodes[0].x - -0.46).abs() < 1e-5, "n0.x = {}", nodes[0].x);
        assert!((nodes[1].x - 1.46).abs() < 1e-5, "n1.x = {}", nodes[1].x);
        assert_eq!(nodes[0].y, 0.0);
        assert_eq!(nodes[0].z, 0.0);
        assert!((nodes[0].vx - -4.6).abs() < 1e-5, "n0.vx = {}", nodes[0].vx);
        assert!((nodes[1].vx - 4.6).abs() < 1e-5, "n1.vx = {}", nodes[1].vx);
    }
    assert_eq!(*session.frame_counter.lock().unwrap(), 1);
    // Repulsion keeps separating the pair on subsequent steps.
    session.step_simulation(0.1);
    let nodes = session.nodes.lock().unwrap();
    assert!(
        nodes[1].x - nodes[0].x > 1.92,
        "separation = {}",
        nodes[1].x - nodes[0].x
    );
    assert_eq!(*session.frame_counter.lock().unwrap(), 2);
}

#[test]
fn wgpu_session_empty_noop() {
    let session = WgpuMeshEngineSession::new(2, 4, 4);
    session.step_simulation(0.5);
    assert_eq!(*session.frame_counter.lock().unwrap(), 0);
    let buf = session.render_pixel_buffer();
    assert_eq!(buf.len(), 4 * 4 * 4);
    assert!(buf.chunks_exact(4).all(|p| p == [0x0D, 0x11, 0x17, 0xFF]));
}

#[test]
fn wgpu_render_pixel_buffer() {
    let session = WgpuMeshEngineSession::new(3, 100, 100);
    session.set_nodes(vec![test_node("center")]);
    let buf = session.render_pixel_buffer();
    assert_eq!(buf.len(), 100 * 100 * 4);
    assert_eq!(&buf[0..4], &[0x0D, 0x11, 0x17, 0xFF]);
    let idx = (50 * 100 + 50) * 4;
    assert_eq!(&buf[idx..idx + 4], &[0x00, 0xE5, 0xFF, 0xFF]);
    // Nodes outside the unit cube are clipped out entirely.
    let mut far = test_node("far");
    far.x = 2.0;
    far.y = 2.0;
    session.set_nodes(vec![far]);
    let buf = session.render_pixel_buffer();
    assert!(buf.chunks_exact(4).all(|p| p == [0x0D, 0x11, 0x17, 0xFF]));
    let mut neg = test_node("neg");
    neg.x = -2.0;
    neg.y = -2.0;
    session.set_nodes(vec![neg]);
    let buf = session.render_pixel_buffer();
    assert!(buf.chunks_exact(4).all(|p| p == [0x0D, 0x11, 0x17, 0xFF]));
    // Zero-sized buffers are valid and empty.
    let zero = WgpuMeshEngineSession::new(4, 0, 0);
    assert!(zero.render_pixel_buffer().is_empty());
    let zero_w = WgpuMeshEngineSession::new(5, 0, 8);
    assert!(zero_w.render_pixel_buffer().is_empty());
}

#[test]
fn wgpu_engine_manager_sessions() {
    let mgr = WgpuEngineManager::new();
    assert_eq!(mgr.create_session(320, 240), 1);
    assert_eq!(mgr.create_session(64, 64), 2);
    let s = mgr.get_session(1).unwrap();
    assert_eq!((s.id, s.width, s.height), (1, 320, 240));
    assert!(s.nodes.lock().unwrap().is_empty());
    assert_eq!(*s.frame_counter.lock().unwrap(), 0);
    assert!(mgr.get_session(999).is_none());
    // Retrievals share the same underlying node store.
    let s1 = mgr.get_session(1).unwrap();
    s1.set_nodes(vec![test_node("a")]);
    assert_eq!(mgr.get_session(1).unwrap().nodes.lock().unwrap().len(), 1);
}

#[test]
fn wgpu_global_manager_and_create_json() {
    assert!(std::ptr::eq(get_wgpu_manager(), get_wgpu_manager()));
    let v1: serde_json::Value = serde_json::from_str(&wgpu_create_session_json(320, 240)).unwrap();
    assert_eq!(v1["width"], 320);
    assert_eq!(v1["height"], 240);
    assert_eq!(v1["node_count"], 0);
    assert_eq!(v1["frame_rate"], 60);
    assert!(v1["session_id"].as_i64().unwrap() > 0);
    let v2: serde_json::Value = serde_json::from_str(&wgpu_create_session_json(1, 1)).unwrap();
    assert!(v2["session_id"].as_i64().unwrap() > v1["session_id"].as_i64().unwrap());
    let cfg: WgpuSessionConfig = serde_json::from_str(&wgpu_create_session_json(8, 8)).unwrap();
    assert_eq!(cfg.width, 8);
    assert_eq!(cfg.height, 8);
}
