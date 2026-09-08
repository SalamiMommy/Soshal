//! Local vector search module for semantic post search.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocument {
    pub id: String,
    pub embedding: Vec<f32>,
    /// Precomputed squared norm (saved at construction; `0.0` on legacy
    /// deserialized docs triggers a one-time recompute).
    #[serde(default)]
    pub norm: f32,
}

impl VectorDocument {
    pub fn new(id: String, embedding: Vec<f32>) -> Self {
        let norm = embedding_norm(&embedding);
        Self {
            id,
            embedding,
            norm,
        }
    }

    pub fn compute_norm(&self) -> f32 {
        if self.norm > 0.0 {
            self.norm
        } else {
            embedding_norm(&self.embedding)
        }
    }
}

#[inline]
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    let chunks_a = a.chunks_exact(8);
    let chunks_b = b.chunks_exact(8);
    let rem_a = chunks_a.remainder();
    let rem_b = chunks_b.remainder();

    let mut sum0 = 0.0f32;
    let mut sum1 = 0.0f32;
    let mut sum2 = 0.0f32;
    let mut sum3 = 0.0f32;
    let mut sum4 = 0.0f32;
    let mut sum5 = 0.0f32;
    let mut sum6 = 0.0f32;
    let mut sum7 = 0.0f32;

    for (ca, cb) in chunks_a.zip(chunks_b) {
        sum0 += ca[0] * cb[0];
        sum1 += ca[1] * cb[1];
        sum2 += ca[2] * cb[2];
        sum3 += ca[3] * cb[3];
        sum4 += ca[4] * cb[4];
        sum5 += ca[5] * cb[5];
        sum6 += ca[6] * cb[6];
        sum7 += ca[7] * cb[7];
    }

    let mut total = (sum0 + sum1) + (sum2 + sum3) + (sum4 + sum5) + (sum6 + sum7);
    for (x, y) in rem_a.iter().zip(rem_b.iter()) {
        total += x * y;
    }
    total
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot = dot_product(a, b);
    let norm_a = embedding_norm(a);
    let norm_b = embedding_norm(b);

    if norm_a <= 0.0 || norm_b <= 0.0 {
        0.0
    } else {
        let sim = dot / (norm_a * norm_b).sqrt();
        if sim.is_finite() {
            sim.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

pub fn embedding_norm(v: &[f32]) -> f32 {
    let chunks = v.chunks_exact(8);
    let rem = chunks.remainder();
    let mut sum0 = 0.0f32;
    let mut sum1 = 0.0f32;
    let mut sum2 = 0.0f32;
    let mut sum3 = 0.0f32;
    let mut sum4 = 0.0f32;
    let mut sum5 = 0.0f32;
    let mut sum6 = 0.0f32;
    let mut sum7 = 0.0f32;

    for c in chunks {
        sum0 += c[0] * c[0];
        sum1 += c[1] * c[1];
        sum2 += c[2] * c[2];
        sum3 += c[3] * c[3];
        sum4 += c[4] * c[4];
        sum5 += c[5] * c[5];
        sum6 += c[6] * c[6];
        sum7 += c[7] * c[7];
    }

    let mut total = (sum0 + sum1) + (sum2 + sum3) + (sum4 + sum5) + (sum6 + sum7);
    for x in rem {
        total += x * x;
    }
    total
}

pub fn cosine_similarity_with_norms(a: &[f32], norm_a: f32, b: &[f32], norm_b: f32) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }

    let dot = dot_product(a, b);
    let sim = dot / (norm_a * norm_b).sqrt();
    if sim.is_finite() {
        sim.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

pub fn rank_vector_documents(
    query_embedding: &[f32],
    docs: &[VectorDocument],
    top_k: usize,
) -> Vec<(String, f32)> {
    let query_norm = embedding_norm(query_embedding);
    if query_norm <= 0.0 || docs.is_empty() {
        return Vec::new();
    }
    let query_sqrt = query_norm.sqrt();
    let score_doc = |(idx, doc): (usize, &VectorDocument)| {
        let doc_norm = doc.compute_norm();
        let score = if doc_norm <= 0.0 || query_embedding.len() != doc.embedding.len() {
            0.0
        } else {
            let dot = dot_product(query_embedding, &doc.embedding);
            let denom = query_sqrt * doc_norm.sqrt();
            let sim = dot / denom;
            if sim.is_finite() {
                sim.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        (idx, score)
    };

    let mut scored: Vec<(usize, f32)> = if docs.len() >= 64 {
        use rayon::prelude::*;
        docs.par_iter().enumerate().map(score_doc).collect()
    } else {
        docs.iter().enumerate().map(score_doc).collect()
    };

    if top_k < scored.len() {
        scored.select_nth_unstable_by(top_k, |a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    scored
        .into_iter()
        .map(|(idx, score)| (docs[idx].id.clone(), score))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        let v3 = vec![0.0, 1.0, 0.0];

        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 1e-5);
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_handles_nan_and_inf() {
        let v_nan = vec![f32::NAN, 1.0, 0.0];
        let v_normal = vec![1.0, 1.0, 0.0];
        let sim = cosine_similarity(&v_nan, &v_normal);
        assert_eq!(sim, 0.0);

        let v_inf = vec![f32::INFINITY, 0.0, 0.0];
        let sim_inf = cosine_similarity(&v_inf, &v_normal);
        assert_eq!(sim_inf, 0.0);
    }
}
