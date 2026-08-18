//! FFI surface for runtime permissions + device location.
//!
//! Android: permission state/requests/settings via JNI (platform.rs) —
//! `request_permissions` fires the OS dialog; callers poll `*_granted`.
//! Linux: location via the XDG Desktop Portal (ashpd); camera/mic have no
//! Linux capture path and stay honest-unsupported. Off-Android the
//! permission fns return false / Err.

use flutter_rust_bridge::frb;

const CAMERA: &str = "android.permission.CAMERA";
const MIC: &str = "android.permission.RECORD_AUDIO";
const FINE_LOCATION: &str = "android.permission.ACCESS_FINE_LOCATION";

/// Host platform name: "android", "linux", or "other".
#[frb(sync, serialize)]
pub fn permissions_platform_current() -> String {
    #[cfg(target_os = "android")]
    {
        "android".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        "linux".to_string()
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        "other".to_string()
    }
}

/// Camera + microphone both granted.
#[frb(sync, serialize)]
pub fn permissions_camera_mic_granted() -> bool {
    crate::platform::permission_granted(CAMERA).unwrap_or(false)
        && crate::platform::permission_granted(MIC).unwrap_or(false)
}

/// Fire the camera + microphone permission dialog (grouped on Android 12+).
/// Returns false when unsupported; result is polled via `*_granted`.
#[frb(sync, serialize)]
pub fn permissions_camera_mic_request() -> bool {
    crate::platform::request_permissions(&[CAMERA, MIC]).is_ok()
}

/// Both denied with "don't ask again" (no rationale would be shown).
#[frb(sync, serialize)]
pub fn permissions_camera_mic_permanently_denied() -> bool {
    let cam_denied = !crate::platform::permission_granted(CAMERA).unwrap_or(true);
    let mic_denied = !crate::platform::permission_granted(MIC).unwrap_or(true);
    let cam_perm = cam_denied && !crate::platform::should_show_rationale(CAMERA).unwrap_or(false);
    let mic_perm = mic_denied && !crate::platform::should_show_rationale(MIC).unwrap_or(false);
    cam_perm || mic_perm
}

/// Open the OS app-settings page (Android).
#[frb(sync, serialize)]
pub fn permissions_open_settings() -> bool {
    crate::platform::open_app_settings().is_ok()
}

/// Fine location granted.
#[frb(sync, serialize)]
pub fn permissions_location_granted() -> bool {
    crate::platform::permission_granted(FINE_LOCATION).unwrap_or(false)
}

/// Fire the location permission dialog. Result polled via `*_granted`.
#[frb(sync, serialize)]
pub fn permissions_location_request() -> bool {
    crate::platform::request_permissions(&[FINE_LOCATION]).is_ok()
}

/// Any location provider (GPS or network) enabled.
#[frb(sync, serialize)]
pub fn permissions_location_enabled() -> bool {
    crate::platform::location_enabled().unwrap_or(false)
}

/// XDG Desktop Portal location fix (Linux only; Err elsewhere).
#[frb(serialize)]
pub async fn permissions_location_portal_fix() -> Result<Option<LocationFixDto>, String> {
    #[cfg(target_os = "linux")]
    {
        let fix = ashpd_location().await?;
        Ok(fix)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("location portal unavailable off-Linux".to_string())
    }
}

/// A location fix: coordinates or an honest failure reason.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocationFixDto {
    pub latitude: f64,
    pub longitude: f64,
}

/// One-shot fix via org.freedesktop.portal.Location.
#[cfg(target_os = "linux")]
async fn ashpd_location() -> Result<Option<LocationFixDto>, String> {
    use ashpd::desktop::location::{Accuracy, CreateSessionOptions, LocationProxy};
    use futures_util::{FutureExt, StreamExt};

    let proxy = LocationProxy::new().await.map_err(super::util::to_err)?;
    let session = proxy
        .create_session(CreateSessionOptions::default().set_accuracy(Accuracy::Street))
        .await
        .map_err(super::util::to_err)?;
    let mut stream = proxy
        .receive_location_updated()
        .await
        .map_err(super::util::to_err)?;
    let (start, location) = futures_util::join!(
        proxy
            .start(&session, None, Default::default())
            .map(|r| r.map_err(super::util::to_err)),
        stream.next().map(|r| {
            r.ok_or_else(|| "portal stream exhausted".to_string())
                .map_err(super::util::to_err)
        }),
    );
    start?; // session started (response unnecessary for a one-shot fix)
    let location = location?;
    Ok(Some(LocationFixDto {
        latitude: location.latitude(),
        longitude: location.longitude(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_android_permissions_default_false() {
        if cfg!(any(target_os = "android", target_os = "linux")) {
            return;
        }
        assert!(!permissions_camera_mic_granted());
        assert!(!permissions_camera_mic_request());
        assert!(!permissions_camera_mic_permanently_denied());
        assert!(!permissions_open_settings());
        assert!(!permissions_location_granted());
        assert!(!permissions_location_request());
        assert!(!permissions_location_enabled());
    }
}
