use chrono::{DateTime, TimeZone};
use rrule::{RRuleSet, Tz};

/// Represents an RFC 5545 recurrence rule associated with an event start time,
/// providing typed recurrence expansion and bounded queries for NIP-52 calendar events.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarRecurrence {
    rrule_set: RRuleSet,
}

impl CalendarRecurrence {
    /// Maximum allowed length for an RRULE string to prevent DoS via hostile inputs.
    pub const MAX_RRULE_LEN: usize = 1024;
    /// Default limit on generated occurrences.
    pub const DEFAULT_MAX_OCCURRENCES: usize = 500;

    /// Parse an RFC 5545 recurrence rule associated with a given start timestamp in Unix seconds.
    ///
    /// Accepts:
    /// - Pure rule bodies (e.g. `"FREQ=WEEKLY;INTERVAL=1;COUNT=10"`)
    /// - Prefixed rules (e.g. `"RRULE:FREQ=WEEKLY;INTERVAL=1"`)
    /// - Complete multi-line iCalendar specs (e.g. `"DTSTART:20260901T000000Z\nRRULE:FREQ=DAILY"`)
    pub fn parse(dtstart_unix: i64, rrule_str: &str) -> Result<Self, String> {
        let trimmed = rrule_str.trim();
        if trimmed.is_empty() {
            return Err("Recurrence rule string is empty".to_string());
        }
        if trimmed.len() > Self::MAX_RRULE_LEN {
            return Err(format!(
                "Recurrence rule string length ({}) exceeds limit of {}",
                trimmed.len(),
                Self::MAX_RRULE_LEN
            ));
        }

        let full_spec = if trimmed.starts_with("DTSTART") {
            trimmed.to_string()
        } else {
            let dt = DateTime::from_timestamp(dtstart_unix, 0)
                .ok_or_else(|| format!("Invalid dtstart_unix timestamp: {}", dtstart_unix))?;
            let formatted_dt = dt.format("%Y%m%dT%H%M%SZ").to_string();
            let rule_part = if trimmed.starts_with("RRULE:") {
                trimmed.to_string()
            } else {
                format!("RRULE:{}", trimmed)
            };
            format!("DTSTART:{}\n{}", formatted_dt, rule_part)
        };

        let rrule_set: RRuleSet = full_spec
            .parse()
            .map_err(|e| format!("Failed to parse recurrence rule: {}", e))?;

        Ok(Self { rrule_set })
    }

    /// Returns the initial start timestamp (DTSTART) in Unix seconds.
    pub fn dtstart(&self) -> i64 {
        self.rrule_set.get_dt_start().timestamp()
    }

    /// Access reference to the underlying validated `RRuleSet`.
    pub fn rrule_set(&self) -> &RRuleSet {
        &self.rrule_set
    }

    /// Return all occurrences occurring between `start_unix` and `end_unix` (inclusive),
    /// capped at `max_count`.
    pub fn occurrences_between(
        &self,
        start_unix: i64,
        end_unix: i64,
        max_count: usize,
    ) -> Vec<i64> {
        if start_unix > end_unix || max_count == 0 {
            return Vec::new();
        }

        let start_dt = match DateTime::from_timestamp(start_unix, 0) {
            Some(dt) => Tz::UTC.from_utc_datetime(&dt.naive_utc()),
            None => return Vec::new(),
        };
        let end_dt = match DateTime::from_timestamp(end_unix, 0) {
            Some(dt) => Tz::UTC.from_utc_datetime(&dt.naive_utc()),
            None => return Vec::new(),
        };

        let mut occurrences = Vec::new();
        for dt in self.rrule_set.clone().into_iter() {
            if dt < start_dt {
                continue;
            }
            if dt > end_dt {
                break;
            }
            occurrences.push(dt.timestamp());
            if occurrences.len() >= max_count {
                break;
            }
        }
        occurrences
    }

