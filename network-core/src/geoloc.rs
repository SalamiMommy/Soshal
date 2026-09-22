//! Free IP-geolocation lookup — device-location fallback for geohash.
//!
//! When the OS location service is off (typical desktop Linux), the XDG
//! portal rejects before prompting, so "Use my location" can never work.
//! This module gives an *approximate* fix from the egress IP (city-region
//! accuracy) to seed lat/lng fields; geohash encoding itself stays pure
//! math in spatial-core. Explicit user consent is required at the UI layer
//! — the user's IP is disclosed to a third party.
//!
//! The provider URL is a compile-time constant run through the same SSRF
//! policy as every outbound request; responses are size-capped and
//! strictly validated so a hostile body can only fail, never fabricate a
//! position.

use std::time::Duration;

use serde::Deserialize;

/// Provider: ipwho.is — free HTTPS JSON geolocation, no API key.
const IPWHOIS_URL: &str = "https://ipwho.is/";

/// Response cap — the payload is a small JSON object; anything larger is
/// hostile or a misbehaving proxy.
const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct IpWhoIsPayload {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    message: Option<String>,
}

/// Approximate (latitude, longitude) for this device's egress IP.
pub async fn ip_coordinates() -> Result<(f64, f64), String> {
    // SSRF policy holds for outbound requests everywhere — even a
    // compile-time constant must not bypass it (future edits stay guarded).
    if !soshal_common_core::url::is_valid_media_url(IPWHOIS_URL) {
        return Err(format!(
            "IP geolocation blocked: URL fails SSRF policy: {IPWHOIS_URL}"
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("IP geolocation client build failed: {e}"))?;
    let resp = client
        .get(IPWHOIS_URL)
        .send()
        .await
        .map_err(|e| format!("IP geolocation request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("IP geolocation body read failed: {e}"))?;
    if body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err("IP geolocation response exceeds size cap".to_string());
    }
    if !status.is_success() {
        return Err(format!("IP geolocation provider error: HTTP {status}"));
    }
    let payload: IpWhoIsPayload =
        serde_json::from_slice(&body).map_err(|e| format!("IP geolocation parse failed: {e}"))?;
    coordinates_from_payload(&payload)
}

/// Validates a provider payload down to finite, in-range coordinates.
fn coordinates_from_payload(payload: &IpWhoIsPayload) -> Result<(f64, f64), String> {
    if !payload.success {
        return Err(match &payload.message {
            Some(m) => format!("IP geolocation rejected: {m}"),
            None => "IP geolocation rejected the lookup".to_string(),
        });
    }
    let (Some(lat), Some(lon)) = (payload.latitude, payload.longitude) else {
        return Err("IP geolocation response lacks coordinates".to_string());
    };
    if !lat.is_finite()
        || !lon.is_finite()
        || !(-90.0..=90.0).contains(&lat)
        || !(-180.0..=180.0).contains(&lon)
    {
        return Err("IP geolocation returned invalid coordinates".to_string());
    }
    Ok((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_payload() {
        let body = br#"{"ip":"93.184.216.34","success":true,"type":"IPv4",
            "continent":"Europe","latitude":51.5007,"longitude":-0.1246}"#;
        let payload: IpWhoIsPayload = serde_json::from_slice(body).unwrap();
        let (lat, lon) = coordinates_from_payload(&payload).unwrap();
        assert_eq!(lat, 51.5007);
        assert_eq!(lon, -0.1246);
    }

    #[test]
    fn missing_coordinates_rejected() {
        // No lat/lon keys in the body → serde defaults to None → the
        // payload must fail validation instead of inventing 0,0.
        let body = br#"{"success":true}"#;
        let payload: IpWhoIsPayload = serde_json::from_slice(body).unwrap();
        let err = coordinates_from_payload(&payload).unwrap_err();
        assert!(err.contains("lacks coordinates"), "got: {err}");
    }

    #[test]
    fn provider_failure_carries_message() {
        let payload = IpWhoIsPayload {
            success: false,
            latitude: Some(1.0),
            longitude: Some(2.0),
            message: Some("reserved range".into()),
        };
        let err = coordinates_from_payload(&payload).unwrap_err();
        assert!(err.contains("reserved range"), "got: {err}");
    }

    #[test]
    fn out_of_range_coordinates_rejected() {
        for (lat, lon) in [(91.0, 0.0), (-90.1, 0.0), (0.0, 181.0), (0.0, -180.1)] {
            let payload = IpWhoIsPayload {
                success: true,
                latitude: Some(lat),
                longitude: Some(lon),
                message: None,
            };
            let err = coordinates_from_payload(&payload).unwrap_err();
            assert!(err.contains("invalid coordinates"), "got: {err}");
        }
    }

    #[test]
    fn non_finite_coordinates_rejected() {
        for (lat, lon) in [(f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            let payload = IpWhoIsPayload {
                success: true,
                latitude: Some(lat),
                longitude: Some(lon),
                message: None,
            };
            assert!(coordinates_from_payload(&payload).is_err());
        }
    }

    #[test]
    fn malformed_json_fails_parse() {
        let body = b"<html>captive portal</html>";
        let res: Result<IpWhoIsPayload, _> = serde_json::from_slice(body);
        assert!(res.is_err());
    }

    #[tokio::test]
    #[ignore = "network: live lookup against ipwho.is"]
    async fn live_lookup_returns_valid_fix() {
        let (lat, lon) = ip_coordinates().await.expect("live IP lookup");
        assert!((-90.0..=90.0).contains(&lat));
        assert!((-180.0..=180.0).contains(&lon));
    }
}
