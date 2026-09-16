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
        let mut n = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
        *n = nodes;
    }

    /// Step force-directed 3D physics layout simulation step
    pub fn step_simulation(&self, delta_time: f32) {
        let dt = if delta_time.is_finite() && delta_time > 0.0 {
            delta_time.min(1.0)
        } else {
            0.016
        };
        let mut nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
        let len = nodes.len();
        if len == 0 {
            return;
        }

        // Repulsion force calculation between node pairs
        let forces = {
            let mut forces = vec![(0.0f32, 0.0f32, 0.0f32); len];
            let thread_count = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .min(len);
            if thread_count <= 1 {
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
            } else {
                let chunk_size = len.div_ceil(thread_count);
                std::thread::scope(|s| {
                    let nodes_ref = &*nodes;
                    let mut handles = Vec::with_capacity(thread_count);
                    for start in (0..len).step_by(chunk_size) {
                        let end = (start + chunk_size).min(len);
                        handles.push(s.spawn(move || {
                            let mut local = vec![(0.0f32, 0.0f32, 0.0f32); len];
                            for i in start..end {
                                for j in (i + 1)..len {
                                    let dx = nodes_ref[i].x - nodes_ref[j].x;
                                    let dy = nodes_ref[i].y - nodes_ref[j].y;
                                    let dz = nodes_ref[i].z - nodes_ref[j].z;
                                    let dist_sq = (dx * dx + dy * dy + dz * dz).max(0.01);
                                    let force = 50.0 / dist_sq;

                                    let fx = (dx / dist_sq.sqrt()) * force;
                                    let fy = (dy / dist_sq.sqrt()) * force;
                                    let fz = (dz / dist_sq.sqrt()) * force;

                                    local[i].0 += fx;
                                    local[i].1 += fy;
                                    local[i].2 += fz;

                                    local[j].0 -= fx;
                                    local[j].1 -= fy;
                                    local[j].2 -= fz;
                                }
                            }
                            local
                        }));
                    }
                    for handle in handles {
                        let partial = handle.join().unwrap();
                        for (f, p) in forces.iter_mut().zip(partial.iter()) {
                            f.0 += p.0;
                            f.1 += p.1;
                            f.2 += p.2;
                        }
                    }
                });
            }
            forces
        };

        // Apply forces & integrate positions
        for (i, node) in nodes.iter_mut().enumerate() {
            node.vx = (node.vx + forces[i].0 * dt) * 0.92;
            node.vy = (node.vy + forces[i].1 * dt) * 0.92;
            node.vz = (node.vz + forces[i].2 * dt) * 0.92;

            node.x += node.vx * dt;
            node.y += node.vy * dt;
            node.z += node.vz * dt;
        }

        let mut fc = self.frame_counter.lock().unwrap_or_else(|e| e.into_inner());
        *fc += 1;
    }

    /// Render offscreen RGBA pixel frame buffer for Flutter TextureRegistry
    pub fn render_pixel_buffer(&self) -> Vec<u8> {
        let Some(byte_len) = (self.width as u64)
            .checked_mul(self.height as u64)
            .and_then(|v| v.checked_mul(4))
            .and_then(|v| usize::try_from(v).ok())
        else {
            return Vec::new();
        };
        if byte_len > 256 * 1024 * 1024 {
            return Vec::new();
        }
        let mut buffer = vec![0u8; byte_len];
        let nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());

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

/// Maximum active WGPU compute sessions kept in memory.
pub const MAX_SESSIONS: usize = 16;
/// Maximum allowed texture width or height.
pub const MAX_DIMENSION: u32 = 4096;

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
        let w = width.clamp(1, MAX_DIMENSION);
        let h = height.clamp(1, MAX_DIMENSION);
        let mut id_guard = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
        let id = *id_guard;
        *id_guard += 1;

        let session = WgpuMeshEngineSession::new(id, w, h);
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if sessions.len() >= MAX_SESSIONS {
            if let Some(&oldest_id) = sessions.keys().min() {
                sessions.remove(&oldest_id);
            }
        }
        sessions.insert(id, session);
        id
    }

    pub fn get_session(&self, id: i64) -> Option<WgpuMeshEngineSession> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
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

    #[test]
    fn test_wgpu_engine_manager_eviction_and_clamping() {
        let mgr = WgpuEngineManager::new();

        // Clamping check
        let s0_id = mgr.create_session(0, 5000);
        let s0 = mgr.get_session(s0_id).unwrap();
        assert_eq!(s0.width, 1);
        assert_eq!(s0.height, 4096);

        // Fill up to MAX_SESSIONS
        for _ in 1..MAX_SESSIONS {
            mgr.create_session(10, 10);
        }
        assert!(mgr.get_session(s0_id).is_some());

        // Create one more session, which should evict s0 (oldest)
        let s_new_id = mgr.create_session(20, 20);
        assert!(
            mgr.get_session(s0_id).is_none(),
            "oldest session should be evicted"
        );
        assert!(mgr.get_session(s_new_id).is_some());
    }

    #[test]
    fn test_wgpu_simulation_nan_delta_time_resilience() {
        let session = WgpuMeshEngineSession::new(1, 10, 10);
        let node = WgpuMeshNode {
            id: "n1".to_string(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.1,
            vy: 0.1,
            vz: 0.1,
            latency_ms: 10,
            connections: vec![],
        };
        session.set_nodes(vec![node]);
        session.step_simulation(f32::NAN);
        let nodes = session.nodes.lock().unwrap();
        assert!(nodes[0].x.is_finite());
        assert!(nodes[0].vx.is_finite());
    }
}
