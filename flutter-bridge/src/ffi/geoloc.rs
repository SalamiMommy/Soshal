//! FFI surface for IP-based location lookup — geohash fallback.
//!
//! Desktop Linux usually has no OS location service (XDG portal rejects
//! before prompting), so "Use my location" can never fill the geohash
//! fields there. `geoloc_ip_lookup` returns an *approximate* fix from the
//! egress IP (city-region accuracy) to seed lat/lng; geohash encoding
//! itself stays pure math in spatial-core. Thin adapter over
//! `soshal_network_core::geoloc`.

use flutter_rust_bridge::frb;

use super::permissions::LocationFixDto;

/// Approximate coordinates (lat/lon) for this device's egress IP.
///
/// Callers must obtain explicit user consent first — the query discloses
/// the user's public IP to the geolocation provider.
#[frb(serialize)]
pub async fn geoloc_ip_lookup() -> Result<LocationFixDto, String> {
    let (latitude, longitude) = soshal_network_core::geoloc::ip_coordinates().await?;
    Ok(LocationFixDto {
        latitude,
        longitude,
    })
}
