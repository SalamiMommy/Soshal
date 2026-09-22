const ALPHA: f64 = 0.3;
const LATENCY_DIVISOR: f64 = 50.0;
const FAILURE_PENALTY: f64 = 20.0;
const DEGRADED_THRESHOLD: f64 = 70.0;
const UNHEALTHY_THRESHOLD: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelayStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone)]
pub struct RelayHealth {
    pub score: f64,
    pub status: RelayStatus,
    pub ema_latency_ms: f64,
    pub failures: u32,
}

pub fn compute_health(ema_latency_ms: f64, failures: u32) -> RelayHealth {
    let penalty = failures as f64 * FAILURE_PENALTY;
    // Non-finite (NaN/±inf) latency is hostile or broken input: score it as
    // the worst case instead of letting NaN comparisons classify it Healthy.
    let latency = if ema_latency_ms.is_finite() {
        ema_latency_ms.max(0.0)
    } else {
        f64::INFINITY
    };
    let raw = 100.0 - (latency / LATENCY_DIVISOR) - penalty;
    let score = raw.clamp(0.0, 100.0);
    let status = if score < UNHEALTHY_THRESHOLD {
        RelayStatus::Unhealthy
    } else if score < DEGRADED_THRESHOLD {
        RelayStatus::Degraded
    } else {
        RelayStatus::Healthy
    };
    RelayHealth {
        score,
        status,
        ema_latency_ms,
        failures,
    }
}

pub fn update_ema(prev_ema_ms: f64, new_latency_ms: f64) -> f64 {
    if !prev_ema_ms.is_finite() || prev_ema_ms <= 0.0 {
        new_latency_ms
    } else {
        ALPHA * new_latency_ms + (1.0 - ALPHA) * prev_ema_ms
    }
}

pub fn backoff_delay(base_ms: u64, failures: u32) -> u64 {
    base_ms.saturating_mul(1u64 << failures.min(6))
}