    /// Find the next occurrence strictly after `after_unix`.
    pub fn next_occurrence(&self, after_unix: i64) -> Option<i64> {
        let after_dt = match DateTime::from_timestamp(after_unix, 0) {
            Some(dt) => Tz::UTC.from_utc_datetime(&dt.naive_utc()),
            None => return None,
        };

        for dt in self.rrule_set.clone().into_iter() {
            if dt > after_dt {
                return Some(dt.timestamp());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daily_recurrence() {
        // Daily for 5 days starting at 2026-09-01 10:00:00 UTC (1788256800)
        let dtstart = 1788256800;
        let rule = "FREQ=DAILY;COUNT=5";
        let recurrence = CalendarRecurrence::parse(dtstart, rule).expect("Failed to parse rule");

        assert_eq!(recurrence.dtstart(), dtstart);

        let occurrences = recurrence.occurrences_between(dtstart, dtstart + 86400 * 10, 10);
        assert_eq!(occurrences.len(), 5);
        for (i, &ts) in occurrences.iter().enumerate() {
            assert_eq!(ts, dtstart + (i as i64) * 86400);
        }

        // Test next occurrence
        let next = recurrence.next_occurrence(dtstart);
        assert_eq!(next, Some(dtstart + 86400));
    }

    #[test]
    fn test_weekly_recurrence_byday() {
        // 2026-09-01 is a Tuesday.
        // Starting 2026-09-01 12:00:00 UTC (1788264000)
        let dtstart = 1788264000;
        // Recur every week on Tuesday and Thursday, count 4
        let rule = "RRULE:FREQ=WEEKLY;BYDAY=TU,TH;COUNT=4";
        let recurrence = CalendarRecurrence::parse(dtstart, rule).expect("Failed to parse rule");

        let occurrences = recurrence.occurrences_between(dtstart, dtstart + 86400 * 30, 100);
        assert_eq!(occurrences.len(), 4);
        // TU (day 0), TH (+2 days), TU (+7 days), TH (+9 days)
        assert_eq!(occurrences[0], dtstart);
        assert_eq!(occurrences[1], dtstart + 2 * 86400);
        assert_eq!(occurrences[2], dtstart + 7 * 86400);
        assert_eq!(occurrences[3], dtstart + 9 * 86400);
    }

    #[test]
    fn test_occurrences_between_range_clipping() {
        let dtstart = 1788256800;
        let rule = "FREQ=DAILY;COUNT=10";
        let recurrence = CalendarRecurrence::parse(dtstart, rule).unwrap();

        // Query only between day 2 and day 4
        let sub = recurrence.occurrences_between(dtstart + 2 * 86400, dtstart + 4 * 86400, 50);
        assert_eq!(sub.len(), 3);
        assert_eq!(sub[0], dtstart + 2 * 86400);
        assert_eq!(sub[1], dtstart + 3 * 86400);
        assert_eq!(sub[2], dtstart + 4 * 86400);

        // Max count cap
        let capped = recurrence.occurrences_between(dtstart, dtstart + 10 * 86400, 2);
        assert_eq!(capped.len(), 2);

        // Inverted range
        let empty = recurrence.occurrences_between(dtstart + 100, dtstart, 10);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_complete_icalendar_spec() {
        let ical = "DTSTART:20260901T100000Z\nRRULE:FREQ=DAILY;COUNT=3";
        let recurrence = CalendarRecurrence::parse(0, ical).expect("Must parse full ical spec");
        assert_eq!(recurrence.dtstart(), 1788256800);
        let occ = recurrence.occurrences_between(0, 2000000000, 10);
        assert_eq!(occ.len(), 3);
    }

    #[test]
    fn test_next_occurrence_boundary() {
        let dtstart = 1788256800;
        let rule = "FREQ=DAILY;COUNT=3";
        let recurrence = CalendarRecurrence::parse(dtstart, rule).unwrap();

        // Next after first is second
        assert_eq!(recurrence.next_occurrence(dtstart), Some(dtstart + 86400));
        // Next after last is None
        assert_eq!(recurrence.next_occurrence(dtstart + 2 * 86400), None);
    }

    #[test]
    fn test_hostile_input_handling() {
        // Empty rule
        assert!(CalendarRecurrence::parse(1000, "").is_err());
        assert!(CalendarRecurrence::parse(1000, "   ").is_err());

        // Oversized rule (> 1024 bytes)
        let huge = "A".repeat(CalendarRecurrence::MAX_RRULE_LEN + 1);
        assert!(CalendarRecurrence::parse(1000, &huge).is_err());

        // Malformed syntax
        assert!(CalendarRecurrence::parse(1000, "INVALID=RULE;NOT=RRULE").is_err());
    }
}
