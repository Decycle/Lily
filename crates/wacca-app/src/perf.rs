//! Frame and latency instrumentation: the F3 overlay and the `wacca_app::perf` debug log.
//!
//! Pure bookkeeping: the shell measures durations with `Instant` and feeds them in; nothing here
//! touches a clock, so every number is unit-testable. Storage is fixed (one ring per stage), so
//! recording costs no allocation per frame.

use wacca_render::layout::Rect;
use wacca_render::text::{Align, Face};

use crate::ui::{Item, View};

/// Frames kept per statistic (2 s at 120 Hz).
const WINDOW: usize = 240;

/// The last [`WINDOW`] samples of one duration, in milliseconds.
#[derive(Clone, Debug)]
pub struct Stat {
    samples: [f32; WINDOW],
    len: usize,
    next: usize,
}

impl Default for Stat {
    fn default() -> Self {
        Self {
            samples: [0.0; WINDOW],
            len: 0,
            next: 0,
        }
    }
}

impl Stat {
    pub fn push(&mut self, ms: f32) {
        self.samples[self.next] = ms;
        self.next = (self.next + 1) % WINDOW;
        self.len = (self.len + 1).min(WINDOW);
    }

    fn values(&self) -> &[f32] {
        &self.samples[..self.len]
    }

    pub fn mean(&self) -> f32 {
        let v = self.values();
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    }

    pub fn max(&self) -> f32 {
        self.values().iter().copied().fold(0.0, f32::max)
    }

    /// `p`-th percentile (0..=1, nearest rank).
    pub fn percentile(&self, p: f32) -> f32 {
        let mut v = self.samples;
        let v = &mut v[..self.len];
        if v.is_empty() {
            return 0.0;
        }
        v.sort_unstable_by(f32::total_cmp);
        let rank = ((p.clamp(0.0, 1.0) * v.len() as f32).ceil() as usize).clamp(1, v.len());
        v[rank - 1]
    }

    /// `mean / p99 / max` in ms.
    fn summary(&self) -> String {
        format!(
            "{:5.2} /{:5.2} /{:5.2}",
            self.mean(),
            self.percentile(0.99),
            self.max()
        )
    }
}

/// Refresh intervals missed by a frame that took `interval` seconds at `period` seconds per
/// refresh: 0 for an on-time frame, 1 when one refresh showed the previous frame again, and so
/// on. Half a period of slack absorbs timing jitter.
pub fn missed_refreshes(interval: f64, period: f64) -> u64 {
    if period <= 0.0 || interval < 1.5 * period {
        0
    } else {
        (interval / period).round() as u64 - 1
    }
}

/// Audio clock vs wall clock while the song plays: how far the song time has run ahead (+) or
/// behind (−) the wall time since the first sample. Restarted on seeks and pauses.
#[derive(Clone, Copy, Debug, Default)]
pub struct Drift {
    origin: Option<(f64, f64)>,
    last: Option<(f64, f64)>,
}

impl Drift {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Records wall time `wall` and song time `audio` (seconds); returns (drift since the start,
    /// this frame's step difference `Δaudio − Δwall`), both in ms.
    pub fn sample(&mut self, wall: f64, audio: f64) -> (f32, f32) {
        let (w0, a0) = *self.origin.get_or_insert((wall, audio));
        let step = self
            .last
            .replace((wall, audio))
            .map_or(0.0, |(w, a)| (audio - a) - (wall - w));
        (
            (((audio - a0) - (wall - w0)) * 1000.0) as f32,
            (step * 1000.0) as f32,
        )
    }
}

/// Everything the overlay shows.
#[derive(Clone, Debug, Default)]
pub struct Perf {
    /// CPU time to build the scene and the overlay view.
    pub build: Stat,
    /// Waiting for the next swap-chain image (`get_current_texture`): with vsync this is where
    /// the loop blocks, and input queues up meanwhile.
    pub acquire: Stat,
    /// Encoding both passes and `queue.submit`.
    pub submit: Stat,
    /// `present()`.
    pub present: Stat,
    /// Start of one frame to the start of the next.
    pub interval: Stat,
    /// Input event handled → the next frame presented (judgement visible from then on, plus
    /// the swap chain's queue).
    pub input: Stat,
    /// Per-frame audio-vs-wall step difference (clock jitter).
    pub jitter: Stat,
    /// Latest audio-vs-wall drift (ms).
    pub drift: f32,
    pub frames: u64,
    pub missed: u64,
    /// Images (jackets, UI textures) uploaded, and the UI-thread time spent on them.
    pub image_loads: u64,
    pub image_ms: f32,
    /// Seconds per display refresh (0 = unknown).
    pub period: f64,
    /// Present mode and frame latency as configured.
    pub mode: String,
    /// Audio device callback size (frames) and reported output latency (ms).
    pub audio: Option<(u64, f32)>,
}

impl Perf {
    /// One frame started `interval` seconds after the previous one.
    pub fn frame(&mut self, interval: f64) {
        self.frames += 1;
        self.interval.push((interval * 1000.0) as f32);
        self.missed += missed_refreshes(interval, self.period);
    }

