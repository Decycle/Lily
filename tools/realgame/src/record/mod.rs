//! Recording the real game: video through an external FFmpeg ([`video`]), WASAPI loopback audio ([`audio`]), and
//! the clock anchors that tie both to the injector's timeline ([`Epoch`], [`meta`]).
//!
//! Every time in a run is stored as milliseconds since one [`Epoch`] (a `std::time::Instant`, which is QPC on
//! Windows); the epoch also records the raw QPC value and the Unix time so external timestamps (FFmpeg's wall
//! clock, WGC/WASAPI QPC stamps) can be mapped onto it.

pub mod audio;
pub mod meta;
pub mod video;
pub mod wav;

use serde::Serialize;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub struct Epoch {
    pub instant: Instant,
    pub anchor: EpochAnchor,
}

/// The same moment on the other clocks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct EpochAnchor {
    pub qpc: i64,
    pub qpc_freq: i64,
    pub unix_s: f64,
}

impl Epoch {
    pub fn now() -> Self {
        let (qpc, qpc_freq) = qpc_now();
        let instant = Instant::now();
        let unix_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        Self {
            instant,
            anchor: EpochAnchor {
                qpc,
                qpc_freq,
                unix_s,
            },
        }
    }

    /// Milliseconds from the epoch to `t` (negative if before).
    pub fn ms(&self, t: Instant) -> f64 {
        match t.checked_duration_since(self.instant) {
            Some(d) => d.as_secs_f64() * 1000.0,
            None => -(self.instant.duration_since(t).as_secs_f64() * 1000.0),
        }
    }

    /// Milliseconds from the epoch to a Unix wall-clock time (as FFmpeg's gdigrab stamps frames).
    pub fn unix_to_ms(&self, unix_s: f64) -> f64 {
        (unix_s - self.anchor.unix_s) * 1000.0
    }
}

#[cfg(windows)]
fn qpc_now() -> (i64, i64) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryPerformanceCounter(out: *mut i64) -> i32;
        fn QueryPerformanceFrequency(out: *mut i64) -> i32;
    }
    let (mut c, mut f) = (0i64, 0i64);
    // SAFETY: out-pointers to live locals; both calls cannot fail on Windows XP+.
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut c);
    }
    (c, f)
}

#[cfg(not(windows))]
fn qpc_now() -> (i64, i64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn epoch_offsets() {
        let e = Epoch::now();
        assert!((e.ms(e.instant + Duration::from_millis(1500)) - 1500.0).abs() < 1e-6);
        if let Some(before) = e.instant.checked_sub(Duration::from_millis(20)) {
            assert!((e.ms(before) + 20.0).abs() < 1e-6);
        }
        assert!((e.unix_to_ms(e.anchor.unix_s + 0.25) - 250.0).abs() < 1e-3);
        #[cfg(windows)]
        assert!(e.anchor.qpc > 0 && e.anchor.qpc_freq > 0);
    }
}
