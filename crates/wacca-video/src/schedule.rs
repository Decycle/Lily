//! Which decoded frame to present for a movie time, and when to restart the decoder. Pure.
//!
//! Policy: the newest decoded frame whose index is ≤ the target is shown and older ones are dropped; a frame
//! not yet due is held back. When the decoder lags far behind, or the time goes backwards, it is restarted at
//! the target (`-ss`).

/// Restart the decoder when the target is this many frames ahead of what it delivers next (1.5 s at 60 fps).
pub const MAX_LAG_FRAMES: u64 = 90;

/// Index of the frame on screen at movie time `t` seconds; `None` before the movie starts.
pub fn frame_index(t: f64, fps: f64) -> Option<u64> {
    (t.is_finite() && t >= 0.0 && fps > 0.0).then(|| (t * fps + 1e-6).floor() as u64)
}

/// `-ss` for a decoder whose first output frame must be `k`: half a frame early so rounding keeps frame `k`.
pub fn seek_secs(k: u64, fps: f64) -> f64 {
    if k == 0 { 0.0 } else { (k as f64 - 0.5) / fps }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Take the due frames from the queue (or hold the current one).
    Pull,
    /// (Re)start decoding at the target frame.
    Restart,
}

/// `next`: index of the earliest frame not yet presented (held-back frame or the decoder's next one), `None` if
/// no decoder runs. `ended`: the decoder has delivered its last frame.
pub fn step(next: Option<u64>, ended: bool, target: u64) -> Step {
    match next {
        None => Step::Restart,
        Some(n) if target + 1 < n => Step::Restart,
        Some(n) if !ended && target > n + MAX_LAG_FRAMES => Step::Restart,
        Some(_) => Step::Pull,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_for_time() {
        assert_eq!(frame_index(-0.01, 60.0), None);
        assert_eq!(frame_index(f64::NAN, 60.0), None);
        assert_eq!(frame_index(0.0, 60.0), Some(0));
        assert_eq!(frame_index(1.0 / 60.0, 60.0), Some(1));
        assert_eq!(frame_index(0.999, 60.0), Some(59));
        assert_eq!(frame_index(10.0, 60.0), Some(600));
    }

    #[test]
    fn seek_keeps_the_target_frame() {
        assert_eq!(seek_secs(0, 60.0), 0.0);
        let s = seek_secs(600, 60.0);
        assert!(s < 10.0 && s > 599.0 / 60.0);
    }

    #[test]
    fn restart_policy() {
        assert_eq!(step(None, false, 0), Step::Restart);
        // Normal playback: next frame is due now or soon.
        assert_eq!(step(Some(11), false, 10), Step::Pull);
        assert_eq!(step(Some(11), false, 11), Step::Pull);
        // Decoder slightly behind: hold and let it catch up.
        assert_eq!(step(Some(11), false, 11 + MAX_LAG_FRAMES), Step::Pull);
        // Far behind (or a seek forward): restart.
        assert_eq!(step(Some(11), false, 12 + MAX_LAG_FRAMES), Step::Restart);
        // Past the end of the movie: hold the last frame, never respawn.
        assert_eq!(step(Some(11), true, 10_000), Step::Pull);
        // Time went backwards: restart.
        assert_eq!(step(Some(11), false, 9), Step::Restart);
        assert_eq!(step(Some(11), true, 9), Step::Restart);
    }
}