    pub fn lines(&self) -> Vec<String> {
        let hz = if self.period > 0.0 {
            1.0 / self.period
        } else {
            0.0
        };
        let fps = match self.interval.mean() {
            m if m > 0.0 => 1000.0 / m,
            _ => 0.0,
        };
        let mut lines = vec![
            format!("{fps:5.1} fps @ {hz:.0} Hz  {}", self.mode),
            "ms          mean /  p99 /  max".to_string(),
            format!("frame    {}", self.interval.summary()),
            format!("build    {}", self.build.summary()),
            format!("acquire  {}", self.acquire.summary()),
            format!("submit   {}", self.submit.summary()),
            format!("present  {}", self.present.summary()),
            format!("input>pr {}", self.input.summary()),
            format!("missed {} of {} frames", self.missed, self.frames),
            format!(
                "audio drift {:+6.2} ms, jitter p99 {:5.2}",
                self.drift,
                self.jitter.percentile(0.99)
            ),
            format!(
                "images {} uploaded, {:.0} ms on UI thread",
                self.image_loads, self.image_ms
            ),
        ];
        if let Some((frames, ms)) = self.audio {
            lines.push(format!("audio out {frames} frames/cb, latency {ms:.1} ms"));
        }
        lines
    }

    /// The overlay's lines at debug level (`RUST_LOG=wacca_app::perf=debug`).
    pub fn log(&self) {
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("\n{}", self.lines().join("\n"));
        }
    }

    /// The overlay in the top-left corner of `window`.
    pub fn view(&self, window: Rect) -> View {
        let px = (window.h / 60.0).clamp(12.0, 22.0).round();
        let lines = self.lines();
        let mut items = vec![Item::Fill {
            rect: Rect {
                x: window.x,
                y: window.y,
                w: px * 17.0,
                h: px * 1.2 * lines.len() as f32 + px * 0.4,
            },
            color: [0.0, 0.0, 0.0, 0.7],
        }];
        items.extend(lines.into_iter().enumerate().map(|(i, text)| Item::Text {
            text,
            at: [
                window.x + px * 0.3,
                window.y + px * 0.2 + px * 1.2 * i as f32,
            ],
            px,
            color: [0.6, 1.0, 0.6, 1.0],
            align: Align::Left,
            max_width: None,
            face: Face::Ui,
        }));
        View {
            items,
            ..View::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_keeps_a_window_and_ranks() {
        let mut s = Stat::default();
        assert_eq!((s.mean(), s.max(), s.percentile(0.99)), (0.0, 0.0, 0.0));
        for i in 1..=100 {
            s.push(i as f32);
        }
        assert_eq!(s.mean(), 50.5);
        assert_eq!(s.max(), 100.0);
        assert_eq!(s.percentile(0.99), 99.0);
        assert_eq!(s.percentile(0.5), 50.0);
        // Older samples fall out of the window.
        for _ in 0..WINDOW {
            s.push(1.0);
        }
        assert_eq!((s.mean(), s.max()), (1.0, 1.0));
    }

    #[test]
    fn missed_refreshes_count_repeated_images() {
        let p = 1.0 / 120.0;
        assert_eq!(missed_refreshes(p, p), 0);
        assert_eq!(missed_refreshes(p * 1.4, p), 0);
        assert_eq!(missed_refreshes(p * 2.0, p), 1);
        assert_eq!(missed_refreshes(p * 4.1, p), 3);
        assert_eq!(missed_refreshes(1.0, 0.0), 0);
    }

    #[test]
    fn drift_measures_audio_against_wall_time() {
        let mut d = Drift::default();
        assert_eq!(d.sample(10.0, 3.0), (0.0, 0.0));
        let (drift, step) = d.sample(10.1, 3.102);
        assert!((drift - 2.0).abs() < 1e-3 && (step - 2.0).abs() < 1e-3);
        let (drift, step) = d.sample(10.2, 3.201);
        assert!((drift - 1.0).abs() < 1e-3 && (step + 1.0).abs() < 1e-3);
        d.reset();
        assert_eq!(d.sample(50.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn perf_counts_frames_and_lists_lines() {
        let mut p = Perf {
            period: 1.0 / 120.0,
            ..Perf::default()
        };
        p.frame(1.0 / 120.0);
        p.frame(3.0 / 120.0);
        assert_eq!((p.frames, p.missed), (2, 2));
        let view = p.view(Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1200.0,
        });
        // Background plus one text item per line.
        assert_eq!(view.items.len(), p.lines().len() + 1);
        assert!(p.lines()[0].contains("120 Hz"));
    }
}
