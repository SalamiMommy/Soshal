//! Thermal & battery-aware P2P seeding scheduler.
//!
//! Background uploads to peers are gated on power state: paused while the
//! device is on battery over cellular (protects the data cap and battery),
//! throttled on battery + Wi-Fi, and ramped to full capacity only while
//! charging on unmetered Wi-Fi. Battery level adds hard floors: below 20%
//! on battery seeding drops to throttled, below 5% it pauses entirely so a
//! dying phone never drains itself serving the mesh. The state is fed from
//! the OS via the bridge (Dart polls the battery/connectivity plugins and
//! pushes values over FFI — cores never touch platform APIs).

use std::sync::RwLock;

/// How aggressively this device serves chunks to peers right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedingMode {
    /// No seeding at all (battery + cellular, or explicit low-power mode).
    Paused,
    /// Seeding allowed but capped (battery + Wi-Fi).
    Throttled,
    /// Full capacity (charging + unmetered Wi-Fi).
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerState {
    pub charging: bool,
    pub battery_percent: u8,
    pub cellular: bool,
    pub low_power_mode: bool,
}

impl Default for PowerState {
    fn default() -> Self {
        Self {
            charging: true,
            battery_percent: 100,
            cellular: false,
            low_power_mode: false,
        }
    }
}

impl PowerState {
    pub fn seeding_mode(&self) -> SeedingMode {
        if self.low_power_mode || (self.cellular && !self.charging) || self.battery_percent < 5 {
            SeedingMode::Paused
        } else if self.battery_percent < 20 && !self.charging {
            SeedingMode::Throttled
        } else if self.charging && !self.cellular {
            SeedingMode::Full
        } else if self.cellular {
            SeedingMode::Paused
        } else {
            SeedingMode::Throttled
        }
    }

    pub fn should_seed(&self) -> bool {
        self.seeding_mode() != SeedingMode::Paused
    }

    /// Parallel upload slots a seeding loop may use.
    pub fn max_parallel_uploads(&self) -> usize {
        match self.seeding_mode() {
            SeedingMode::Paused => 0,
            SeedingMode::Throttled => 2,
            SeedingMode::Full => 8,
        }
    }

    /// Per-peer bytes/sec cap a seeding loop should respect.
    pub fn upload_budget_bytes_per_sec(&self) -> u64 {
        match self.seeding_mode() {
            SeedingMode::Paused => 0,
            SeedingMode::Throttled => 256 * 1024,
            SeedingMode::Full => u64::MAX,
        }
    }

    /// Recommended mDNS discovery advertisement interval in seconds based on power state.
    pub fn mdns_broadcast_interval_secs(&self) -> u64 {
        match self.seeding_mode() {
            SeedingMode::Paused => 60,
            SeedingMode::Throttled => 30,
            SeedingMode::Full => 10,
        }
    }

    /// Recommended maximum concurrent QUIC stream window capacity.
    pub fn quic_max_stream_window(&self) -> u32 {
        match self.seeding_mode() {
            SeedingMode::Paused => 4,
            SeedingMode::Throttled => 16,
            SeedingMode::Full => 64,
        }
    }
}

impl SeedingMode {
    /// True when seeding must be refused entirely right now.
    pub fn paused(&self) -> bool {
        *self == SeedingMode::Paused
    }
}

pub struct PowerScheduler {
    state: RwLock<PowerState>,
}

impl Default for PowerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerScheduler {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(PowerState::default()),
        }
    }

    pub fn update(&self, state: PowerState) {
        if let Ok(mut s) = self.state.write() {
            *s = state;
        }
    }

    pub fn current(&self) -> PowerState {
        *self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn mode(&self) -> SeedingMode {
        self.current().seeding_mode()
    }
}

pub static GLOBAL_POWER_SCHEDULER: std::sync::OnceLock<PowerScheduler> = std::sync::OnceLock::new();

pub fn global_power_scheduler() -> &'static PowerScheduler {
    GLOBAL_POWER_SCHEDULER.get_or_init(PowerScheduler::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_only_when_charging_unmetered() {
        assert_eq!(
            PowerState {
                charging: true,
                cellular: false,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Full
        );
        assert_eq!(
            PowerState {
                charging: true,
                cellular: true,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Paused
        );
    }

    #[test]
    fn battery_wifi_throttles() {
        assert_eq!(
            PowerState {
                charging: false,
                cellular: false,
                battery_percent: 40,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Throttled
        );
        assert_eq!(
            PowerState {
                charging: false,
                cellular: false,
                battery_percent: 40,
                ..PowerState::default()
            }
            .max_parallel_uploads(),
            2
        );
    }

    #[test]
    fn low_power_or_cellular_battery_pauses() {
        assert_eq!(
            PowerState {
                low_power_mode: true,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Paused
        );
        assert!(!PowerState {
            charging: false,
            cellular: true,
            ..PowerState::default()
        }
        .should_seed());
    }

    #[test]
    fn scheduler_update_roundtrip() {
        let s = global_power_scheduler();
        assert_eq!(s.mode(), SeedingMode::Full);
        s.update(PowerState {
            charging: false,
            cellular: false,
            battery_percent: 20,
            low_power_mode: false,
        });
        assert_eq!(s.mode(), SeedingMode::Throttled);
        s.update(PowerState::default());
    }

    #[test]
    fn low_battery_floor_throttles() {
        assert_eq!(
            PowerState {
                charging: false,
                cellular: false,
                battery_percent: 15,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Throttled
        );
    }

    #[test]
    fn critical_battery_pauses_even_while_charging() {
        assert_eq!(
            PowerState {
                charging: true,
                cellular: false,
                battery_percent: 4,
                ..PowerState::default()
            }
            .seeding_mode(),
            SeedingMode::Paused
        );
    }
}
