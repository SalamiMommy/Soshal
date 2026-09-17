use crate::distance;
use soshal_common_core::json_util::{json_in_borrow, json_out};

const MAX_GEOHASH_LEN: usize = 16;
/// Maximum geohash precision.
#[doc(hidden)]
pub const MAX_PRECISION: usize = 12;

pub fn get_nearby_prefixes(geohash_str: &str, precision: usize) -> Vec<String> {
    if geohash_str.is_empty() {
        return Vec::new();
    }
    if geohash_str.len() > MAX_GEOHASH_LEN {
        return Vec::new();
    }
    if precision == 0 {
        return Vec::new();
    }
    let precision = precision.min(MAX_PRECISION);
    // Slice at a char boundary: byte-slicing multi-byte input at `precision`
    // would panic.
    let center = if geohash_str.is_ascii() && geohash_str.len() > precision {
        &geohash_str[..precision]
    } else if geohash_str.chars().count() > precision {
        geohash_str
            .char_indices()
            .nth(precision)
            .map(|(i, _)| &geohash_str[..i])
            .unwrap_or(geohash_str)
    } else {
        geohash_str
    };
    let mut result = Vec::with_capacity(9);
    result.push(center.to_string());
    if geohash::decode(center).is_ok() {
        if let Ok(neighbors) = geohash::neighbors(center) {
            result.push(neighbors.n);
            result.push(neighbors.ne);
            result.push(neighbors.e);
            result.push(neighbors.se);
            result.push(neighbors.s);
            result.push(neighbors.sw);
            result.push(neighbors.w);
            result.push(neighbors.nw);
        }
    }
    result
}

pub fn precision_for_distance(max_km: f64) -> i32 {
    if max_km <= 0.0 {
        return 4;
    }
    if max_km >= 500.0 {
        return 3;
    }
    if max_km >= 100.0 {
        return 4;
    }
    if max_km >= 10.0 {
        return 5;
    }
    6
}

/// Encodes a WGS84 coordinate into a geohash string at the given precision.
pub fn encode_geohash(lat: f64, lon: f64, precision: usize) -> Option<String> {
    if !lat.is_finite() || !lon.is_finite() || precision == 0 {
        return None;
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let p = precision.min(MAX_PRECISION);
    geohash::encode(geohash::Coord { x: lon, y: lat }, p).ok()
}

/// Returns true if `s` is a decodable geohash string (non-empty, ≤
/// `MAX_PRECISION` chars, valid base32 alphabet).
pub fn is_valid_geohash(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_PRECISION && geohash::decode(s).is_ok()
}

pub fn compute_spatial_matrix_json(input: &str) -> String {
    #[derive(serde::Deserialize)]
    struct LocationPoint<'a> {
        #[serde(borrow)]
        id: &'a str,
        lat: f64,
        lon: f64,
    }
    #[derive(serde::Deserialize)]
    struct Input<'a> {
        center_lat: f64,
        center_lon: f64,
        #[serde(borrow)]
        points: Vec<LocationPoint<'a>>,
        max_distance_km: f64,
    }
    #[derive(serde::Serialize)]
    struct DistanceResult<'a> {
        id: &'a str,
        distance_km: f64,
    }

    let Some(input) = json_in_borrow::<Input>(input) else {
        return "[]".to_string();
    };

    let mut results: Vec<DistanceResult> = input
        .points
        .into_iter()
        .filter_map(|pt| {
            let dist = distance::haversine_km(input.center_lat, input.center_lon, pt.lat, pt.lon);
            if input.max_distance_km <= 0.0 || dist <= input.max_distance_km {
                Some(DistanceResult {
                    id: pt.id,
                    distance_km: dist,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_unstable_by(|a, b| {
        a.distance_km
            .partial_cmp(&b.distance_km)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    json_out(&results, "[]")
}

pub fn expand_geohash_prefix_json(input: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Input<'a> {
        #[serde(borrow)]
        geohash: &'a str,
        precision: Option<usize>,
    }
    #[derive(serde::Serialize)]
    struct Output {
        prefixes: Vec<String>,
    }

    let Some(input) = json_in_borrow::<Input>(input) else {
        return "{\"prefixes\":[]}".to_string();
    };

    let precision = input.precision.unwrap_or(5);
    let prefixes = get_nearby_prefixes(input.geohash, precision);
    let out = Output { prefixes };
    json_out(&out, "{\"prefixes\":[]}")
}

pub fn filter_geohash_presence_events_json(input: &str) -> String {
    use std::collections::HashSet;

    #[derive(serde::Deserialize)]
    struct SpatialEvent<'a> {
        #[serde(borrow)]
        id: &'a str,
        #[serde(borrow)]
        pubkey: &'a str,
        #[serde(borrow)]
        geohash: &'a str,
        created_at: u64,
    }
    #[derive(serde::Deserialize)]
    struct Input<'a> {
        #[serde(borrow)]
        events: Vec<SpatialEvent<'a>>,
        target_prefix: String,
        now_sec: u64,
        max_age_sec: u64,
    }
    #[derive(serde::Serialize)]
    struct FilteredSpatialEvent<'a> {
        id: &'a str,
        pubkey: &'a str,
        geohash: &'a str,
    }

    let Some(input) = json_in_borrow::<Input>(input) else {
        return "[]".to_string();
    };

    let mut seen_ids = HashSet::with_capacity(input.events.len());
    let mut filtered = Vec::with_capacity(input.events.len());

    for ev in &input.events {
        if !seen_ids.insert(ev.id) {
            continue;
        }

        if input.now_sec > ev.created_at && (input.now_sec - ev.created_at) > input.max_age_sec {
            continue;
        }

        if ev.geohash.is_empty() || ev.geohash.len() > MAX_GEOHASH_LEN {
            continue;
        }

        if !input.target_prefix.is_empty() && !ev.geohash.starts_with(&input.target_prefix) {
            continue;
        }

        filtered.push(FilteredSpatialEvent {
            id: ev.id,
            pubkey: ev.pubkey,
            geohash: ev.geohash,
        });
    }

    json_out(&filtered, "[]")
}
