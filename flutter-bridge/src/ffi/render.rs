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
