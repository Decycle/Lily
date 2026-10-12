//! Song clock driven by the audio device.
//!
//! Position = seek target + frames consumed by the device / rate − output latency, interpolated
//! with wall time between callbacks, never past the audio already handed to the device, and
//! never going backwards (until the next [`Clock::reset`]). Device-free so it can be unit tested.

use std::time::{Duration, Instant};

/// State captured at the most recent device callback.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    /// Frames consumed before this callback.
    frames: u64,
    /// Frames of audio written by this callback.
    written: u64,
    /// Time from the callback until its first frame is audible.
    latency: Duration,
    at: Instant,
}

/// Monotonic playback clock in seconds of song time.
#[derive(Debug, Clone)]
pub struct Clock {
    rate: f64,
    base: f64,
    generation: u64,
    consumed: u64,
    anchor: Option<Anchor>,
    last: f64,
}

impl Clock {
    pub fn new(sample_rate: u32) -> Self {
        Clock {
            rate: f64::from(sample_rate.max(1)),
            base: 0.0,
            generation: 0,
            consumed: 0,
            anchor: None,
            last: 0.0,
        }
    }

    /// Restart counting at song time `position` (after a seek). Returns the new generation;
    /// callbacks reporting an older generation are ignored.
    pub fn reset(&mut self, position: f64) -> u64 {
        self.generation += 1;
        self.base = position;
        self.consumed = 0;
        self.anchor = None;
        self.last = position;
        self.generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Record a device callback that wrote `written` frames of generation `generation`, whose
    /// first frame will be heard `latency` after `at`.
    pub fn on_callback(&mut self, generation: u64, written: u64, latency: Duration, at: Instant) {
        if generation != self.generation {
            return;
        }
        self.anchor = Some(Anchor {
            frames: self.consumed,
            written,
            latency,
            at,
        });
        self.consumed += written;
    }

    /// Song time being heard at `now`. Before the first callback after a reset it holds the
    /// reset position (the device has not played anything yet).
    pub fn position(&mut self, now: Instant) -> f64 {
        let Some(a) = self.anchor else {
            return self.last;
        };
        let elapsed = now.saturating_duration_since(a.at).as_secs_f64();
        let elapsed = elapsed.min(a.written as f64 / self.rate);
        let raw = self.base + a.frames as f64 / self.rate - a.latency.as_secs_f64() + elapsed;
        self.last = self.last.max(raw);
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn holds_reset_position_before_first_callback() {
        let mut c = Clock::new(48000);
        c.reset(12.5);
        assert_eq!(c.position(Instant::now()), 12.5);
    }

    #[test]
    fn counts_consumed_frames_minus_latency_with_interpolation() {
        let t0 = Instant::now();
        let mut c = Clock::new(1000);
        let g = c.reset(0.0);
        // Two callbacks of 100 frames, 20 ms output latency.
        c.on_callback(g, 100, 20 * MS, t0);
        c.on_callback(g, 100, 20 * MS, t0 + 100 * MS);
        // At the second callback: 100 frames consumed − 20 ms latency.
        assert!((c.position(t0 + 100 * MS) - 0.08).abs() < 1e-9);
        // 50 ms later: interpolated.
        assert!((c.position(t0 + 150 * MS) - 0.13).abs() < 1e-9);
    }

    #[test]
    fn never_runs_past_written_audio_on_underrun() {
        let t0 = Instant::now();
        let mut c = Clock::new(1000);
        let g = c.reset(1.0);
        c.on_callback(g, 10, Duration::ZERO, t0);
        // 1 s later with no new callback: capped at the 10 frames written.
        assert!((c.position(t0 + 1000 * MS) - 1.01).abs() < 1e-9);
    }

    #[test]
    fn is_monotonic_when_latency_grows() {
        let t0 = Instant::now();
        let mut c = Clock::new(1000);
        let g = c.reset(0.0);
        c.on_callback(g, 100, Duration::ZERO, t0);
        let p1 = c.position(t0 + 100 * MS);
        c.on_callback(g, 100, 50 * MS, t0 + 100 * MS);
        let p2 = c.position(t0 + 100 * MS);
        assert!(p2 >= p1);
        assert!((p2 - 0.1).abs() < 1e-9);
    }

    #[test]
    fn stale_generation_is_ignored_and_reset_restarts() {
        let t0 = Instant::now();
        let mut c = Clock::new(1000);
        let old = c.reset(0.0);
        c.on_callback(old, 500, Duration::ZERO, t0);
        let new = c.reset(30.0);
        c.on_callback(old, 500, Duration::ZERO, t0);
        assert_eq!(c.position(t0 + 10 * MS), 30.0);
        c.on_callback(new, 100, Duration::ZERO, t0);
        c.on_callback(new, 100, Duration::ZERO, t0 + 100 * MS);
        assert!((c.position(t0 + 100 * MS) - 30.1).abs() < 1e-9);
        assert_eq!(c.generation(), new);
    }

    #[test]
    fn paused_callbacks_freeze_position() {
        let t0 = Instant::now();
        let mut c = Clock::new(1000);
        let g = c.reset(0.0);
        c.on_callback(g, 100, Duration::ZERO, t0);
        c.on_callback(g, 0, Duration::ZERO, t0 + 100 * MS);
        assert!((c.position(t0 + 5000 * MS) - 0.1).abs() < 1e-9);
    }
}
