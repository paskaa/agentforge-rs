//! Quota monitor — detects QPM rate limiting and backs off.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static RATE_LIMITED: AtomicBool = AtomicBool::new(false);
static LAST_LIMIT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Mark that we've been rate limited.
pub fn mark_rate_limited() {
    RATE_LIMITED.store(true, Ordering::SeqCst);
    if let Ok(mut last) = LAST_LIMIT.lock() {
        *last = Some(Instant::now());
    }
}

/// Check if we're currently rate limited.
/// Auto-resets after 60 seconds.
pub fn is_rate_limited() -> bool {
    if !RATE_LIMITED.load(Ordering::SeqCst) {
        return false;
    }
    if let Ok(last) = LAST_LIMIT.lock() {
        if let Some(instant) = *last {
            if instant.elapsed() > Duration::from_secs(60) {
                RATE_LIMITED.store(false, Ordering::SeqCst);
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_cycle() {
        assert!(!is_rate_limited());
        mark_rate_limited();
        assert!(is_rate_limited());
        // Can't easily test auto-reset in unit test
    }
}
