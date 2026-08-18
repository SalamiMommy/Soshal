use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::json_util::{json_in, json_out};

const MAX_EDGES: usize = 1_000_000;
const MAX_ITERATIONS: usize = 300;
const MAX_NODE_ID_LEN: usize = 200;
const MAX_NODE_LABEL_LEN: usize = 200;
const LARGE_GRAPH_NODE_COUNT: usize = 200;
const LARGE_GRAPH_ITERATIONS: usize = 60;
const CONVERGENCE_EPSILON: f64 = 0.1;

#[derive(Deserialize)]
struct ForceLayoutNodeInput {
    id: String,
    label: String,
    radius: Option<f64>,
    color: Option<String>,
}

#[derive(Deserialize)]
struct ForceLayoutEdgeInput {
    source: String,
    target: String,
}

#[derive(Deserialize)]
struct ForceLayoutInput {
    nodes: Vec<ForceLayoutNodeInput>,
    edges: Vec<ForceLayoutEdgeInput>,
    width: f64,
    height: f64,
    iterations: Option<usize>,
    initial_positions: Option<Vec<(f64, f64)>>,
}

#[derive(Serialize, Deserialize)]
struct ForceLayoutNodeOut {
    id: String,
    label: String,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    radius: f64,
    color: String,
}

fn calculate_force_layout(input: ForceLayoutInput) -> Vec<ForceLayoutNodeOut> {
    if input.nodes.len() > 500
        || input.edges.len() > MAX_EDGES
        || input
            .iterations
            .map(|i| i > MAX_ITERATIONS)
            .unwrap_or(false)
    {
        return Vec::new();
    }
    if !input.width.is_finite()
        || !input.height.is_finite()
        || input.width <= 0.0
        || input.height <= 0.0
        || input.width > 100_000.0
        || input.height > 100_000.0
    {
        return Vec::new();
    }
    let center_x = input.width / 2.0;
    let center_y = input.height / 2.0;
    let iterations = input.iterations.unwrap_or(100);
    let mut nodes: Vec<ForceLayoutNodeOut> = Vec::with_capacity(input.nodes.len());
    let initial_pos = input.initial_positions.unwrap_or_default();
    for (i, nd) in input.nodes.iter().enumerate() {
        if nd.id.len() > MAX_NODE_ID_LEN || nd.label.len() > MAX_NODE_LABEL_LEN {
            continue;
        }
        let (ix, iy) = initial_pos.get(i).copied().unwrap_or((center_x, center_y));
        let (ix, iy) = if ix.is_finite() && iy.is_finite() {
            (ix, iy)
        } else {
            (center_x, center_y)
        };
        let radius = nd.radius.unwrap_or(20.0).clamp(1.0, 10_000.0);
        nodes.push(ForceLayoutNodeOut {
            id: nd.id.clone(),
            label: nd.label.clone(),
            x: ix,
            y: iy,
            vx: 0.0,
            vy: 0.0,
            radius,
            color: nd.color.clone().unwrap_or_else(|| "#888".to_string()),
        });
    }
    let node_count = nodes.len();
    let iterations = iterations.min(if node_count > LARGE_GRAPH_NODE_COUNT {
        LARGE_GRAPH_ITERATIONS
    } else {
        MAX_ITERATIONS
    });
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    let mut id_to_index: HashMap<&str, usize> = HashMap::with_capacity(node_count);
    for (i, n) in nodes.iter().enumerate() {
        id_to_index.insert(n.id.as_str(), i);
    }
    for edge in &input.edges {
        if let (Some(&si), Some(&ti)) = (
            id_to_index.get(edge.source.as_str()),
            id_to_index.get(edge.target.as_str()),
        ) {
            adjacency[si].push(ti);
            adjacency[ti].push(si);
        }
    }
    let mut forces = vec![(0.0f64, 0.0f64); node_count];
    let mut still_streak: usize = 0;
    for _iter in 0..iterations {
        forces.fill((0.0, 0.0));
        for i in 0..node_count {
            for j in (i + 1)..node_count {
                let dx = nodes[i].x - nodes[j].x;
                let dy = nodes[i].y - nodes[j].y;
                if dx == 0.0 && dy == 0.0 {
                    continue;
                }
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);
                let force = 3000.0 / (dist * dist);
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;
                forces[i].0 += fx;
                forces[i].1 += fy;
                forces[j].0 -= fx;
                forces[j].1 -= fy;
            }
        }
        let mut total_movement = 0.0f64;
        for i in 0..node_count {
            let mut fx = forces[i].0;
            let mut fy = forces[i].1;
            for &neighbor_idx in &adjacency[i] {
                fx += (nodes[neighbor_idx].x - nodes[i].x) * 0.005;
                fy += (nodes[neighbor_idx].y - nodes[i].y) * 0.005;
            }
            fx += (center_x - nodes[i].x) * 0.01;
            fy += (center_y - nodes[i].y) * 0.01;
            nodes[i].vx = (nodes[i].vx + fx) * 0.9;
            nodes[i].vy = (nodes[i].vy + fy) * 0.9;
            total_movement += nodes[i].vx.abs() + nodes[i].vy.abs();
            nodes[i].x += nodes[i].vx;
            nodes[i].y += nodes[i].vy;
            if !nodes[i].x.is_finite() {
                nodes[i].x = center_x;
            }
            if !nodes[i].y.is_finite() {
                nodes[i].y = center_y;
            }
            let margin = 40.0;
            nodes[i].x = nodes[i].x.max(margin).min(input.width - margin);
            nodes[i].y = nodes[i].y.max(margin).min(input.height - margin);
        }
        if total_movement < CONVERGENCE_EPSILON * node_count as f64 {
            still_streak += 1;
            if still_streak >= 2 {
                break;
            }
        } else {
            still_streak = 0;
        }
    }
    nodes
}

pub fn calculate_force_layout_json(input: &str) -> String {
    let parsed = json_in(
        input,
        ForceLayoutInput {
            nodes: Vec::new(),
            edges: Vec::new(),
            width: 0.0,
            height: 0.0,
            iterations: None,
            initial_positions: None,
        },
    );
    let out = calculate_force_layout(parsed);
    json_out(&out, "[]")
}
