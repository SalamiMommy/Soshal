//! High Dynamic Range (HDR) latency histogram tracking.
//!
//! Provides ultra-low overhead, memory-bounded percentile latency profiling
//! (p50, p90, p95, p99, p99.9) for network requests, media chunk serving,
//! relay responses, and FFI calls using `hdrhistogram`.

use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};

/// Statistical summary of recorded latencies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatencySummary {
    pub count: u64,
    pub min: u64,
    pub max: u64,
    pub mean: f64,
    pub stdev: f64,
    pub p50: u64,
    pub p90: u64,
    pub p95: u64,
    pub p99: u64,
    pub p999: u64,
}

/// HDR latency histogram with configurable precision and auto-resizing.
#[derive(Clone, Debug)]
pub struct LatencyHistogram {
    hist: Histogram<u64>,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new(3).expect("sigfig 3 is within 1..=5")
    }
}

impl LatencyHistogram {
    /// Create a new HDR latency histogram with the given significant figures (1..=5).
    /// Auto-resizing is enabled so values from 1 microsecond up to hours can be tracked.
    pub fn new(sigfig: u8) -> Result<Self, String> {
        let hist = Histogram::<u64>::new(sigfig)
            .map_err(|e| format!("failed to initialize HDR histogram: {e:?}"))?;
        Ok(Self { hist })
    }

    /// Create a new HDR latency histogram with explicit bounds and precision.
    pub fn new_with_bounds(low: u64, high: u64, sigfig: u8) -> Result<Self, String> {
        let hist = Histogram::<u64>::new_with_bounds(low, high, sigfig)
            .map_err(|e| format!("failed to initialize bounded HDR histogram: {e:?}"))?;
        Ok(Self { hist })
    }

    /// Record a single latency sample (e.g. duration in microseconds or milliseconds).
    pub fn record(&mut self, value: u64) -> Result<(), String> {
        self.hist
            .record(value)
            .map_err(|e| format!("failed to record sample {value}: {e:?}"))
    }

    /// Record multiple occurrences of the same latency sample.
    pub fn record_n(&mut self, value: u64, count: u64) -> Result<(), String> {
        self.hist
            .record_n(value, count)
            .map_err(|e| format!("failed to record {count} occurrences of {value}: {e:?}"))
    }

    /// Total number of recorded samples.
    pub fn count(&self) -> u64 {
        self.hist.len()
    }

    /// Returns `true` if no samples have been recorded.
    pub fn is_empty(&self) -> bool {
        self.hist.is_empty()
    }

    /// Minimum recorded value (or 0 if empty).
    pub fn min(&self) -> u64 {
        self.hist.min()
    }

    /// Maximum recorded value (or 0 if empty).
    pub fn max(&self) -> u64 {
        self.hist.max()
    }

    /// Arithmetic mean of all recorded samples.
    pub fn mean(&self) -> f64 {
        self.hist.mean()
    }

    /// Standard deviation of recorded samples.
    pub fn stdev(&self) -> f64 {
        self.hist.stdev()
    }

    /// Value at a given quantile (between 0.0 and 1.0, e.g. 0.50 for median).
    pub fn value_at_quantile(&self, quantile: f64) -> u64 {
        self.hist.value_at_quantile(quantile)
    }

    /// 50th percentile (median) latency.
    pub fn p50(&self) -> u64 {
        self.value_at_quantile(0.50)
    }

    /// 90th percentile latency.
    pub fn p90(&self) -> u64 {
        self.value_at_quantile(0.90)
    }

    /// 95th percentile latency.
    pub fn p95(&self) -> u64 {
        self.value_at_quantile(0.95)
    }

    /// 99th percentile latency.
    pub fn p99(&self) -> u64 {
        self.value_at_quantile(0.99)
    }

    /// 99.9th percentile latency.
    pub fn p999(&self) -> u64 {
        self.value_at_quantile(0.999)
    }

    /// Combine another histogram into this one.
    pub fn add(&mut self, other: &Self) -> Result<(), String> {
        self.hist
            .add(&other.hist)
            .map_err(|e| format!("failed to add HDR histogram: {e:?}"))
    }

    /// Clear all recorded samples and reset the histogram.
    pub fn reset(&mut self) {
        self.hist.reset();
    }

    /// Export a statistical summary snapshot.
    pub fn summary(&self) -> LatencySummary {
        LatencySummary {
            count: self.count(),
            min: self.min(),
            max: self.max(),
            mean: self.mean(),
            stdev: self.stdev(),
            p50: self.p50(),
            p90: self.p90(),
            p95: self.p95(),
            p99: self.p99(),
            p999: self.p999(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_recording_and_percentiles() {
        let mut hist = LatencyHistogram::default();
        assert!(hist.is_empty());
        assert_eq!(hist.count(), 0);

        // Record 1..=100
        for i in 1..=100 {
            hist.record(i).unwrap();
        }

        assert_eq!(hist.count(), 100);
        assert_eq!(hist.min(), 1);
        assert_eq!(hist.max(), 100);

        // p50 is roughly 50 (within 1% precision with 3 sigfigs)
        let p50 = hist.p50();
        assert!((49..=51).contains(&p50), "p50 was {p50}");

        let p90 = hist.p90();
        assert!((89..=91).contains(&p90), "p90 was {p90}");

        let p99 = hist.p99();
        assert!((98..=100).contains(&p99), "p99 was {p99}");
    }

    #[test]
    fn summary_snapshot() {
        let mut hist = LatencyHistogram::new(3).unwrap();
        hist.record_n(10, 50).unwrap();
        hist.record_n(100, 45).unwrap();
        hist.record_n(1000, 5).unwrap();

        let s = hist.summary();
        assert_eq!(s.count, 100);
        assert_eq!(s.min, 10);
        assert_eq!(s.max, 1000);
        assert!(s.p50 <= 100);
        assert!(s.p99 >= 1000);
    }

    #[test]
    fn merge_histograms() {
        let mut h1 = LatencyHistogram::default();
        let mut h2 = LatencyHistogram::default();

        h1.record(100).unwrap();
        h2.record(200).unwrap();

        h1.add(&h2).unwrap();
        assert_eq!(h1.count(), 2);
        assert_eq!(h1.min(), 100);
        assert_eq!(h1.max(), 200);
    }

    #[test]
    fn reset_clears_samples() {
        let mut hist = LatencyHistogram::default();
        hist.record(500).unwrap();
        assert_eq!(hist.count(), 1);

        hist.reset();
        assert_eq!(hist.count(), 0);
        assert!(hist.is_empty());
    }
}
