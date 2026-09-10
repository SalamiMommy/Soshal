//! Network heuristics engine: tracking connection quality, RTT, and bandwidth.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionType {
    Wifi = 0,
    Cellular = 1,
    Offline = 2,
}

impl ConnectionType {
    fn from_u8(val: u8) -> Self {
        match val {
            0 => ConnectionType::Wifi,
            1 => ConnectionType::Cellular,
            _ => ConnectionType::Offline,
        }
    }
}

pub struct NetworkHeuristics {
    rtt_ms: AtomicU64,
    bandwidth_kbps: AtomicU64,
    conn_type: AtomicU8,
}

impl Default for NetworkHeuristics {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkHeuristics {
    pub fn new() -> Self {
        Self {
            rtt_ms: AtomicU64::new(50),
            bandwidth_kbps: AtomicU64::new(10_000),
            conn_type: AtomicU8::new(ConnectionType::Wifi as u8),
        }
    }

    pub fn record_sample(&self, rtt: u64, bytes: usize, duration_ms: u64) {
        if let Some(kbps) = (bytes as u64 * 8).checked_div(duration_ms) {
            let old_kbps = self.bandwidth_kbps.load(Ordering::Relaxed);
            let new_kbps = (old_kbps * 7 + kbps * 3) / 10;
            self.bandwidth_kbps.store(new_kbps, Ordering::Relaxed);
        }

        let old_rtt = self.rtt_ms.load(Ordering::Relaxed);
        let new_rtt = (old_rtt * 7 + rtt * 3) / 10;
        self.rtt_ms.store(new_rtt, Ordering::Relaxed);
    }

    pub fn set_connection_type(&self, conn: ConnectionType) {
        self.conn_type.store(conn as u8, Ordering::Relaxed);
    }

    pub fn connection_type(&self) -> ConnectionType {
        ConnectionType::from_u8(self.conn_type.load(Ordering::Relaxed))
    }

    pub fn is_high_bandwidth(&self) -> bool {
        self.conn_type.load(Ordering::Relaxed) == (ConnectionType::Wifi as u8)
            && self.bandwidth_kbps.load(Ordering::Relaxed) > 3_000
    }
}

pub static GLOBAL_HEURISTICS: std::sync::OnceLock<NetworkHeuristics> = std::sync::OnceLock::new();

pub fn global_heuristics() -> &'static NetworkHeuristics {
    GLOBAL_HEURISTICS.get_or_init(NetworkHeuristics::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kbps_samples(h: &NetworkHeuristics, rtt: u64, kbps: u64, count: u64) {
        for _ in 0..count {
            h.record_sample(rtt, (kbps / 8) as usize, 1);
        }
    }

    #[test]
    fn fresh_instance_high_bandwidth() {
        assert!(NetworkHeuristics::new().is_high_bandwidth());
    }

    #[test]
    fn single_sample_weighted_average_keeps_high() {
        let h = NetworkHeuristics::new();
        h.record_sample(100, 125, 1);
        assert!(h.is_high_bandwidth());
    }

    #[test]
    fn zero_duration_skips_bandwidth_update() {
        let h = NetworkHeuristics::new();
        h.record_sample(50, 1024, 0);
        assert!(h.is_high_bandwidth());
    }

    #[test]
    fn many_low_samples_sink_below_threshold() {
        let h = NetworkHeuristics::new();
        kbps_samples(&h, 100, 1000, 100);
        assert!(!h.is_high_bandwidth());
    }

    #[test]
    fn at_threshold_not_high_above_threshold_high() {
        let at = NetworkHeuristics::new();
        kbps_samples(&at, 50, 3000, 100);
        assert!(!at.is_high_bandwidth());
        let above = NetworkHeuristics::new();
        kbps_samples(&above, 50, 3200, 100);
        assert!(above.is_high_bandwidth());
    }

    #[test]
    fn cellular_and_offline_never_high() {
        let h = NetworkHeuristics::new();
        h.set_connection_type(ConnectionType::Cellular);
        assert!(!h.is_high_bandwidth());
        h.set_connection_type(ConnectionType::Offline);
        assert!(!h.is_high_bandwidth());
        h.set_connection_type(ConnectionType::Wifi);
        assert!(h.is_high_bandwidth());
    }

    #[test]
    fn global_heuristics_stable_and_usable() {
        let a = global_heuristics();
        let b = global_heuristics();
        assert!(std::ptr::eq(a, b));
        a.set_connection_type(ConnectionType::Wifi);
        a.record_sample(100, 125, 1);
        assert!(a.is_high_bandwidth());
    }
}
