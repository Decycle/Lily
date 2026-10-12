//! A fully decoded sound effect, shared cheaply between the app and the mixer.

use std::fmt;
use std::sync::Arc;

/// Interleaved PCM of one waveform. Cloning only bumps a reference count.
#[derive(Clone, PartialEq)]
pub struct Sfx {
    pub rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
    /// Sustain loop `[start, end)` in frames (hold sounds); played until the voice is stopped.
    pub loop_range: Option<(usize, usize)>,
}

impl fmt::Debug for Sfx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sfx")
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("frames", &self.frames())
            .field("loop_range", &self.loop_range)
            .finish()
    }
}

impl Sfx {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Linear-interpolated copy at `rate` (loop points scaled); `self` if the rate matches.
    /// The device runs at the BGM rate (48 kHz) and a few SEs are 44.1 or 24 kHz.
    pub fn resampled(&self, rate: u32) -> Sfx {
        if rate == self.rate || self.rate == 0 || rate == 0 {
            return self.clone();
        }
        let ch = usize::from(self.channels.max(1));
        let frames = self.frames();
        let step = f64::from(self.rate) / f64::from(rate);
        let scale = |f: usize| (f as f64 / step).round() as usize;
        let out_frames = scale(frames);
        let mut out = Vec::with_capacity(out_frames * ch);
        for i in 0..out_frames {
            let pos = i as f64 * step;
            let i0 = (pos as usize).min(frames.saturating_sub(1));
            let i1 = (i0 + 1).min(frames.saturating_sub(1));
            let t = (pos - i0 as f64) as f32;
            for c in 0..ch {
                let (a, b) = (self.samples[i0 * ch + c], self.samples[i1 * ch + c]);
                out.push(a + (b - a) * t);
            }
        }
        Sfx {
            rate,
            channels: self.channels,
            samples: out.into(),
            loop_range: self
                .loop_range
                .map(|(s, e)| (scale(s), scale(e).min(out_frames))),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sfx(rate: u32, channels: u16, samples: &[f32]) -> Sfx {
        Sfx {
            rate,
            channels,
            samples: samples.into(),
            loop_range: None,
        }
    }

    #[test]
    fn resamples_linearly_and_scales_loop_points() {
        let mut s = sfx(2, 2, &[0.0, 1.0, 1.0, 0.0, 2.0, 0.0]);
        s.loop_range = Some((1, 3));
        let r = s.resampled(4);
        assert_eq!(r.rate, 4);
        assert_eq!(r.frames(), 6);
        assert_eq!(
            &r.samples[..],
            &[0.0, 1.0, 0.5, 0.5, 1.0, 0.0, 1.5, 0.0, 2.0, 0.0, 2.0, 0.0]
        );
        assert_eq!(r.loop_range, Some((2, 6)));
        assert_eq!(s.resampled(2), s);
        assert!(format!("{s:?}").contains("frames: 3"));
    }
}
