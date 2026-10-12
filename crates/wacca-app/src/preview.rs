//! Song-select preview audio envelope (`docs/research/05-save-and-song-select.md` §3.3).

/// The cursor must rest this long before a preview starts.
// UNVERIFIED: doc 05 §3.3 suggests ~300 ms.
pub const DEBOUNCE_SECS: f64 = 0.3;
pub const FADE_IN_SECS: f64 = 0.5;
pub const FADE_OUT_SECS: f64 = 1.0;
/// Used when the catalog has no preview length.
pub const DEFAULT_SECS: f64 = 15.0;

/// Song audio from `begin` for `len` seconds, looped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub begin: f64,
    pub len: f64,
}

impl Window {
    pub fn new(begin: f64, len: f64) -> Self {
        Self {
            begin: if begin.is_finite() {
                begin.max(0.0)
            } else {
                0.0
            },
            len: if len.is_finite() && len > 0.0 {
                len
            } else {
                DEFAULT_SECS
            },
        }
    }

    /// BGM gain at audio position `pos`: fade in from `begin`, fade out towards the end.
    pub fn gain(&self, pos: f64) -> f32 {
        let t = pos - self.begin;
        let fade_in = t / FADE_IN_SECS;
        let fade_out = (self.len - t) / FADE_OUT_SECS;
        fade_in.min(fade_out).clamp(0.0, 1.0) as f32
    }

    /// The preview has played out and should start again from `begin`.
    pub fn over(&self, pos: f64) -> bool {
        pos >= self.begin + self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_in_and_out() {
        let w = Window::new(50.0, 10.0);
        assert_eq!(w.gain(50.0), 0.0);
        assert_eq!(w.gain(50.25), 0.5);
        assert_eq!(w.gain(55.0), 1.0);
        assert_eq!(w.gain(59.5), 0.5);
        assert_eq!(w.gain(60.0), 0.0);
        assert_eq!(w.gain(40.0), 0.0);
        assert!(!w.over(59.9) && w.over(60.0));
    }

    #[test]
    fn bad_values_fall_back() {
        let w = Window::new(f64::NAN, 0.0);
        assert_eq!(w, Window::new(0.0, DEFAULT_SECS));
        assert_eq!(Window::new(-3.0, 5.0).begin, 0.0);
    }
}
