//! Traffic shaping peer-blocklist (USER-SPACE ONLY — no actual eBPF).
//!
//! NOTE: no eBPF/BPF program is compiled or attached anywhere. This is an
//! in-process IP blocklist consulted before packet/request processing.
//! `EbpfMode` values are *intended-mechanism* tags only; enforcement is
//! always the in-process `inspect_packet` check. True kernel eBPF (aya)
//! requires root and is a deliberate non-goal on Android (not shippable).

use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Traffic shaper mechanism tag (intended mechanism — not actually attached).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EbpfMode {
    /// Requested kernel TC/XDP eBPF (Linux desktop / root mobile) — NOT implemented
    KernelTcXdp,
    /// Requested socket BPF filter (`SO_ATTACH_BPF`) — NOT implemented
    SocketFilterBpf,
    /// User-space token-bucket fallback (the ONLY enforced path)
    UserSpaceFallback,
}

/// Statistics emitted by the traffic shaper engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbpfShaperStats {
    pub mode: EbpfMode,
    pub dropped_packets: u64,
    pub passed_packets: u64,
    /// Estimated user-space cycles saved per drop (informational)
    pub nanos_saved: u64,
    pub blocked_peers_count: usize,
}

/// Traffic Shaper Manager (in-process IP blocklist; no kernel attachment)
#[derive(Debug, Clone)]
pub struct EbpfShaper {
    mode: EbpfMode,
    blocked_ips: Arc<Mutex<HashSet<String>>>,
    dropped_packets: Arc<AtomicU64>,
    passed_packets: Arc<AtomicU64>,
}

impl Default for EbpfShaper {
    fn default() -> Self {
        Self::new(EbpfMode::UserSpaceFallback).expect("user-space fallback cannot fail")
    }
}

/// Maximum number of blocked IPs allowed in memory to prevent exhaustion.
pub const MAX_BLOCKED_IPS: usize = 10_000;

impl EbpfShaper {
    /// Create a new traffic shaper instance with requested mechanism tag.
    /// Kernel eBPF modes are NOT implemented — returns `Err` for them; only
    /// the user-space fallback path is available.
    pub fn new(requested_mode: EbpfMode) -> Result<Self, String> {
        match requested_mode {
            EbpfMode::KernelTcXdp | EbpfMode::SocketFilterBpf => Err(
                "kernel eBPF mode unavailable: eBPF not compiled, user-space fallback only (roadmap)"
                    .to_string(),
            ),
            EbpfMode::UserSpaceFallback => Ok(Self {
                mode: EbpfMode::UserSpaceFallback,
                blocked_ips: Arc::new(Mutex::new(HashSet::new())),
                dropped_packets: Arc::new(AtomicU64::new(0)),
                passed_packets: Arc::new(AtomicU64::new(0)),
            }),
        }
    }

    /// Block a peer IP (user-space blocklist; no kernel attach).
    /// Validates IP address format and bounds capacity to MAX_BLOCKED_IPS.
    pub fn block_ip(&self, ip: &str) -> bool {
        let trimmed = ip.trim();
        if trimmed.parse::<std::net::IpAddr>().is_err() {
            return false;
        }
        let mut ips = self.blocked_ips.lock().unwrap_or_else(|e| e.into_inner());
        if ips.len() >= MAX_BLOCKED_IPS && !ips.contains(trimmed) {
            return false;
        }
        ips.insert(trimmed.to_string())
    }

    /// Unblock a peer IP address
    pub fn unblock_ip(&self, ip: &str) -> bool {
        let trimmed = ip.trim();
        if trimmed.parse::<std::net::IpAddr>().is_err() {
            return false;
        }
        let mut ips = self.blocked_ips.lock().unwrap_or_else(|e| e.into_inner());
        ips.remove(trimmed)
    }

    /// User-space packet admission check against blocklist.
    /// Returns `true` if allowed, `false` if dropped (in-process, pre-buffer).
    pub fn inspect_packet(&self, src_ip: &str, _payload_len: usize) -> bool {
        let ips = self.blocked_ips.lock().unwrap_or_else(|e| e.into_inner());
        if ips.contains(src_ip.trim()) {
            self.dropped_packets.fetch_add(1, Ordering::SeqCst);
            false
        } else {
            self.passed_packets.fetch_add(1, Ordering::SeqCst);
            true
        }
    }

