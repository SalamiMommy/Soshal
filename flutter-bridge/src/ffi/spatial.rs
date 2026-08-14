//! Spatial FFI module
//! Geohash, spatial presence

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn spatial_encode_geohash(_lat: f64, _lon: f64) -> Result<String, String> {
    Ok("geohash".to_string()).into()
}
