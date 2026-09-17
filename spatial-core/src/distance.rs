pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    if lat1.is_nan() || lon1.is_nan() || lat2.is_nan() || lon2.is_nan() {
        return f64::NAN;
    }
    if lat1.is_infinite() || lon1.is_infinite() || lat2.is_infinite() || lon2.is_infinite() {
        return f64::INFINITY;
    }
    if !(-90.0..=90.0).contains(&lat1)
        || !(-90.0..=90.0).contains(&lat2)
        || !(-180.0..=180.0).contains(&lon1)
        || !(-180.0..=180.0).contains(&lon2)
    {
        return f64::NAN;
    }
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = ((d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2))
    .clamp(0.0, 1.0);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

pub fn haversine_batch_km(lat1: f64, lon1: f64, targets: &[(f64, f64)], results: &mut [f64]) {
    let r = 6371.0;
    if lat1.is_nan()
        || lon1.is_nan()
        || !(-90.0..=90.0).contains(&lat1)
        || !(-180.0..=180.0).contains(&lon1)
    {
        for res in results.iter_mut() {
            *res = f64::NAN;
        }
        return;
    }
    if lat1.is_infinite() || lon1.is_infinite() {
        for res in results.iter_mut() {
            *res = f64::INFINITY;
        }
        return;
    }
    let lat1_rad = lat1.to_radians();
    let cos_lat1 = lat1_rad.cos();

    for (i, &(lat2, lon2)) in targets.iter().enumerate().take(results.len()) {
        if lat2.is_nan()
            || lon2.is_nan()
            || !(-90.0..=90.0).contains(&lat2)
            || !(-180.0..=180.0).contains(&lon2)
        {
            results[i] = f64::NAN;
            continue;
        }
        if lat2.is_infinite() || lon2.is_infinite() {
            results[i] = f64::INFINITY;
            continue;
        }
        if (lat1 - lat2).abs() < 1e-7 && (lon1 - lon2).abs() < 1e-7 {
            results[i] = 0.0;
            continue;
        }
        let d_lat = (lat2 - lat1).to_radians();
        let d_lon = (lon2 - lon1).to_radians();
        let a = ((d_lat / 2.0).sin().powi(2)
            + cos_lat1 * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2))
        .clamp(0.0, 1.0);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        results[i] = r * c;
    }
}

pub fn decode_geohash_coords(g: &str) -> Option<(f64, f64)> {
    if g.is_empty() || g.len() > 16 {
        return None;
    }
    match geohash::decode(g) {
        Ok((c, _, _)) => Some((c.y, c.x)),
        Err(_) => None,
    }
}

pub fn haversine_distance(g1: &str, g2: &str) -> f64 {
    if g1 == g2 && !g1.is_empty() {
        return 0.0;
    }
    if g1.is_empty() || g2.is_empty() {
        return f64::INFINITY;
    }
    if g1.len() > 16 || g2.len() > 16 {
        return f64::INFINITY;
    }

    match (decode_geohash_coords(g1), decode_geohash_coords(g2)) {
        (Some((lat1, lon1)), Some((lat2, lon2))) => haversine_km(lat1, lon1, lat2, lon2),
        _ => f64::INFINITY,
    }
}
