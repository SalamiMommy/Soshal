//! Spatial FFI module
//! Geohash, spatial presence

use flutter_rust_bridge::frb;
use soshal_spatial_core::geohash::encode_geohash;

#[frb(sync, serialize)]
pub fn spatial_encode_geohash(lat: f64, lon: f64) -> Result<String, String> {
    encode_geohash(lat, lon, 9)
        .ok_or_else(|| "invalid coordinates".to_string())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_geohash_paris() {
        let gh = spatial_encode_geohash(48.8566, 2.3522).unwrap();
        assert_eq!(gh.len(), 9);
        assert!(gh.starts_with("u09t"));
    }

    #[test]
    fn test_encode_geohash_invalid() {
        assert!(spatial_encode_geohash(200.0, 0.0).is_err());
        assert!(spatial_encode_geohash(f64::NAN, 0.0).is_err());
    }
}
