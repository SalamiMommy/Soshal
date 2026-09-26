use streaming_algorithms::{CountMinSketch, HyperLogLog, Top};

/// High-performance cardinality estimator using HyperLogLog.
/// Estimates the number of distinct elements (e.g. distinct viewers, unique relays)
/// in constant memory ($\approx 1.5$ KB) with bounded relative error ($\approx 2\%$).
#[derive(Clone)]
pub struct StreamCardinality {
    hll: HyperLogLog<String>,
}

impl StreamCardinality {
    /// Create a new HyperLogLog estimator with the target error rate (default: `0.02` for 2% error).
    pub fn new(error_rate: f64) -> Self {
        let err = error_rate.clamp(0.005, 0.2);
        Self {
            hll: HyperLogLog::new(err),
        }
    }

    /// Observe an element in the stream.
    pub fn add(&mut self, item: &str) {
        if !item.is_empty() && item.len() <= 1024 {
            self.hll.push(&item.to_string());
        }
    }

    /// Return the estimated number of distinct elements observed.
    pub fn estimate(&self) -> u64 {
        self.hll.len() as u64
    }

    /// Check if the stream estimator has observed any items.
    pub fn is_empty(&self) -> bool {
        self.estimate() == 0
    }

    /// Union with another HyperLogLog estimator (must have matching error rate).
    pub fn union(&mut self, other: &Self) {
        self.hll.union(&other.hll);
    }
}

impl Default for StreamCardinality {
    fn default() -> Self {
        Self::new(0.02)
    }
}

/// Frequency estimation for high-volume streams using Count-Min Sketch.
/// Tracks event/hashtag frequencies with fixed memory bounds.
#[derive(Clone)]
pub struct StreamFrequency {
    cms: CountMinSketch<String, u64>,
}

impl StreamFrequency {
    /// Construct a Count-Min Sketch.
    /// `error_tolerance` (e.g. 0.001) and `confidence` (e.g. 0.99).
    pub fn new(error_tolerance: f64, confidence: f64) -> Self {
        let eps = error_tolerance.clamp(0.0001, 0.1);
        let conf = confidence.clamp(0.8, 0.9999);
        Self {
            cms: CountMinSketch::new(conf, eps, ()),
        }
    }

    /// Add `count` occurrences of `item`.
    pub fn add(&mut self, item: &str, count: u64) {
        if !item.is_empty() && item.len() <= 1024 && count > 0 {
            self.cms.push(&item.to_string(), &count);
        }
    }

    /// Estimate the frequency of `item`.
    pub fn estimate(&self, item: &str) -> u64 {
        if item.is_empty() || item.len() > 1024 {
            return 0;
        }
        self.cms.get(&item.to_string())
    }
}

impl Default for StreamFrequency {
    fn default() -> Self {
        Self::new(0.001, 0.999)
    }
}

/// Identifies top-K heavy hitters (e.g., trending hashtags/topics) in a stream
/// within fixed memory limits.
#[derive(Clone)]
pub struct StreamTopK {
    top: Top<String, u64>,
}

impl StreamTopK {
    /// Create a new Top-K tracker for the top `k` items.
    pub fn new(k: usize) -> Self {
        let capacity = k.clamp(1, 1000);
        Self {
            top: Top::new(capacity, 0.999, 0.001, ()),
        }
    }

    /// Add `count` occurrences of `item`.
    pub fn add(&mut self, item: &str, count: u64) {
        if !item.is_empty() && item.len() <= 1024 && count > 0 {
            self.top.push(item.to_string(), &count);
        }
    }

    /// Return the current top-K items and their estimated counts, sorted descending by count.
    pub fn top_k(&self) -> Vec<(String, u64)> {
        let mut results: Vec<(String, u64)> = self
            .top
            .iter()
            .map(|(item, count)| (item.clone(), *count))
            .collect();
        results.sort_by_key(|b| std::cmp::Reverse(b.1));
        results
    }
}

impl Default for StreamTopK {
    fn default() -> Self {
        Self::new(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cardinality_estimation() {
        let mut hll = StreamCardinality::new(0.02);
        assert!(hll.is_empty());
        assert_eq!(hll.estimate(), 0);

        // Insert 1000 unique items
        for i in 0..1000 {
            hll.add(&format!("viewer_{}", i));
        }

        // Relative error should be within ~5% for 1000 items
        let estimate = hll.estimate();
        assert!(
            (950..=1050).contains(&estimate),
            "Estimate {} outside expected range",
            estimate
        );
        assert!(!hll.is_empty());
    }

    #[test]
    fn test_cardinality_union() {
        let mut hll1 = StreamCardinality::new(0.02);
        let mut hll2 = StreamCardinality::new(0.02);

        for i in 0..500 {
            hll1.add(&format!("user_{}", i));
        }
        for i in 500..1000 {
            hll2.add(&format!("user_{}", i));
        }

        hll1.union(&hll2);
        let estimate = hll1.estimate();
        assert!((950..=1050).contains(&estimate));
    }

    #[test]
    fn test_frequency_estimation() {
        let mut cms = StreamFrequency::new(0.01, 0.99);
        cms.add("nostr", 50);
        cms.add("bitcoin", 20);
        cms.add("lightning", 10);

        assert_eq!(cms.estimate("nostr"), 50);
        assert_eq!(cms.estimate("bitcoin"), 20);
        assert_eq!(cms.estimate("lightning"), 10);
        assert_eq!(cms.estimate("nonexistent"), 0);
    }

    #[test]
    fn test_top_k_tracking() {
        let mut top = StreamTopK::new(3);
        top.add("nostr", 100);
        top.add("bitcoin", 80);
        top.add("lightning", 60);
        top.add("ai", 20);
        top.add("rust", 10);

        let ranking = top.top_k();
        assert!(ranking.len() <= 3);
        assert_eq!(ranking[0].0, "nostr");
        assert_eq!(ranking[0].1, 100);
        assert_eq!(ranking[1].0, "bitcoin");
        assert_eq!(ranking[1].1, 80);
        assert_eq!(ranking[2].0, "lightning");
        assert_eq!(ranking[2].1, 60);
    }
}
