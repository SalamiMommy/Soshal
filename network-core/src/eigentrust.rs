//! Cryptographic EigenTrust Peer Reputation Engine.
//! Computes global trust scores from local peer transaction feedback matrices.

use std::collections::{HashMap, HashSet};

/// Pre-trusted peers vector alpha weight (damping factor).
pub const EIGENTRUST_ALPHA: f64 = 0.15;
/// Maximum iteration convergence steps.
pub const MAX_EIGENTRUST_ITERATIONS: usize = 50;
/// Convergence epsilon threshold.
pub const CONVERGENCE_EPSILON: f64 = 1e-5;

/// Local transaction experience recorded for a peer.
#[derive(Debug, Clone, Default)]
pub struct PeerAuditScore {
    pub valid_chunks: u64,
    pub invalid_chunks: u64,
    pub total_latency_ms: u64,
    pub successful_transfers: u64,
    pub failed_transfers: u64,
}

impl PeerAuditScore {
    /// Computes local subjective trust value $s_{ij} = \max(0, \text{satisfactory} - \text{unsatisfactory})$.
    pub fn subjective_trust(&self) -> f64 {
        let satisfactory = self.valid_chunks + self.successful_transfers;
        let unsatisfactory = (self.invalid_chunks * 5) + self.failed_transfers;
        if satisfactory > unsatisfactory {
            (satisfactory - unsatisfactory) as f64
        } else {
            0.0
        }
    }
}

/// EigenTrust Engine for computing global reputation vectors across P2P peers.
#[derive(Debug, Default)]
pub struct EigenTrustEngine {
    /// Local trust scores: map of (src_pubkey, dst_pubkey) -> AuditScore
    pub local_matrix: HashMap<(String, String), PeerAuditScore>,
    /// Pre-trusted peer set (bootstrap nodes / trusted web-of-trust seeds)
    pub pre_trusted_peers: Vec<String>,
}

impl EigenTrustEngine {
    pub fn new(pre_trusted_peers: Vec<String>) -> Self {
        Self {
            local_matrix: HashMap::new(),
            pre_trusted_peers,
        }
    }

    /// Record a transaction outcome for a peer.
    pub fn record_audit(&mut self, src: &str, dst: &str, valid: bool, latency_ms: u64) {
        let entry = self
            .local_matrix
            .entry((src.to_string(), dst.to_string()))
            .or_default();
        if valid {
            entry.valid_chunks += 1;
            entry.successful_transfers += 1;
        } else {
            entry.invalid_chunks += 1;
            entry.failed_transfers += 1;
        }
        entry.total_latency_ms += latency_ms;
    }

    /// Computes global EigenTrust reputation scores for a list of peers.
    /// Iterates $t^{(k+1)} = (1 - a) C^T t^{(k)} + a p$ until convergence.
    pub fn compute_global_trust(&self, peers: &[String]) -> HashMap<String, f64> {
        let n = peers.len();
        if n == 0 {
            return HashMap::new();
        }

        let peer_to_idx: HashMap<&str, usize> = peers
            .iter()
            .enumerate()
            .map(|(i, p)| (p.as_str(), i))
            .collect();

        let local_idx: HashMap<(usize, usize), f64> = self
            .local_matrix
            .iter()
            .filter_map(|((src, dst), audit)| {
                let si = *peer_to_idx.get(src.as_str())?;
                let di = *peer_to_idx.get(dst.as_str())?;
                Some(((si, di), audit.subjective_trust()))
            })
            .collect();
        let pre_trusted_set: HashSet<&str> =
            self.pre_trusted_peers.iter().map(|s| s.as_str()).collect();

        // 1. Build normalized sparse trust rows (only rated (i, j) pairs —
        //    dense n×n matrix + full scan per iteration is O(n²·iters)).
        let mut rows: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for ((i, j), trust) in &local_idx {
            if *trust > 0.0 {
                rows[*i].push((*j, *trust));
            }
        }
        for row in rows.iter_mut() {
            let sum_s_ij: f64 = row.iter().map(|(_, v)| *v).sum();
            if sum_s_ij > 0.0 {
                for (_, v) in row.iter_mut() {
                    *v /= sum_s_ij;
                }
            } else {
                // If peer i has no local trust ratings, fallback to pre-trusted distribution
                for (j, pj) in peers.iter().enumerate() {
                    if pre_trusted_set.contains(pj.as_str()) {
                        row.push((j, 1.0 / (self.pre_trusted_peers.len() as f64).max(1.0)));
                    }
                }
                // Normalize fallback row to ensure row-stochasticity
                let sum: f64 = row.iter().map(|(_, v)| *v).sum();
                if sum > 0.0 && (sum - 1.0).abs() > f64::EPSILON {
                    for (_, v) in row.iter_mut() {
                        *v /= sum;
                    }
                }
            }
        }

        // 2. Pre-trusted vector p (length n)
        let mut p = vec![0.0; n];
        let num_pre_trusted = self
            .pre_trusted_peers
            .iter()
            .filter(|pt| peer_to_idx.contains_key(pt.as_str()))
            .count();
        if num_pre_trusted > 0 {
            for pt in &self.pre_trusted_peers {
                if let Some(&idx) = peer_to_idx.get(pt.as_str()) {
                    p[idx] = 1.0 / (num_pre_trusted as f64);
                }
            }
        } else {
            p.fill(1.0 / (n as f64));
        }

        // 3. Power Iteration: t = p initial
        let mut t = p.clone();
        let mut t_next = vec![0.0; n];

        for _ in 0..MAX_EIGENTRUST_ITERATIONS {
            t_next.fill(0.0);

            // t_next = (1 - alpha) * C^T * t + alpha * p
            for (i, row) in rows.iter().enumerate() {
                let ti = t[i];
                if ti == 0.0 {
                    continue;
                }
                for &(j, v) in row {
                    t_next[j] += v * ti;
                }
            }
            for j in 0..n {
                t_next[j] = (1.0 - EIGENTRUST_ALPHA) * t_next[j] + EIGENTRUST_ALPHA * p[j];
            }

            // Check convergence diff
            let diff: f64 = t
                .iter()
                .zip(t_next.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            std::mem::swap(&mut t, &mut t_next);
            if diff < CONVERGENCE_EPSILON {
                break;
            }
        }

        // Map back to peer pubkeys
        peers.iter().cloned().zip(t).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eigentrust_computation() {
        let pre_trusted = vec!["peer_a".to_string()];
        let mut engine = EigenTrustEngine::new(pre_trusted);

        engine.record_audit("peer_a", "peer_b", true, 20);
        engine.record_audit("peer_a", "peer_b", true, 25);
        engine.record_audit("peer_a", "peer_c", false, 500);

        let peers = vec![
            "peer_a".to_string(),
            "peer_b".to_string(),
            "peer_c".to_string(),
        ];
        let scores = engine.compute_global_trust(&peers);

        assert!(scores["peer_b"] > scores["peer_c"]);
    }
}
