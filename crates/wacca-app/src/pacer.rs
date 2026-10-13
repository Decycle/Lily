//! Frame pacing for vsync'd presentation.
//!
//! With FIFO presentation the swap chain hands out an image only once the display has taken
//! the previous one, so a loop that renders as fast as it can spends most of every refresh
//! blocked inside `get_current_texture` (measured: ~7 of 8.3 ms at 120 Hz). Touch events
//! arriving meanwhile wait in the message queue and are judged late by up to that long.
//!
//! The pacer instead lets the event loop sleep (handling input the moment it arrives) until
//! [`MARGIN`] before the swap chain is expected to free the next image, so the acquire only
//! blocks for about the margin. Expected times come from the last acquire that blocked (it
//! returned right at a refresh); acquires that did not block keep the predicted phase.

use std::time::{Duration, Instant};

/// How early a frame starts before the next image is expected: covers timer wake-up jitter.
pub const MARGIN: Duration = Duration::from_micros(2000);
/// An acquire at least this long waited for the display (it returned at a refresh).
const BLOCKED: Duration = Duration::from_micros(300);

#[derive(Clone, Copy, Debug)]
pub struct Pacer {
    period: Duration,
    /// When the swap chain last freed an image (actual or predicted).
    ready: Option<Instant>,
}

impl Pacer {
    /// `period`: seconds per refresh; zero disables pacing.
    pub fn new(period: Duration) -> Self {
        Self {
            period,
            ready: None,
        }
    }

    /// An acquire that started at `start` returned at `done`.
    pub fn acquired(&mut self, start: Instant, done: Instant) {
        let blocked = done.saturating_duration_since(start) >= BLOCKED;
        self.ready = match self.ready {
            Some(r) if !blocked => {
                // Keep the phase: the latest predicted refresh at or before `done`.
                let late = done.saturating_duration_since(r).as_secs_f64();
                let n = (late / self.period.as_secs_f64()).floor() as u32;
                Some(r + self.period * n)
            }
            _ => Some(done),
        };
    }

    /// When the next frame should start (`None` = now).
    pub fn next_start(&self) -> Option<Instant> {
        if self.period <= MARGIN {
            return None;
        }
        Some(self.ready? + self.period - MARGIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn starts_a_margin_before_the_next_refresh() {
        let t0 = Instant::now();
        let mut p = Pacer::new(8 * MS);
        assert_eq!(p.next_start(), None);
        // Blocked for 6 ms: the image came free at t0 + 6 ms.
        p.acquired(t0, t0 + 6 * MS);
        assert_eq!(p.next_start(), Some(t0 + 14 * MS - MARGIN));
    }

    #[test]
    fn unblocked_acquires_keep_the_phase() {
        let t0 = Instant::now();
        let mut p = Pacer::new(8 * MS);
        p.acquired(t0, t0 + 4 * MS);
        // Started late (no wait) 11 ms later: the refresh at t0 + 12 ms is the reference.
        p.acquired(t0 + 15 * MS, t0 + 15 * MS);
        assert_eq!(p.next_start(), Some(t0 + 20 * MS - MARGIN));
        // An unblocked acquire before any blocked one sets the phase itself.
        let mut q = Pacer::new(8 * MS);
        q.acquired(t0, t0);
        assert_eq!(q.next_start(), Some(t0 + 8 * MS - MARGIN));
    }

    #[test]
    fn unknown_or_tiny_period_does_not_pace() {
        let t0 = Instant::now();
        let mut p = Pacer::new(Duration::ZERO);
        p.acquired(t0, t0 + 5 * MS);
        assert_eq!(p.next_start(), None);
    }
}
