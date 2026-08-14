//! Relay health scoring tests

use soshal_network_core::relay_health::{backoff_delay, compute_health, update_ema, RelayStatus};

#[test]
fn compute_health_healthy_low_latency() {
    let health = compute_health(50.0, 0);
    assert_eq!(health.status, RelayStatus::Healthy);
    assert!(health.score > 70.0);
}

#[test]
fn compute_health_degraded_medium_latency() {
    let health = compute_health(2500.0, 0);
    assert_eq!(health.status, RelayStatus::Degraded);
    assert!(health.score < 70.0 && health.score > 30.0);
}

#[test]
fn compute_health_unhealthy_high_latency() {
    let health = compute_health(5000.0, 0);
    assert_eq!(health.status, RelayStatus::Unhealthy);
    assert!(health.score < 30.0);
}

#[test]
fn compute_health_failure_penalty() {
    let health_no_failures = compute_health(100.0, 0);
    let health_with_failures = compute_health(100.0, 2);
    assert!(health_with_failures.score < health_no_failures.score);
    // Each failure should deduct 20 points
    assert_eq!(health_no_failures.score - health_with_failures.score, 40.0);
}

#[test]
fn compute_health_score_clamped() {
    let health = compute_health(10000.0, 100);
    assert_eq!(health.score, 0.0);
}

#[test]
fn compute_health_score_upper_bound() {
    let health = compute_health(0.0, 0);
    assert!(health.score <= 100.0);
}

#[test]
fn update_ema_initial_value() {
    let ema = update_ema(0.0, 100.0);
    assert_eq!(ema, 100.0);
}

#[test]
fn update_ema_smoothing() {
    let ema1 = update_ema(100.0, 200.0);
    let ema2 = update_ema(ema1, 200.0);
    // EMA should converge toward the new value
    assert!(ema2 > ema1);
    assert!(ema2 < 200.0);
}

#[test]
fn update_ema_negative_prev() {
    let ema = update_ema(-1.0, 100.0);
    assert_eq!(ema, 100.0);
}

#[test]
fn backoff_delay_no_failures() {
    let delay = backoff_delay(1000, 0);
    assert_eq!(delay, 1000);
}

#[test]
fn backoff_delay_with_failures() {
    let delay1 = backoff_delay(1000, 1);
    let delay2 = backoff_delay(1000, 2);
    let delay3 = backoff_delay(1000, 3);
    assert_eq!(delay1, 2000);
    assert_eq!(delay2, 4000);
    assert_eq!(delay3, 8000);
}

#[test]
fn backoff_delay_capped_at_6_failures() {
    let delay6 = backoff_delay(1000, 6);
    let delay10 = backoff_delay(1000, 10);
    assert_eq!(delay6, delay10);
    assert_eq!(delay6, 1000 * 64);
}

#[test]
fn backoff_delay_zero_base() {
    let delay = backoff_delay(0, 5);
    assert_eq!(delay, 0);
}

#[test]
fn relay_status_comparison() {
    assert!(RelayStatus::Healthy != RelayStatus::Degraded);
    assert!(RelayStatus::Degraded != RelayStatus::Unhealthy);
    assert!(RelayStatus::Healthy == RelayStatus::Healthy);
}

#[test]
fn relay_health_clone() {
    let health1 = compute_health(100.0, 0);
    let health2 = health1.clone();
    assert_eq!(health1.score, health2.score);
    assert_eq!(health1.status, health2.status);
    assert_eq!(health1.ema_latency_ms, health2.ema_latency_ms);
    assert_eq!(health1.failures, health2.failures);
}
