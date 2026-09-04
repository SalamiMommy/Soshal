use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[derive(Debug, Deserialize)]
struct RateLimitInput {
    attempts: u32,
    first_attempt: u64,
    now: u64,
    max_attempts: u32,
    window_ms: u64,
    blocked_until: Option<u64>,
}

#[derive(Debug, Serialize)]
struct RateLimitOutput {
    allowed: bool,
    remaining_attempts: u32,
    retry_after: Option<u64>,
    new_attempts: u32,
    new_first_attempt: u64,
    new_blocked_until: Option<u64>,
}

fn rate_limit_check(input: &RateLimitInput) -> RateLimitOutput {
    let now = input.now;

    if let Some(blocked_until) = input.blocked_until {
        if now < blocked_until {
            return RateLimitOutput {
                allowed: false,
                remaining_attempts: 0,
                retry_after: Some(blocked_until - now),
                new_attempts: input.attempts,
                new_first_attempt: input.first_attempt,
                new_blocked_until: input.blocked_until,
            };
        }
    }

    if now.saturating_sub(input.first_attempt) >= input.window_ms {
        let remaining = input.max_attempts.saturating_sub(1);
        return RateLimitOutput {
            allowed: true,
            remaining_attempts: remaining,
            retry_after: None,
            new_attempts: 1,
            new_first_attempt: now,
            new_blocked_until: None,
        };
    }

    if input.attempts >= input.max_attempts {
        let exp = input.attempts.min(30);
        let backoff_secs = 2u64.saturating_pow(exp).min(3600);
        let backoff_ms = backoff_secs * 1000;
        let blocked_until = now + backoff_ms;

        return RateLimitOutput {
            allowed: false,
            remaining_attempts: 0,
            retry_after: Some(backoff_ms),
            new_attempts: input.attempts,
            new_first_attempt: input.first_attempt,
            new_blocked_until: Some(blocked_until),
        };
    }

    let new_attempts = input.attempts + 1;
    let remaining = input.max_attempts.saturating_sub(new_attempts);
    RateLimitOutput {
        allowed: true,
        remaining_attempts: remaining,
        retry_after: None,
        new_attempts,
        new_first_attempt: input.first_attempt,
        new_blocked_until: None,
    }
}

pub fn rate_limit_check_json(input: &str) -> String {
    let Some(input) = json_in::<Option<RateLimitInput>>(input, None) else {
        return String::new();
    };
    let output = rate_limit_check(&input);
    json_out(&output, "")
}
