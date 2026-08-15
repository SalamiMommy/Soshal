//! WGPU Native Compute Shaders FFI Module

use flutter_rust_bridge::frb;
use soshal_spatial_core::wgpu_engine::{get_wgpu_manager, wgpu_create_session_json, WgpuMeshNode};

/// Create a new WGPU compute session for rendering offscreen mesh layout
#[frb(sync, serialize)]
pub fn render_create_session(width: u32, height: u32) -> Result<String, String> {
    Ok(wgpu_create_session_json(width, height))
}

/// Step WGPU force-directed graph physics & generate offscreen RGBA pixel frame
#[frb(sync, serialize)]
pub fn render_compute_mesh_frame(
    session_id: i64,
    nodes_json: String,
    delta_time: f32,
) -> Result<Vec<u8>, String> {
    let session = match get_wgpu_manager().get_session(session_id) {
        Some(s) => s,
        None => return Err(format!("Session {} not found", session_id)),
    };

    if !nodes_json.is_empty() {
        if let Ok(nodes) = serde_json::from_str::<Vec<WgpuMeshNode>>(&nodes_json) {
            session.set_nodes(nodes);
        }
    }

    session.step_simulation(delta_time);
    Ok(session.render_pixel_buffer())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_create_session_returns_json() {
        let json = render_create_session(64, 48).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["session_id"].as_i64().unwrap() > 0);
        assert_eq!(v["width"], 64);
        assert_eq!(v["height"], 48);
    }

    #[test]
    fn render_frame_empty_nodes_blank_buffer() {
        let json = render_create_session(8, 8).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = v["session_id"].as_i64().unwrap();
        let buf = render_compute_mesh_frame(id, String::new(), 0.016).unwrap();
        assert_eq!(buf.len(), 8 * 8 * 4, "RGBA w*h*4");
        assert_eq!(buf[0], 0x0D, "bg #0D1117");
        assert_eq!(buf[4], 0x0D);
    }

    #[test]
    fn render_frame_draws_node_pixel() {
        let json = render_create_session(8, 8).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = v["session_id"].as_i64().unwrap();
        let nodes = serde_json::json!([{
            "id": "n1", "x": 0.0, "y": 0.0, "z": 0.0,
            "vx": 0.0, "vy": 0.0, "vz": 0.0,
            "latency_ms": 15, "connections": []
        }])
        .to_string();
        let buf = render_compute_mesh_frame(id, nodes, 0.016).unwrap();
        let center = (4 * 8 * 4 + 4 * 4) as usize;
        assert_eq!(buf[center], 0x00, "cyan node pixel");
        assert_eq!(buf[center + 1], 0xE5);
        assert_eq!(buf[center + 2], 0xFF);
    }

    #[test]
    fn render_invalid_session_errors() {
        let e = render_compute_mesh_frame(999, String::new(), 0.016).unwrap_err();
        assert!(e.contains("Session 999 not found"), "got {e}");
    }

    #[test]
    fn render_bad_nodes_json_ignored() {
        let json = render_create_session(4, 4).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = v["session_id"].as_i64().unwrap();
        let buf = render_compute_mesh_frame(id, "not json".to_string(), 0.016).unwrap();
        assert_eq!(buf.len(), 4 * 4 * 4);
    }
}
