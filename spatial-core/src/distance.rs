pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

pub fn haversine_batch_km(lat1: f64, lon1: f64, targets: &[(f64, f64)], results: &mut [f64]) {
    let r = 6371.0;
    let lat1_rad = lat1.to_radians();
    let cos_lat1 = lat1_rad.cos();

    for (i, &(lat2, lon2)) in targets.iter().enumerate().take(results.len()) {
        let d_lat = (lat2 - lat1).to_radians();
        let d_lon = (lon2 - lon1).to_radians();
        let a = (d_lat / 2.0).sin().powi(2)
            + cos_lat1 * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        results[i] = r * c;
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

    match (geohash::decode(g1), geohash::decode(g2)) {
        (Ok((c1, _, _)), Ok((c2, _, _))) => haversine_km(c1.y, c1.x, c2.y, c2.x),
        _ => f64::INFINITY,
    }
}