    /// Get current traffic shaper metrics
    pub fn stats(&self) -> EbpfShaperStats {
        let drops = self.dropped_packets.load(Ordering::SeqCst);
        let passes = self.passed_packets.load(Ordering::SeqCst);
        let blocked = self
            .blocked_ips
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();

        let nanos_saved = drops.saturating_mul(1_200);

        EbpfShaperStats {
            mode: self.mode,
            dropped_packets: drops,
            passed_packets: passes,
            nanos_saved,
            blocked_peers_count: blocked,
        }
    }
}

// Thread-safe singleton instance for FFI API
static GLOBAL_SHAPER: std::sync::OnceLock<EbpfShaper> = std::sync::OnceLock::new();

fn get_global_shaper() -> &'static EbpfShaper {
    GLOBAL_SHAPER.get_or_init(EbpfShaper::default)
}

#[derive(Debug, Deserialize)]
struct BlockIpInput {
    ip: String,
}

#[derive(Debug, Deserialize)]
struct UnblockIpInput {
    ip: String,
}

/// FFI JSON endpoint for blocking an IP address
pub fn ebpf_block_ip_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<BlockIpInput>>(input_json, None) else {
        return json_out(&false, "");
    };
    let res = get_global_shaper().block_ip(&input.ip);
    json_out(&res, "")
}

/// FFI JSON endpoint for unblocking an IP address
pub fn ebpf_unblock_ip_json(input_json: &str) -> String {
    let Some(input) = json_in::<Option<UnblockIpInput>>(input_json, None) else {
        return json_out(&false, "");
    };
    let res = get_global_shaper().unblock_ip(&input.ip);
    json_out(&res, "")
}

/// FFI JSON endpoint for fetching current traffic shaper stats
pub fn ebpf_get_stats_json() -> String {
    let stats = get_global_shaper().stats();
    json_out(&stats, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ebpf_shaper_packet_filtering() {
        let shaper = EbpfShaper::new(EbpfMode::UserSpaceFallback).unwrap();
        assert!(shaper.inspect_packet("192.168.1.50", 128));

        shaper.block_ip("192.168.1.50");
        assert!(!shaper.inspect_packet("192.168.1.50", 128));

        let stats = shaper.stats();
        assert_eq!(stats.dropped_packets, 1);
        assert_eq!(stats.passed_packets, 1);
        assert_eq!(stats.blocked_peers_count, 1);
    }

    #[test]
    fn test_ebpf_kernel_modes_rejected() {
        let err = EbpfShaper::new(EbpfMode::KernelTcXdp).unwrap_err();
        assert!(err.contains("kernel eBPF mode unavailable"));
        let err = EbpfShaper::new(EbpfMode::SocketFilterBpf).unwrap_err();
        assert!(err.contains("kernel eBPF mode unavailable"));
        assert_eq!(
            EbpfShaper::default().stats().mode,
            EbpfMode::UserSpaceFallback
        );
    }

    #[test]
    fn test_ebpf_ip_validation() {
        let shaper = EbpfShaper::new(EbpfMode::UserSpaceFallback).unwrap();
        assert!(!shaper.block_ip("not-an-ip"));
        assert!(!shaper.block_ip("999.999.999.999"));
        assert!(!shaper.block_ip(""));
        assert!(!shaper.unblock_ip("invalid"));

        // Valid IPv4 and IPv6
        assert!(shaper.block_ip("192.168.1.1"));
        assert!(shaper.block_ip("2001:db8::1"));
        assert_eq!(shaper.stats().blocked_peers_count, 2);
    }

    #[test]
    fn test_ebpf_capacity_limit() {
        let shaper = EbpfShaper::new(EbpfMode::UserSpaceFallback).unwrap();
        {
            let mut ips = shaper.blocked_ips.lock().unwrap();
            for i in 0..MAX_BLOCKED_IPS {
                ips.insert(format!("10.0.{}.{}", i / 256, i % 256));
            }
        }
        // At capacity: adding new IP fails
        assert!(!shaper.block_ip("192.168.1.100"));
        assert_eq!(shaper.stats().blocked_peers_count, MAX_BLOCKED_IPS);

        // Re-adding an existing IP returns false because already in set, but doesn't increase count
        assert!(!shaper.block_ip("10.0.0.0"));
        assert_eq!(shaper.stats().blocked_peers_count, MAX_BLOCKED_IPS);
    }
}
