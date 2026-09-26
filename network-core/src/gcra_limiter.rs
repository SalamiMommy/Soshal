use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

/// Thread-safe, lock-free rate limiter based on the Generic Cell Rate Algorithm (GCRA).
/// Tracks quotas per peer key (e.g., peer public key or IP address).
#[derive(Clone)]
pub struct PeerGcraLimiter {
    limiter: Arc<RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>>,
}

impl PeerGcraLimiter {
    /// Construct a new GCRA limiter with requests-per-second and allowed burst capacity.
    /// Caps inputs to safe bounds (1..=10,000 rps, 1..=50,000 burst).
    pub fn new(requests_per_second: u32, burst_capacity: u32) -> Self {
        let rps = NonZeroU32::new(requests_per_second.clamp(1, 10_000)).unwrap();
        let burst = NonZeroU32::new(burst_capacity.clamp(1, 50_000)).unwrap();
        let quota = Quota::per_second(rps).allow_burst(burst);
        let limiter = Arc::new(RateLimiter::keyed(quota));
        Self { limiter }
    }

    /// Construct with custom duration interval (e.g., N requests per M milliseconds).
    pub fn with_interval(requests: u32, interval_ms: u64, burst_capacity: u32) -> Self {
        let reqs = NonZeroU32::new(requests.clamp(1, 10_000)).unwrap();
        let burst = NonZeroU32::new(burst_capacity.clamp(1, 50_000)).unwrap();
        let duration = Duration::from_millis(interval_ms.clamp(1, 3_600_000));
        let quota = Quota::with_period(duration)
            .unwrap_or_else(|| Quota::per_second(reqs))
            .allow_burst(burst);
        let limiter = Arc::new(RateLimiter::keyed(quota));
        Self { limiter }
    }

    /// Check if a single request from `peer_key` is allowed right now.
    /// Returns `true` if allowed, `false` if rate limit is exceeded.
    pub fn check_peer(&self, peer_key: &str) -> bool {
        if peer_key.is_empty() || peer_key.len() > 256 {
            return false;
        }
        self.limiter.check_key(&peer_key.to_string()).is_ok()
    }

    /// Check if `cost` units from `peer_key` are allowed.
    /// Returns `Ok(())` if allowed, or `Err(wait_ms)` with milliseconds to wait.
    pub fn check_peer_n(&self, peer_key: &str, cost: u32) -> Result<(), u64> {
        if peer_key.is_empty() || peer_key.len() > 256 {
            return Err(1000);
        }
        let n = NonZeroU32::new(cost.max(1)).unwrap();
        match self.limiter.check_key_n(&peer_key.to_string(), n) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(negative)) => {
                let wait_ms = negative
                    .wait_time_from(DefaultClock::default().now())
                    .as_millis() as u64;
                Err(wait_ms.max(1))
            }
            Err(_) => Err(1000), // Cost exceeds max burst capacity
        }
    }
}

impl Default for PeerGcraLimiter {
    fn default() -> Self {
        // Default: 30 requests/sec with burst of 60
        Self::new(30, 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcra_limiter_burst_and_rejection() {
        // 5 rps, burst 5
        let limiter = PeerGcraLimiter::new(5, 5);
        let peer = "peer_alice";

        // First 5 requests should pass (using up burst)
        for _ in 0..5 {
            assert!(limiter.check_peer(peer));
        }

        // 6th immediate request should be rejected by GCRA
        assert!(!limiter.check_peer(peer));

        // Different peer has independent quota
        let bob = "peer_bob";
        assert!(limiter.check_peer(bob));
    }

    #[test]
    fn test_gcra_limiter_cost() {
        let limiter = PeerGcraLimiter::new(10, 10);
        let peer = "peer_carol";

        // Check cost of 5 units (should succeed)
        assert!(limiter.check_peer_n(peer, 5).is_ok());

        // Check cost of 6 more units (exceeds remaining 5 burst)
        let err = limiter.check_peer_n(peer, 6);
        assert!(err.is_err());
        assert!(err.unwrap_err() > 0);
    }

    #[test]
    fn test_empty_or_oversized_peer_rejected() {
        let limiter = PeerGcraLimiter::default();
        assert!(!limiter.check_peer(""));
        let huge = "p".repeat(300);
        assert!(!limiter.check_peer(&huge));
    }
}
