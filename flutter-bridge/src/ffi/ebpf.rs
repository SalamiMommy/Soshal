//! eBPF Traffic Shaper FFI Module

use flutter_rust_bridge::frb;
use soshal_network_core::ebpf::{ebpf_block_ip_json, ebpf_get_stats_json, ebpf_unblock_ip_json};

/// Block a peer IP address in the eBPF kernel / socket filter
#[frb(sync, serialize)]
pub fn ebpf_block_ip(ip: String) -> Result<bool, String> {
    let json_req = serde_json::json!({ "ip": ip }).to_string();
    let res_json = ebpf_block_ip_json(&json_req);
    let res: bool = serde_json::from_str(&res_json).unwrap_or(false);
    Ok(res)
}

/// Unblock a peer IP address in the eBPF kernel / socket filter
#[frb(sync, serialize)]
pub fn ebpf_unblock_ip(ip: String) -> Result<bool, String> {
    let json_req = serde_json::json!({ "ip": ip }).to_string();
    let res_json = ebpf_unblock_ip_json(&json_req);
    let res: bool = serde_json::from_str(&res_json).unwrap_or(false);
    Ok(res)
}

/// Fetch eBPF traffic shaping metrics (dropped packets, nanoseconds saved, active mode)
#[frb(sync, serialize)]
pub fn ebpf_get_stats() -> Result<String, String> {
    Ok(ebpf_get_stats_json())
}
