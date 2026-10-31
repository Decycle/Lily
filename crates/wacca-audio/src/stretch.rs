//! Time-stretching a BGM source without changing its pitch (practice mode): WSOLA, waveform-
//! similarity overlap-add. Slices of the input, Hann-windowed and overlapping by half, are laid
//! one output hop apart while the read position advances `speed` hops; each slice's start is
//! moved by up to [`SEARCH`] frames to where its waveform best continues the previous one, so
//! the overlaps add up without phasing. Clean from about 0.5× to 2×, grainy below ~0.3×.

use crate::Error;
use crate::hca::Decoder;

/// Slice length in frames (32 ms at 48 kHz).
const WINDOW: usize = 1536;
/// Output hop: half a slice.
const HOP: usize = WINDOW / 2;
/// How far a slice may move to match the previous one (8 ms at 48 kHz).
const SEARCH: i64 = 384;
/// The similarity search compares every `STRIDE`-th frame and tries every `STRIDE`-th offset
/// first, then refines around the best.
const STRIDE: usize = 4;

/// `inner` played at `speed` (2.0 = twice as fast) with its pitch kept. Frame counts and seeks
/// are in output frames; `speed` input seconds go by per output second.
#[derive(Debug)]
pub struct Stretched {
    inner: Box<dyn Decoder>,
    speed: f64,
    channels: usize,
    /// Decoded input (interleaved); `input[0]` is input frame `base`.
    input: Vec<f32>,
    base: i64,
    /// The inner decoder has no more frames.
    eof: bool,
    /// Nominal input position of the next slice.
    next: f64,
    /// Where the previous slice started (`None` right after a seek).
    prev: Option<i64>,
    /// Second half of the previous windowed slice, added under the next one.
    tail: Vec<f32>,
    /// Output frames produced since the start (or the last seek's target).
    out_pos: u64,
    window: Vec<f32>,
}

impl Stretched {
    pub fn new(inner: Box<dyn Decoder>, speed: f64) -> Self {
        let channels = usize::from(inner.channels().max(1));
        // Periodic Hann: two copies half a window apart sum to exactly 1.
        let window = (0..WINDOW)
            .map(|i| {
                let x = i as f32 / WINDOW as f32;
                0.5 - 0.5 * (std::f32::consts::TAU * x).cos()
            })
            .collect();
        Self {
            inner,
            speed: speed.clamp(0.05, 4.0),
            channels,
            input: Vec::new(),
            base: 0,
            eof: false,
            next: 0.0,
            prev: None,
            tail: vec![0.0; HOP * channels],
            out_pos: 0,
            window,
        }
    }

    fn frames(&self) -> i64 {
        (self.input.len() / self.channels) as i64
    }

    /// Decodes until input frame `end` (exclusive) is buffered or the source ends.
    fn fill(&mut self, end: i64) -> Result<(), Error> {
        while !self.eof && self.base + self.frames() < end {
            if !self.inner.decode(&mut self.input)? {
                self.eof = true;
            }
        }
        Ok(())
    }

    /// Input sample of channel `c` at absolute frame `f` (silence outside what is buffered).
    fn at(&self, f: i64, c: usize) -> f32 {
        let i = f - self.base;
        if i < 0 || i >= self.frames() {
            return 0.0;
        }
        self.input[i as usize * self.channels + c]
    }

    /// Mono sample at absolute frame `f`.
    fn mono(&self, f: i64) -> f32 {
        (0..self.channels).map(|c| self.at(f, c)).sum::<f32>() / self.channels as f32
    }

    /// Similarity of the input at `a` and at `b` over half a slice (every `stride`-th frame).
    fn similarity(&self, a: i64, b: i64, stride: usize) -> f32 {
        (0..HOP as i64)
            .step_by(stride)
            .map(|k| self.mono(a + k) * self.mono(b + k))
            .sum()
    }

    /// Start of the next slice: near `target`, where it best continues the previous slice.
    fn best_start(&self, target: i64) -> i64 {
        let Some(prev) = self.prev else {
            return target;
        };
        // What would naturally follow the previous slice's first half.
        let natural = prev + HOP as i64;
        let score = |d: i64| self.similarity(natural, target + d, STRIDE);
        let coarse = (-SEARCH..=SEARCH)
            .step_by(STRIDE)
            .max_by(|&a, &b| score(a).total_cmp(&score(b)))
            .unwrap_or(0);
        let fine = (coarse - STRIDE as i64..=coarse + STRIDE as i64)
            .max_by(|&a, &b| {
                self.similarity(natural, target + a, 1)
                    .total_cmp(&self.similarity(natural, target + b, 1))
            })
            .unwrap_or(coarse);
        target + fine
    }
}

