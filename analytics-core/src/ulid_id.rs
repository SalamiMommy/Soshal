//! Lexicographically sortable 128-bit identifiers (ULID) for telemetry, analytics,
//! and chronological event logging.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use ulid::Ulid;

/// Global thread-safe monotonic ULID generator.
///
/// Ensures sequential ordering even when multiple IDs are generated within the same millisecond.
pub struct MonotonicIdGenerator {
    generator: Mutex<ulid::Generator>,
}

impl Default for MonotonicIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicIdGenerator {
    pub fn new() -> Self {
        Self {
            generator: Mutex::new(ulid::Generator::new()),
        }
    }

    /// Generate the next monotonic ULID.
    pub fn next_ulid(&self) -> Ulid {
        let mut gen = self.generator.lock().unwrap_or_else(|e| e.into_inner());
        gen.generate().unwrap_or_else(|_| Ulid::new())
    }

    /// Generate the next monotonic ULID as a 26-character Crockford Base32 string.
    pub fn next_string(&self) -> String {
        self.next_ulid().to_string()
    }
}

static GLOBAL_GENERATOR: std::sync::LazyLock<MonotonicIdGenerator> =
    std::sync::LazyLock::new(MonotonicIdGenerator::new);

/// Generate a thread-safe, monotonic, chronologically sortable 26-character ID.
pub fn generate_event_id() -> String {
    GLOBAL_GENERATOR.next_string()
}

/// Extract millisecond Unix epoch timestamp from a 26-character ULID string.
pub fn extract_timestamp_ms(id_str: &str) -> Option<u64> {
    Ulid::from_string(id_str).ok().map(|u| u.timestamp_ms())
}

/// Structured analytics event tag equipped with a chronologically sortable identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticsEventRecord {
    /// 128-bit sortable identifier (26-character Crockford Base32).
    pub id: Ulid,
    /// Category / event name (e.g. `feed_impression`, `audio_chunk_delivered`).
    pub event_name: String,
    /// Additional context payload.
    pub payload: serde_json::Value,
}

impl AnalyticsEventRecord {
    /// Create a new record timestamped at generation time.
    pub fn new(event_name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: GLOBAL_GENERATOR.next_ulid(),
            event_name: event_name.into(),
            payload,
        }
    }

    /// Timestamp of this event in milliseconds since Unix epoch.
    pub fn timestamp_ms(&self) -> u64 {
        self.id.timestamp_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_generation_and_sorting() {
        let gen = MonotonicIdGenerator::new();
        let id1 = gen.next_ulid();
        let id2 = gen.next_ulid();
        let id3 = gen.next_ulid();

        assert!(id1 <= id2);
        assert!(id2 <= id3);
        assert_eq!(id1.to_string().len(), 26);
    }

    #[test]
    fn test_extract_timestamp() {
        let id_str = generate_event_id();
        let ts = extract_timestamp_ms(&id_str);
        assert!(ts.is_some());
        let ms = ts.unwrap();
        // Timestamp must be within recent history (after year 2024, ms > 1700000000000)
        assert!(ms > 1_700_000_000_000);
    }

    #[test]
    fn test_event_record_serde() {
        let record = AnalyticsEventRecord::new(
            "post_view",
            serde_json::json!({ "post_id": "abc123", "duration_ms": 1500 }),
        );
        let json = serde_json::to_string(&record).expect("serialize");
        assert!(json.contains("post_view"));
        assert!(json.contains("abc123"));

        let deserialized: AnalyticsEventRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized, record);
        assert_eq!(deserialized.timestamp_ms(), record.timestamp_ms());
    }

    #[test]
    fn test_invalid_ulid_string() {
        assert!(extract_timestamp_ms("not-a-valid-ulid").is_none());
        assert!(extract_timestamp_ms("").is_none());
    }
}
