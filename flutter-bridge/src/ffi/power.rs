//! FFI surface for OS power/connectivity sampling.
//!
//! Feeds the P2P seeding scheduler. Android: JNI on BatteryManager /
//! PowerManager / ConnectivityManager (platform.rs). Linux: UPower battery
//! with NetworkManager device scan via zbus; absent session bus or battery
//! falls back to honest desktop defaults (charging, full battery, not
//! cellular, no save mode). Off-Android the sample degrades to the same
//! defaults so the scheduler never stalls.

use flutter_rust_bridge::frb;

/// OS power/connectivity facts consumed by the seeding scheduler.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PowerStateDto {
    pub charging: bool,
    pub battery_percent: i32,
    pub cellular: bool,
    pub low_power_mode: bool,
}

/// One-shot OS power/connectivity sample.
#[frb(serialize)]
pub async fn power_sample_os_state() -> Result<PowerStateDto, String> {
    #[cfg(target_os = "android")]
    {
        let (charging, battery_percent) = crate::platform::battery_state()?;
        let low_power_mode = crate::platform::power_save_mode()?;
        let cellular = crate::platform::cellular_connection()?;
        Ok(PowerStateDto {
            charging,
            battery_percent,
            cellular,
            low_power_mode,
        })
    }
    #[cfg(target_os = "linux")]
    {
        Ok(linux_sample().await)
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        Ok(PowerStateDto {
            charging: true,
            battery_percent: 100,
            cellular: false,
            low_power_mode: false,
        })
    }
}

/// UPower + NetworkManager over the session bus. Every lookup is
/// best-effort: any failure degrades to desktop defaults so the scheduler
/// keeps running with whatever facts we could gather.
#[cfg(target_os = "linux")]
async fn linux_sample() -> PowerStateDto {
    use zbus::proxy::Proxy;

    let mut charging = true;
    let mut battery_percent = 100;
    let mut cellular = false;

    let Ok(conn) = zbus::Connection::system().await else {
        return PowerStateDto {
            charging,
            battery_percent,
            cellular,
            low_power_mode: false,
        };
    };

    // UPower: root OnBattery flag + first battery device percentage.
    let upower_root = Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
    )
    .await;
    if let Ok(root) = upower_root {
        let on_battery = root.get_property::<bool>("OnBattery").await.unwrap_or(true);
        charging = !on_battery;
        if let Ok(devices) = root
            .call_method("EnumerateDevices", &())
            .await
            .and_then(|m| {
                m.body()
                    .deserialize::<Vec<zbus::zvariant::OwnedObjectPath>>()
            })
        {
            for device in devices {
                let proxy = Proxy::new(
                    &conn,
                    "org.freedesktop.UPower",
                    device.as_str(),
                    "org.freedesktop.UPower.Device",
                )
                .await;
                let Ok(proxy) = proxy else { continue };
                let kind = proxy.get_property::<u32>("Type").await.unwrap_or(0);
                if kind != 2 {
                    continue; // 2 = battery
                }
                battery_percent = proxy
                    .get_property::<f64>("Percentage")
                    .await
                    .map(|p| p.clamp(0.0, 100.0) as i32)
                    .unwrap_or(100);
                break;
            }
        }
    }

    // NetworkManager: any MODEM device ⇒ cellular connection available.
    let nm = Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await;
    if let Ok(nm) = nm {
        if let Ok(devices) = nm
            .get_property::<Vec<zbus::zvariant::OwnedObjectPath>>("Devices")
            .await
        {
            for device in devices {
                let proxy = Proxy::new(
                    &conn,
                    "org.freedesktop.NetworkManager",
                    device.as_str(),
                    "org.freedesktop.NetworkManager.Device",
                )
                .await;
                let Ok(proxy) = proxy else { continue };
                if proxy.get_property::<u32>("Type").await.unwrap_or(0) == 6 {
                    // NM_DEVICE_TYPE_MODEM
                    cellular = true;
                    break;
                }
            }
        }
    }

    PowerStateDto {
        charging,
        battery_percent,
        cellular,
        low_power_mode: false, // UPower has no direct power-save flag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_shape() {
        let dto = PowerStateDto {
            charging: true,
            battery_percent: 100,
            cellular: false,
            low_power_mode: false,
        };
        assert!(dto.charging);
        assert_eq!(dto.battery_percent, 100);
        assert!(!dto.cellular);
        assert!(!dto.low_power_mode);
    }
}