impl Decoder for Stretched {
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_frames(&self) -> u64 {
        (self.inner.total_frames() as f64 / self.speed).round() as u64
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
        // Exactly `total_frames`, so the player's duration and the end of the audio agree.
        let left = self.total_frames().saturating_sub(self.out_pos);
        if left == 0 {
            return Ok(false);
        }
        let emit = (left as usize).min(HOP);
        let target = self.next.round() as i64;
        let need = target + SEARCH + STRIDE as i64 + WINDOW as i64;
        let need = need.max(self.prev.map_or(0, |p| p + WINDOW as i64));
        self.fill(need)?;
        let start = self.best_start(target);
        let ch = self.channels;
        for i in 0..WINDOW {
            let w = self.window[i];
            for c in 0..ch {
                let s = self.at(start + i as i64, c) * w;
                if i < HOP {
                    if i < emit {
                        out.push(self.tail[i * ch + c] + s);
                    }
                } else {
                    self.tail[(i - HOP) * ch + c] = s;
                }
            }
        }
        self.prev = Some(start);
        self.next += HOP as f64 * self.speed;
        self.out_pos += emit as u64;
        // Drop input that no later slice or comparison can reach.
        let keep = (start + HOP as i64).min(self.next.round() as i64 - SEARCH - STRIDE as i64);
        let drop = (keep - self.base).clamp(0, self.frames());
        if drop > 0 {
            self.input.drain(..drop as usize * ch);
            self.base += drop;
        }
        Ok(true)
    }

    fn seek(&mut self, frame: u64) -> Result<(), Error> {
        let input = (frame as f64 * self.speed).round() as u64;
        self.inner.seek(input)?;
        self.input.clear();
        self.base = input as i64;
        self.eof = false;
        self.next = input as f64;
        self.prev = None;
        self.tail.fill(0.0);
        self.out_pos = frame.min(self.total_frames());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mono test source: `n` frames of `f(i)` at 48 kHz, decoded in blocks of 1000.
    #[derive(Debug)]
    struct Fn {
        n: u64,
        at: u64,
        f: fn(u64) -> f32,
    }

    impl Decoder for Fn {
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            48_000
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

    fn tone(i: u64) -> f32 {
        (i as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 0.5
    }

    fn all(s: &mut Stretched) -> Vec<f32> {
        let mut out = Vec::new();
        while s.decode(&mut out).unwrap() {}
        out
    }

    /// Upward zero crossings per second over `x` (skipping the first and last 0.1 s).
    fn pitch(x: &[f32]) -> f64 {
        let x = &x[4800..x.len() - 4800];
        let ups = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        ups as f64 / (x.len() as f64 / 48_000.0)
    }

    #[test]
    fn length_scales_and_pitch_stays() {
        for speed in [0.5, 0.8, 1.5, 2.0] {
            let src = Fn {
                n: 96_000,
                at: 0,
                f: tone,
            };
            let mut s = Stretched::new(Box::new(src), speed);
            let out = all(&mut s);
            let want = 96_000.0 / speed;
            assert_eq!(out.len() as f64, want.round(), "speed {speed}");
            let p = pitch(&out);
            assert!((p - 440.0).abs() < 5.0, "speed {speed}: {p} Hz");
            // Level kept (a 0.5 sine has RMS ~0.354).
            let mid = &out[out.len() / 4..out.len() * 3 / 4];
            let rms = (mid.iter().map(|v| v * v).sum::<f32>() / mid.len() as f32).sqrt();
            assert!((rms - 0.354).abs() < 0.03, "speed {speed}: rms {rms}");
        }
    }

    #[test]
    fn slow_speeds_still_run() {
        let src = Fn {
            n: 24_000,
            at: 0,
            f: tone,
        };
        let mut s = Stretched::new(Box::new(src), 0.1);
        let out = all(&mut s);
        assert_eq!(out.len(), 240_000);
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }

    #[test]
    fn seeks_map_output_to_input_frames() {
        let src = Fn {
            n: 96_000,
            at: 0,
            f: |i| i as f32,
        };
        let mut s = Stretched::new(Box::new(src), 0.5);
        assert_eq!(s.total_frames(), 192_000);
        s.seek(20_000).unwrap();
        // Output frame 20 000 at half speed is input frame 10 000.
        assert_eq!(s.base, 10_000);
        assert_eq!(s.next, 10_000.0);
    }
}
