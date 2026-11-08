//! Coalesce producer notifications without retaining old frames.

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct FrameWake {
    pending: AtomicBool,
}

impl FrameWake {
    /// Post at most one outstanding wake. A failed post must not permanently
    /// suppress later callbacks (the capture timer also provides a fallback).
    pub fn notify(&self, post: impl FnOnce() -> bool) {
        if !self.pending.swap(true, Ordering::AcqRel) && !post() {
            self.pending.store(false, Ordering::Release);
        }
    }

    /// Call only when consuming the posted wake, before draining frames.
    /// A producer arriving during processing may then queue the next wake.
    pub fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// A deadline checked during dispatch, not just when low-priority timer
/// messages happen to be delivered.
pub struct DispatchDeadline {
    next: std::time::Instant,
    interval: std::time::Duration,
}

impl DispatchDeadline {
    pub fn new(now: std::time::Instant, interval: std::time::Duration) -> Self {
        Self {
            next: now,
            interval,
        }
    }

    pub fn due(&mut self, now: std::time::Instant) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + self.interval;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc};
    use std::time::{Duration, Instant};

    #[test]
    fn concurrent_burst_posts_one_wake_and_consumer_rearms() {
        let wake = Arc::new(FrameWake::default());
        let posts = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let wake = wake.clone();
                let posts = posts.clone();
                scope.spawn(move || {
                    for _ in 0..1000 {
                        wake.notify(|| {
                            posts.fetch_add(1, Ordering::Relaxed);
                            true
                        });
                    }
                });
            }
        });
        assert_eq!(posts.load(Ordering::Relaxed), 1);
        wake.acknowledge();
        wake.notify(|| {
            posts.fetch_add(1, Ordering::Relaxed);
            true
        });
        assert_eq!(posts.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn failed_post_does_not_disable_future_notifications() {
        let wake = FrameWake::default();
        wake.notify(|| false);
        let mut posted = false;
        wake.notify(|| {
            posted = true;
            true
        });
        assert!(posted);
    }

    #[test]
    fn deadlines_run_under_continuous_frame_dispatch_without_timer_messages() {
        let start = Instant::now();
        let mut poll = DispatchDeadline::new(start, Duration::from_millis(500));
        let due: Vec<_> = (0..=100)
            .filter(|i| poll.due(start + Duration::from_millis(i * 20)))
            .collect();
        assert_eq!(due, [0, 25, 50, 75, 100]);
    }
}
