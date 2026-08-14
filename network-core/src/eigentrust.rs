//! Cryptographic EigenTrust Peer Reputation Engine.
//! Computes global trust scores from local peer transaction feedback matrices.

use std::collections::HashMap;

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

        // 1. Build normalized local trust matrix C (n x n)
        let mut c = vec![vec![0.0; n]; n];

        for i in 0..n {
            let mut sum_s_ij = 0.0;
            for j in 0..n {
                if i == j {
                    continue;
                }
                if let Some(audit) = self.local_matrix.get(&(peers[i].clone(), peers[j].clone())) {
                    let trust = audit.subjective_trust();
                    c[i][j] = trust;
                    sum_s_ij += trust;
                }
            }
            if sum_s_ij > 0.0 {
                for v in c[i].iter_mut() {
                    *v /= sum_s_ij;
                }
            } else {
                // If peer i has no local trust ratings, fallback to pre-trusted distribution
                for j in 0..n {
                    if self.pre_trusted_peers.contains(&peers[j]) {
                        c[i][j] = 1.0 / (self.pre_trusted_peers.len() as f64).max(1.0);
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

        for _ in 0..MAX_EIGENTRUST_ITERATIONS {
            let mut t_next = vec![0.0; n];

            // t_next = (1 - alpha) * C^T * t + alpha * p
            for j in 0..n {
                let mut c_t_j = 0.0;
                for i in 0..n {
                    c_t_j += c[i][j] * t[i];
                }
                t_next[j] = (1.0 - EIGENTRUST_ALPHA) * c_t_j + EIGENTRUST_ALPHA * p[j];
            }

            // Check convergence diff
            let diff: f64 = t
                .iter()
                .zip(t_next.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            t = t_next;
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
