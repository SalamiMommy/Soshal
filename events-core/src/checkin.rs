pub fn can_checkin(event_start: u64, event_end: u64, now: u64, buffer_secs: u64) -> bool {
    let with_buffer = event_start.saturating_sub(buffer_secs);
    now >= with_buffer && now <= event_end
}
