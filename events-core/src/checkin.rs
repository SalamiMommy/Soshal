pub fn can_checkin(event_start: u64, event_end: u64, now: u64, buffer_secs: u64) -> bool {
    let with_buffer = event_start.saturating_sub(buffer_secs);
    now >= with_buffer && now <= event_end
}

pub fn within_checkin_radius(
    event_lat: f64,
    event_lon: f64,
    user_lat: f64,
    user_lon: f64,
    radius_m: f64,
) -> bool {
    if radius_m <= 0.0 {
        return false;
    }
    // Fast bounding-box pre-filter: 1 degree latitude is approx 111,320 meters.
    // If delta latitude or delta longitude (scaled) exceeds radius, reject in O(1) without trig.
    let max_lat_deg = radius_m / 111_000.0;
    let d_lat_deg = (user_lat - event_lat).abs();
    if d_lat_deg > max_lat_deg {
        return false;
    }

    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let d_lat = (user_lat - event_lat).to_radians();
    let d_lon = (user_lon - event_lon).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + event_lat.to_radians().cos() * user_lat.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c <= radius_m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_within_checkin_radius() {
        assert!(within_checkin_radius(
            37.7749, -122.4194, 37.7749, -122.4194, 500.0
        ));
        assert!(within_checkin_radius(
            37.7749, -122.4194, 37.779, -122.418, 500.0
        ));
        assert!(!within_checkin_radius(
            37.7749, -122.4194, 34.0522, -118.2437, 500.0
        ));
        assert!(within_checkin_radius(
            37.7749, -122.4194, 34.0522, -118.2437, 600_000.0
        ));
    }
}
