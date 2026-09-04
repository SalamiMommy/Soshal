//! eBPF Traffic Shaper FFI Module

use flutter_rust_bridge::frb;
use soshal_network_core::ebpf::{ebpf_block_ip_json, ebpf_get_stats_json, ebpf_unblock_ip_json};

/// Block a peer IP address in the eBPF kernel / socket filter
#[frb(sync, serialize)]
pub fn ebpf_block_ip(ip: String) -> Result<bool, String> {
    let json_req = serde_json::json!({ "ip": ip }).to_string();
    let res_json = ebpf_block_ip_json(&json_req);
    let res: bool = serde_json::from_str(&res_json).map_err(|e| format!("ebpf parse: {e}"))?;
    Ok(res)
}

/// Unblock a peer IP address in the eBPF kernel / socket filter
#[frb(sync, serialize)]
pub fn ebpf_unblock_ip(ip: String) -> Result<bool, String> {
    let json_req = serde_json::json!({ "ip": ip }).to_string();
    let res_json = ebpf_unblock_ip_json(&json_req);
    let res: bool = serde_json::from_str(&res_json).map_err(|e| format!("ebpf parse: {e}"))?;
    Ok(res)
}

/// Fetch eBPF traffic shaping metrics (dropped packets, nanoseconds saved, active mode)
#[frb(sync, serialize)]
pub fn ebpf_get_stats() -> Result<String, String> {
    Ok(ebpf_get_stats_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_network_core::ebpf::EbpfShaperStats;

    fn blocked_count() -> usize {
        let stats: EbpfShaperStats = serde_json::from_str(&ebpf_get_stats().unwrap()).unwrap();
        stats.blocked_peers_count
    }

    #[test]
    fn test_ebpf_block_unblock_flow() {
        assert_eq!(blocked_count(), 0);

        assert!(ebpf_block_ip("192.168.1.50".to_string()).unwrap());
        assert_eq!(blocked_count(), 1);

        assert!(ebpf_block_ip("192.168.1.51".to_string()).unwrap());
        assert_eq!(blocked_count(), 2);

        assert!(ebpf_unblock_ip("192.168.1.50".to_string()).unwrap());
        assert_eq!(blocked_count(), 1);

        assert!(!ebpf_unblock_ip("203.0.113.9".to_string()).unwrap());
        assert_eq!(blocked_count(), 1);
    }
}
