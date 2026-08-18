//! Scroll-velocity & network-aware predictive media prefetching queue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollState {
    pub velocity: f32, // px/s
    pub top_index: u32,
    pub bottom_index: u32,
}

pub struct Prefetcher {
    last_scroll: Mutex<ScrollState>,
    is_fast_scrolling: AtomicBool,
}

impl Default for Prefetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Prefetcher {
    pub fn new() -> Self {
        Self {
            last_scroll: Mutex::new(ScrollState {
                velocity: 0.0,
                top_index: 0,
                bottom_index: 0,
            }),
            is_fast_scrolling: AtomicBool::new(false),
        }
    }

    pub fn update_scroll_telemetry(&self, velocity: f32, top_index: u32, bottom_index: u32) {
        let is_fast = velocity.abs() > 1500.0;
        self.is_fast_scrolling.store(is_fast, Ordering::Relaxed);

        if let Ok(mut state) = self.last_scroll.lock() {
            *state = ScrollState {
                velocity,
                top_index,
                bottom_index,
            };
        }
    }

    pub fn should_prefetch_media(&self, item_index: u32) -> bool {
        if self.is_fast_scrolling.load(Ordering::Relaxed) {
            // Fast scrolling: cancel media prefetching to save bandwidth & prevent stutter
            return false;
        }

        if let Ok(state) = self.last_scroll.lock() {
            // Prefetch next 5 items below bottom index
            item_index >= state.top_index && item_index <= state.bottom_index + 5
        } else {
            false
        }
    }
}

pub static GLOBAL_PREFETCHER: std::sync::OnceLock<Arc<Prefetcher>> = std::sync::OnceLock::new();

pub fn global_prefetcher() -> &'static Arc<Prefetcher> {
    GLOBAL_PREFETCHER.get_or_init(|| Arc::new(Prefetcher::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_scroll_boundary_is_strict() {
        let p = Prefetcher::new();
        p.update_scroll_telemetry(1500.0, 0, 0);
        assert!(p.should_prefetch_media(0));
        p.update_scroll_telemetry(1500.1, 0, 0);
        assert!(!p.should_prefetch_media(0));
    }

    #[test]
    fn test_global_prefetcher_returns_same_instance() {
        assert!(Arc::ptr_eq(global_prefetcher(), global_prefetcher()));
    }

    #[test]
    fn test_should_prefetch_media_default_state() {
        let p = Prefetcher::new();
        for idx in 0..=5 {
            assert!(p.should_prefetch_media(idx));
        }
        assert!(!p.should_prefetch_media(6));
    }
}
