//! Array chunking utility.

use serde::{Deserialize, Serialize};

use crate::json_util::{json_in, json_out};

#[derive(Deserialize)]
struct ChunkArrayInput {
    arr: Vec<serde_json::Value>,
    size: usize,
}

#[derive(Serialize)]
struct ChunkArrayOutput {
    chunks: Vec<Vec<serde_json::Value>>,
}

/// Splits an array into chunks of the given size.
pub fn chunk_array(arr: Vec<serde_json::Value>, size: usize) -> Vec<Vec<serde_json::Value>> {
    if size == 0 {
        return vec![];
    }
    arr.chunks(size).map(|c| c.to_vec()).collect()
}

/// Accepts JSON `{"arr": [...], "size": N}`, returns JSON `{"chunks": [[...], ...]}`.
pub fn chunk_array_json(input: &str) -> String {
    let input = json_in(
        input,
        ChunkArrayInput {
            arr: Vec::new(),
            size: 0,
        },
    );
    let chunks = chunk_array(input.arr, input.size);
    json_out(&ChunkArrayOutput { chunks }, r#"{"chunks":[]}"#)
}
