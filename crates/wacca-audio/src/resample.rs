//! Streaming sample-rate conversion for BGM sources, for output devices that cannot run at the
//! game's 48 kHz (Windows' shared mode only offers the rate set in the Sound control panel, often
//! 44.1 kHz). 4-point Catmull-Rom interpolation: cheap, and clean enough for a fallback path.

use crate::Error;
use crate::hca::Decoder;

/// Output frames produced per [`Decoder::decode`] call.
const BLOCK: usize = 4096;

/// `inner` converted to `rate`: frame counts, positions and seeks are in output frames, so the
/// player's clock works unchanged.
#[derive(Debug)]
pub struct Resampled {
    inner: Box<dyn Decoder>,
    rate: u32,
    channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Decoded input not yet consumed (interleaved); `src[0]` is input frame `base`.
    src: Vec<f32>,
    /// Position of the next output frame, in input frames relative to `src[0]`.
    pos: f64,
    /// The inner decoder has no more frames.
    eof: bool,
}

impl Resampled {
    pub fn new(inner: Box<dyn Decoder>, rate: u32) -> Self {
        let step = f64::from(inner.sample_rate()) / f64::from(rate.max(1));
        let channels = usize::from(inner.channels().max(1));
        Self {
            inner,
            rate,
            channels,
            step,
            src: Vec::new(),
            pos: 0.0,
            eof: false,
        }
    }

    fn src_frames(&self) -> usize {
        self.src.len() / self.channels
    }

    /// Input sample `frame` of channel `c`, clamped to what is buffered.
    fn at(&self, frame: isize, c: usize) -> f32 {
        let last = self.src_frames().saturating_sub(1) as isize;
        let f = frame.clamp(0, last.max(0)) as usize;
        self.src.get(f * self.channels + c).copied().unwrap_or(0.0)
    }
}

/// Catmull-Rom spline through `p1`–`p2` at `t` in `[0, 1]`.
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * (p1 - p2) + p3 - p0) * t3)
}

impl Decoder for Resampled {
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn total_frames(&self) -> u64 {
        (self.inner.total_frames() as f64 / self.step).round() as u64
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
        // Enough input for a whole block plus the interpolation's look-ahead.
        let want = self.pos + BLOCK as f64 * self.step + 3.0;
        while !self.eof && (self.src_frames() as f64) < want {
            if !self.inner.decode(&mut self.src)? {
                self.eof = true;
            }
        }
        let frames = self.src_frames() as f64;
        let mut produced = 0;
        // Before the end, keep two frames of look-ahead; at the end, run up to the last frame.
        let limit = if self.eof { frames } else { frames - 2.0 };
        while produced < BLOCK && self.pos < limit {
            let i = self.pos.floor() as isize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..self.channels {
                out.push(catmull_rom(
                    self.at(i - 1, c),
                    self.at(i, c),
                    self.at(i + 1, c),
                    self.at(i + 2, c),
                    t,
                ));
            }
            self.pos += self.step;
            produced += 1;
        }
        // Drop consumed input, keeping one frame behind the position for the next spline.
        let keep_from = (self.pos.floor() as usize)
            .saturating_sub(1)
            .min(self.src_frames());
        if keep_from > 0 {
            self.src.drain(..keep_from * self.channels);
            self.pos -= keep_from as f64;
        }
        Ok(produced > 0 || !self.eof)
    }

    fn seek(&mut self, frame: u64) -> Result<(), Error> {
        let input = (frame as f64 * self.step).round() as u64;
        self.inner.seek(input)?;
        self.src.clear();
        self.pos = 0.0;
        self.eof = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mono test source: `n` frames of `f(i)` at `rate`, decoded in blocks of 1000.
    #[derive(Debug)]
    struct Fn {
        rate: u32,
        n: u64,
        at: u64,
        f: fn(u64) -> f32,
    }

    impl Decoder for Fn {
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            self.rate
        }
        fn total_frames(&self) -> u64 {
            self.n
        }
        fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
            let end = (self.at + 1000).min(self.n);
            out.extend((self.at..end).map(self.f));
            self.at = end;
            Ok(self.at < self.n)
        }
        fn seek(&mut self, frame: u64) -> Result<(), Error> {
            self.at = frame.min(self.n);
            Ok(())
        }
    }

    fn all(r: &mut Resampled) -> Vec<f32> {
        let mut out = Vec::new();
        while r.decode(&mut out).unwrap() {}
        out
    }

    #[test]
    fn length_and_rate_scale() {
        let src = Fn {
            rate: 48_000,
            n: 48_000,
            at: 0,
            f: |_| 0.5,
        };
        let mut r = Resampled::new(Box::new(src), 44_100);
        assert_eq!((r.sample_rate(), r.total_frames()), (44_100, 44_100));
        let out = all(&mut r);
        assert!((out.len() as i64 - 44_100).abs() <= 2, "{}", out.len());
        // A constant stays constant (the spline reproduces it exactly).
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn a_tone_keeps_its_pitch_and_level() {
        // 1 kHz at 48 kHz, read at 44.1 kHz: still 1 kHz, i.e. 44.1 samples per period.
        let src = Fn {
            rate: 48_000,
            n: 48_000,
            at: 0,
            f: |i| (i as f32 * std::f32::consts::TAU * 1000.0 / 48_000.0).sin(),
        };
        let mut r = Resampled::new(Box::new(src), 44_100);
        let out = all(&mut r);
        for (k, s) in out.iter().enumerate().take(40_000).skip(100) {
            let want = (k as f32 * std::f32::consts::TAU * 1000.0 / 44_100.0).sin();
            assert!((s - want).abs() < 0.01, "sample {k}: {s} vs {want}");
        }
    }

    #[test]
    fn seeks_are_in_output_frames() {
        let src = Fn {
            rate: 48_000,
            n: 96_000,
            at: 0,
            f: |i| i as f32,
        };
        let mut r = Resampled::new(Box::new(src), 24_000);
        r.seek(12_000).unwrap();
        let mut out = Vec::new();
        r.decode(&mut out).unwrap();
        // Output frame 12000 at 24 kHz = input frame 24000 at 48 kHz (a ramp: value = frame).
        assert!((out[0] - 24_000.0).abs() < 1e-3, "{}", out[0]);
        assert!((out[1] - 24_002.0).abs() < 1e-3, "{}", out[1]);
    }
}
