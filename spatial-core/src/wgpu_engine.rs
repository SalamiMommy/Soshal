use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// 3D Mesh Node representation for GPU Compute Shader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuMeshNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub latency_ms: u32,
    pub connections: Vec<String>,
}

/// WGPU Session configuration and layout stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgpuSessionConfig {
    pub session_id: i64,
    pub width: u32,
    pub height: u32,
    pub node_count: usize,
    pub frame_rate: u32,
}

/// WGPU Offscreen Compute Engine Session
#[derive(Debug, Clone)]
pub struct WgpuMeshEngineSession {
    pub id: i64,
    pub width: u32,
    pub height: u32,
    pub nodes: Arc<Mutex<Vec<WgpuMeshNode>>>,
    pub frame_counter: Arc<Mutex<u64>>,
}

impl WgpuMeshEngineSession {
    pub fn new(id: i64, width: u32, height: u32) -> Self {
        Self {
            id,
            width,
            height,
            nodes: Arc::new(Mutex::new(Vec::new())),
            frame_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Update or insert mesh nodes into the GPU compute queue
    pub fn set_nodes(&self, nodes: Vec<WgpuMeshNode>) {
        let mut n = self.nodes.lock().unwrap();
        *n = nodes;
    }

    /// Step force-directed 3D physics layout simulation step
    pub fn step_simulation(&self, delta_time: f32) {
        let mut nodes = self.nodes.lock().unwrap();
        let len = nodes.len();
        if len == 0 {
            return;
        }

        // Repulsion force calculation between node pairs
        let mut forces = Vec::with_capacity(len);
        forces.resize(len, (0.0f32, 0.0f32, 0.0f32));

        for i in 0..len {
            for j in (i + 1)..len {
                let dx = nodes[i].x - nodes[j].x;
                let dy = nodes[i].y - nodes[j].y;
                let dz = nodes[i].z - nodes[j].z;
                let dist_sq = (dx * dx + dy * dy + dz * dz).max(0.01);
                let force = 50.0 / dist_sq;

                let fx = (dx / dist_sq.sqrt()) * force;
                let fy = (dy / dist_sq.sqrt()) * force;
                let fz = (dz / dist_sq.sqrt()) * force;

                forces[i].0 += fx;
                forces[i].1 += fy;
                forces[i].2 += fz;

                forces[j].0 -= fx;
                forces[j].1 -= fy;
                forces[j].2 -= fz;
            }
        }

        // Apply forces & integrate positions
        for (i, node) in nodes.iter_mut().enumerate() {
            node.vx = (node.vx + forces[i].0 * delta_time) * 0.92;
            node.vy = (node.vy + forces[i].1 * delta_time) * 0.92;
            node.vz = (node.vz + forces[i].2 * delta_time) * 0.92;

            node.x += node.vx * delta_time;
            node.y += node.vy * delta_time;
            node.z += node.vz * delta_time;
        }

        let mut fc = self.frame_counter.lock().unwrap();
        *fc += 1;
    }

    /// Render offscreen RGBA pixel frame buffer for Flutter TextureRegistry
    pub fn render_pixel_buffer(&self) -> Vec<u8> {
        let mut buffer = vec![0u8; (self.width * self.height * 4) as usize];
        let nodes = self.nodes.lock().unwrap();

        // Background color: Dark mesh canvas #0D1117
        for chunk in buffer.chunks_exact_mut(4) {
            chunk[0] = 0x0D;
            chunk[1] = 0x11;
            chunk[2] = 0x17;
            chunk[3] = 0xFF;
        }

        // Draw node points into RGBA buffer
        for node in nodes.iter() {
            let px = ((node.x + 1.0) * 0.5 * (self.width as f32)) as i32;
            let py = ((node.y + 1.0) * 0.5 * (self.height as f32)) as i32;

            if px >= 0 && px < (self.width as i32) && py >= 0 && py < (self.height as i32) {
                let idx = ((py as u32 * self.width + px as u32) * 4) as usize;
                if idx + 3 < buffer.len() {
                    // Vibrant node accent color (Cyan #00E5FF)
                    buffer[idx] = 0x00;
                    buffer[idx + 1] = 0xE5;
                    buffer[idx + 2] = 0xFF;
                    buffer[idx + 3] = 0xFF;
                }
            }
        }

        buffer
    }
}

/// Global WGPU session registry
#[derive(Default)]
pub struct WgpuEngineManager {
    sessions: Arc<Mutex<HashMap<i64, WgpuMeshEngineSession>>>,
    next_id: Arc<Mutex<i64>>,
}

impl WgpuEngineManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn create_session(&self, width: u32, height: u32) -> i64 {
        let mut id_guard = self.next_id.lock().unwrap();
        let id = *id_guard;
        *id_guard += 1;

        let session = WgpuMeshEngineSession::new(id, width, height);
        self.sessions.lock().unwrap().insert(id, session);
        id
    }

    pub fn get_session(&self, id: i64) -> Option<WgpuMeshEngineSession> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }
}

static WGPU_MANAGER: OnceLock<WgpuEngineManager> = OnceLock::new();

pub fn get_wgpu_manager() -> &'static WgpuEngineManager {
    WGPU_MANAGER.get_or_init(WgpuEngineManager::new)
}

/// FFI endpoint to create a WGPU compute session
pub fn wgpu_create_session_json(width: u32, height: u32) -> String {
    let session_id = get_wgpu_manager().create_session(width, height);
    let cfg = WgpuSessionConfig {
        session_id,
        width,
        height,
        node_count: 0,
        frame_rate: 60,
    };
    serde_json::to_string(&cfg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wgpu_engine_session() {
        let session = WgpuMeshEngineSession::new(1, 100, 100);
        let node1 = WgpuMeshNode {
            id: "n1".to_string(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.1,
            vy: 0.1,
            vz: 0.0,
            latency_ms: 15,
            connections: vec!["n2".to_string()],
        };
        let node2 = WgpuMeshNode {
            id: "n2".to_string(),
            x: 0.5,
            y: 0.5,
            z: 0.0,
            vx: -0.1,
            vy: -0.1,
            vz: 0.0,
            latency_ms: 25,
            connections: vec!["n1".to_string()],
        };

        session.set_nodes(vec![node1, node2]);
        session.step_simulation(0.016);

        let buf = session.render_pixel_buffer();
        assert_eq!(buf.len(), 100 * 100 * 4);
    }
}
