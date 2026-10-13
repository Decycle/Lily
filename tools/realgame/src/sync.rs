//! Song-time sync (doc 07 §3): find where the game's BGM starts in the loopback recording by correlating it with
//! the BGM we decode ourselves, so every harness timestamp can be mapped to chart time.
//!
//! Two stages, both Pearson correlation: a 1 ms energy envelope of the first [`DetectorOptions::envelope_ms`] of
//! sound in the BGM is matched against the envelope of the capture (coarse, cheap, robust to mixing), then the
//! waveform of the first [`DetectorOptions::wave_ms`] is matched at full rate within ±[`DetectorOptions::refine_ms`]
//! of the coarse hit (sample-exact). [`Detector`] works incrementally on a live stream (the injector waits for it,
//! doc 07 §3.2) and on a whole recording (offline analysis); both report the same [`Match`].

use std::ops::Range;

/// Mono mix of interleaved frames.
pub fn mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    interleaved
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Mean absolute value of each complete `block` of samples.
pub fn envelope(x: &[f32], block: usize) -> Vec<f32> {
    x.chunks_exact(block.max(1))
        .map(|b| b.iter().map(|s| s.abs()).sum::<f32>() / b.len() as f32)
        .collect()
}

/// Index of the first sample whose magnitude exceeds `level`.
pub fn first_sound(x: &[f32], level: f32) -> Option<usize> {
    x.iter().position(|s| s.abs() > level)
}

/// Best Pearson correlation of `tpl` against `sig[lag..lag + tpl.len()]` over `lags` (lags whose window does not
/// fit in `sig` are skipped). Returns `(lag, r)`; `None` if no lag fits or the template is constant.
pub fn best_lag(sig: &[f32], tpl: &[f32], lags: Range<usize>) -> Option<(usize, f64)> {
    let n = tpl.len();
    let last = (sig.len() + 1).checked_sub(n)?.min(lags.end);
    if n == 0 || lags.start >= last {
        return None;
    }
    let mean_t = tpl.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let tc: Vec<f64> = tpl.iter().map(|&v| f64::from(v) - mean_t).collect();
    let norm_t = tc.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm_t == 0.0 {
        return None;
    }
    // Prefix sums of the signal and its square over the searched span, for each window's mean and norm.
    let span = &sig[lags.start..last - 1 + n];
    let mut s1 = Vec::with_capacity(span.len() + 1);
    let mut s2 = Vec::with_capacity(span.len() + 1);
    let (mut a, mut b) = (0.0f64, 0.0f64);
    s1.push(0.0);
    s2.push(0.0);
    for &v in span {
        a += f64::from(v);
        b += f64::from(v) * f64::from(v);
        s1.push(a);
        s2.push(b);
    }
    let mut best: Option<(usize, f64)> = None;
    for k in 0..last - lags.start {
        let w = &span[k..k + n];
        let sum = s1[k + n] - s1[k];
        let var = (s2[k + n] - s2[k]) - sum * sum / n as f64;
        if var <= 0.0 {
            continue;
        }
        let dot: f64 = w.iter().zip(&tc).map(|(&s, &t)| f64::from(s) * t).sum();
        let r = dot / (var.sqrt() * norm_t);
        if best.is_none_or(|(_, br)| r > br) {
            best = Some((lags.start + k, r));
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorOptions {
    pub rate: u32,
    /// Envelope block length in samples (48 = 1 ms at 48 kHz).
    pub block: usize,
    /// Length of the coarse envelope template, from the BGM's first sound.
    pub envelope_ms: f64,
    /// Length of the fine waveform template.
    pub wave_ms: f64,
    /// Fine search radius around the coarse hit.
    pub refine_ms: f64,
    /// Coarse correlation needed to accept a match.
    pub threshold: f64,
    /// Fine (waveform) correlation a confirmed coarse hit must also reach; otherwise scanning goes on.
    pub fine_threshold: f64,
    /// A coarse hit is final once this much more capture has been scanned without a better one.
    pub confirm_ms: f64,
    /// BGM samples quieter than this count as silence when looking for its first sound.
    pub silence: f32,
}

impl Default for DetectorOptions {
    fn default() -> Self {
        Self {
            rate: 48_000,
            block: 48,
            envelope_ms: 1500.0,
            wave_ms: 250.0,
            refine_ms: 6.0,
            threshold: 0.6,
            fine_threshold: 0.5,
            confirm_ms: 200.0,
            silence: 1e-3,
        }
    }
}

/// Where BGM sample 0 lies in the capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    /// Capture sample index of BGM sample 0 (negative if the BGM started before the capture).
    pub bgm0: i64,
    /// Coarse (envelope) and fine (waveform) correlation at the match, the confidence measures.
    pub coarse: f64,
    pub fine: f64,
}

#[derive(Debug)]
pub struct Detector {
    opts: DetectorOptions,
    tpl_env: Vec<f32>,
    tpl_wave: Vec<f32>,
    /// BGM sample index where both templates start (block aligned).
    tpl_start: usize,
    raw: Vec<f32>,
    env: Vec<f32>,
    scanned: usize,
    /// Best coarse hit at or above the threshold, and the best overall (envelope lag, r).
    hit: Option<(usize, f64)>,
    peak: Option<(usize, f64)>,
}

impl Detector {
    /// `None` if the BGM is silent or shorter than the templates.
    pub fn new(bgm: &[f32], opts: DetectorOptions) -> Option<Self> {
        let ms = |v: f64| (v * f64::from(opts.rate) / 1000.0).round() as usize;
        let start = first_sound(bgm, opts.silence)? / opts.block * opts.block;
        let env_len = ms(opts.envelope_ms) / opts.block;
        let tpl_env = envelope(bgm.get(start..start + env_len * opts.block)?, opts.block);
        let tpl_wave = bgm.get(start..start + ms(opts.wave_ms))?.to_vec();
        Some(Self {
            opts,
            tpl_env,
            tpl_wave,
            tpl_start: start,
            raw: Vec::new(),
            env: Vec::new(),
            scanned: 0,
            hit: None,
            peak: None,
        })
    }

    /// Mono capture samples received so far.
    pub fn captured(&self) -> &[f32] {
        &self.raw
    }

    /// Appends capture samples; returns the match once a coarse hit is confirmed and refined.
    pub fn push(&mut self, samples: &[f32]) -> Option<Match> {
        let b = self.opts.block;
        let done = self.env.len() * b;
        self.raw.extend_from_slice(samples);
        self.env
            .extend(envelope(&self.raw[done..self.raw.len() / b * b], b));
        let n = self.tpl_env.len();
        if let Some(end) = (self.env.len() + 1)
            .checked_sub(n)
            .filter(|&e| e > self.scanned)
        {
            if let Some((lag, r)) = best_lag(&self.env, &self.tpl_env, self.scanned..end) {
                if self.peak.is_none_or(|(_, p)| r > p) {
                    self.peak = Some((lag, r));
                }
                if r >= self.opts.threshold && self.hit.is_none_or(|(_, h)| r > h) {
                    self.hit = Some((lag, r));
                }
            }
            self.scanned = end;
        }
        let hit = self.hit?;
        let confirm = (self.opts.confirm_ms * f64::from(self.opts.rate) / 1000.0) as usize / b;
        if self.scanned < hit.0 + confirm {
            return None;
        }
        let m = self.refine(hit)?;
        if m.fine < self.opts.fine_threshold {
            log::debug!("rejecting coarse hit {m:?}");
            self.hit = None;
            return None;
        }
        Some(m)
    }

    /// The best coarse position seen so far, refined, whether or not it passed the threshold.
    pub fn best(&self) -> Option<Match> {
        self.refine(self.peak?)
    }

    fn refine(&self, (lag, coarse): (usize, f64)) -> Option<Match> {
        let radius = (self.opts.refine_ms * f64::from(self.opts.rate) / 1000.0) as usize;
        let center = lag * self.opts.block;
        let lags = center.saturating_sub(radius)..center + radius + 1;
        let (at, fine) = best_lag(&self.raw, &self.tpl_wave, lags)?;
        Some(Match {
            bgm0: at as i64 - self.tpl_start as i64,
            coarse,
            fine,
        })
    }
}

/// Residual (ms) of the alignment at a later point of the song: correlates `bgm[bgm_at..+len]` within
/// ±`radius` samples of where `m` predicts it in `capture`. `None` if out of range. Returns (residual_ms, r).
pub fn check_at(
    capture: &[f32],
    bgm: &[f32],
    m: &Match,
    bgm_at: usize,
    len: usize,
    radius: usize,
    rate: u32,
) -> Option<(f64, f64)> {
    let tpl = bgm.get(bgm_at..bgm_at + len)?;
    let predicted = usize::try_from(m.bgm0 + bgm_at as i64).ok()?;
    let lags = predicted.saturating_sub(radius)..predicted + radius + 1;
    let (at, r) = best_lag(capture, tpl, lags)?;
    Some(((at as f64 - predicted as f64) * 1000.0 / f64::from(rate), r))
}

/// Decodes a whole BGM cue from `<assets>/Sound/Bgm` (the stream our player plays) to mono; returns it and its rate.
pub fn load_bgm(
    assets: &std::path::Path,
    cue: &str,
) -> Result<(Vec<f32>, u32), wacca_audio::Error> {
    use wacca_audio::Decoder;
    let bank = wacca_audio::BgmBank::open(assets.join("Sound").join("Bgm"))?;
    let mut dec = bank.open_cue(cue)?;
    let mut pcm = Vec::new();
    while dec.decode(&mut pcm)? {}
    Ok((mono(&pcm, usize::from(dec.channels())), dec.sample_rate()))
}

/// One capture packet: index of its first frame and that frame's capture time (ms on the run's epoch).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub frame: u64,
    pub ms: f64,
}

/// Epoch ms of capture frame `idx` (fractional allowed), from the last packet at or before it (or the first).
pub fn frame_ms(packets: &[Packet], rate: u32, idx: f64) -> Option<f64> {
    let i = packets.partition_point(|p| p.frame as f64 <= idx);
    let p = packets.get(i.saturating_sub(1))?;
    Some(p.ms + (idx - p.frame as f64) * 1000.0 / f64::from(rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise-like "music" with a loud/quiet pattern so the envelope has structure.
    fn music(len: usize, seed: u32) -> Vec<f32> {
        let lcg = |s: u32| s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut s = seed;
        let mut gain = 1.0;
        (0..len)
            .map(|i| {
                s = lcg(s);
                if i % 480 == 0 {
                    gain = 0.1 + (s >> 24) as f32 / 256.0;
                    s = lcg(s);
                }
                let noise = (s >> 8) as f32 / (1 << 24) as f32 - 0.5;
                noise * gain
            })
            .collect()
    }

    fn opts() -> DetectorOptions {
        DetectorOptions {
            envelope_ms: 300.0,
            wave_ms: 50.0,
            confirm_ms: 50.0,
            ..DetectorOptions::default()
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(mono(&[1.0, 3.0, -2.0, 0.0], 2), [2.0, -1.0]);
        assert_eq!(envelope(&[1.0, -1.0, 2.0, 0.0, 9.0], 2), [1.0, 1.0]);
        assert_eq!(first_sound(&[0.0, 0.001, 0.5], 0.01), Some(2));
        let sig = [0.0, 0.0, 1.0, 2.0, 1.0, 0.0];
        let (lag, r) = best_lag(&sig, &[1.0, 2.0, 1.0], 0..10).unwrap();
        assert_eq!(lag, 2);
        assert!((r - 1.0).abs() < 1e-9);
        // Constant windows (lag 0) are skipped; lags beyond the range or the signal are not searched.
        assert_eq!(best_lag(&sig, &[0.0, 1.0], 0..2).map(|b| b.0), Some(1));
        assert!(best_lag(&sig, &[1.0; 7], 0..3).is_none());
        assert!(best_lag(&sig, &[1.0, 1.0], 0..3).is_none());
    }

    #[test]
    fn finds_bgm_start_streaming_with_gain_noise_and_leading_silence() {
        let rate = 48_000;
        // BGM: 0.1 s silence then music.
        let mut bgm = vec![0.0; 4800];
        bgm.extend(music(48_000, 7));
        // Capture: 0.7 s of other sound (different music), then the BGM at half gain plus a little noise.
        let offset = 33_333usize;
        let mut cap = music(offset, 99)
            .iter()
            .map(|v| v * 0.2)
            .collect::<Vec<_>>();
        let noise = music(bgm.len(), 5);
        cap.extend(bgm.iter().zip(&noise).map(|(b, n)| 0.5 * b + 0.02 * n));
        let mut d = Detector::new(&bgm, opts()).unwrap();
        let mut found = None;
        for chunk in cap.chunks(480) {
            let m = d.push(chunk);
            if found.is_none() {
                found = m;
                // Confirmed well before the end of the capture: live use can act on it.
                assert!(m.is_none() || d.captured().len() < offset + 4800 + 24_000);
            }
        }
        let m = found.expect("match");
        assert_eq!(m.bgm0, offset as i64);
        assert!(m.coarse > 0.9 && m.fine > 0.9, "{m:?}");
        assert_eq!(d.best().unwrap().bgm0, offset as i64);
        let (res, r) = check_at(d.captured(), &bgm, &m, 30_000, 2400, 480, rate).unwrap();
        assert_eq!(res, 0.0);
        assert!(r > 0.9);
    }

    #[test]
    fn unrelated_audio_does_not_match() {
        let bgm = music(48_000, 7);
        let mut d = Detector::new(&bgm, opts()).unwrap();
        assert!(d.push(&music(96_000, 1234)).is_none());
        assert!(d.best().unwrap().coarse < 0.6);
        assert!(Detector::new(&[0.0; 48_000], opts()).is_none());
        // Same envelope, different waveform (random sign flips): the coarse stage matches, the fine one rejects.
        let flips = music(bgm.len(), 3);
        let fake: Vec<f32> = bgm
            .iter()
            .zip(&flips)
            .map(|(b, f)| if *f > 0.0 { *b } else { -*b })
            .collect();
        let mut d = Detector::new(&bgm, opts()).unwrap();
        assert!(d.push(&fake).is_none());
        assert!(d.best().unwrap().coarse > 0.9);
    }

    #[test]
    fn packet_times() {
        let p = [
            Packet {
                frame: 0,
                ms: 100.0,
            },
            Packet {
                frame: 480,
                ms: 110.5,
            },
        ];
        assert_eq!(frame_ms(&p, 48_000, 240.0), Some(105.0));
        assert_eq!(frame_ms(&p, 48_000, 960.0), Some(120.5));
        assert_eq!(frame_ms(&p, 48_000, -48.0), Some(99.0));
        assert_eq!(frame_ms(&[], 48_000, 0.0), None);
    }
}
