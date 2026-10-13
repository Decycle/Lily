//! Timing check: a steady click on the audio clock; the player taps along (touch anywhere or
//! Space) and the median tap error, outliers dropped, becomes a suggested judge offset in the
//! options' unit ([`options::offset_units`]).
//!
//! The clicks are rendered into a BGM source ([`Metronome`]) instead of being fired as sound
//! effects: an effect starts at the next device callback (up to a buffer of jitter), while a
//! click in the BGM sits on an exact sample of the clock the judgement reads
//! ([`wacca_audio::Player::position`]). Taps are read off that same clock, as in play, so the
//! measured error is what the judgement would see: output latency the clock does not know about
//! plus touch / key latency plus the player's own habit.

use wacca_audio::{Decoder, Error, Sfx};
use wacca_game::save::Settings;
use wacca_render::layout::Rect;
use wacca_render::text::Align;

use crate::circle::{self, Canvas, NAVY, PINK, slab};
use crate::options;
use crate::ui::{Action, GREY, View, WHITE};

pub const BPM: f64 = 120.0;
/// Seconds between clicks.
pub const PERIOD: f64 = 60.0 / BPM;
/// Silence before the first click (s of audio clock).
pub const LEAD_IN: f64 = 1.0;
/// Taps needed before there is a suggestion.
pub const MIN_TAPS: usize = 16;
/// Only the most recent taps count, so early warm-up taps age out.
const KEEP_TAPS: usize = 32;
/// Taps further than this many (normal-scaled) MADs from the median are dropped.
const OUTLIER_MADS: f64 = 3.0;
/// MAD floor (s): very steady tapping must not reject a tap a few ms off.
const MIN_MAD: f64 = 0.005;
/// How long the ring flash of a click lasts (s).
const FLASH_SECS: f64 = 0.15;
/// Length of the click track; far longer than anyone taps.
const TRACK_SECS: u64 = 600;
/// Frames per decoded block.
const BLOCK: u64 = 1024;

/// Signed error of a tap at audio position `pos` against the nearest click (s, + = late);
/// `None` before the clicks start.
pub fn tap_error(pos: f64) -> Option<f64> {
    if !pos.is_finite() || pos < LEAD_IN - PERIOD / 2.0 {
        return None;
    }
    let k = ((pos - LEAD_IN) / PERIOD).round().max(0.0);
    Some(pos - (LEAD_IN + k * PERIOD))
}

/// Brightness (1 → 0) of the flash marking each click, at audio position `pos`.
pub fn flash(pos: f64) -> f32 {
    if pos < LEAD_IN {
        return 0.0;
    }
    let since = (pos - LEAD_IN).rem_euclid(PERIOD);
    (1.0 - since / FLASH_SECS).max(0.0) as f32
}

/// Median of a sorted, non-empty slice.
fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

/// Typical tap error (s, + = late): the median of the taps within [`OUTLIER_MADS`] scaled
/// median absolute deviations of the median. `None` below [`MIN_TAPS`] taps.
pub fn robust_error(errors: &[f64]) -> Option<f64> {
    if errors.len() < MIN_TAPS {
        return None;
    }
    let mut v = errors.to_vec();
    v.sort_by(f64::total_cmp);
    let m = median(&v);
    let mut dev: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    dev.sort_by(f64::total_cmp);
    // 1.4826 × MAD estimates the standard deviation of normally distributed taps.
    let limit = OUTLIER_MADS * (1.4826 * median(&dev)).max(MIN_MAD);
    let kept: Vec<f64> = v.into_iter().filter(|x| (x - m).abs() <= limit).collect();
    Some(if kept.is_empty() { m } else { median(&kept) })
}

/// The judge offset setting that cancels a typical tap error `err` (s, + = late). Judged time is
/// audio time + offset, so taps that land `err` late need an offset of `-err`.
pub fn suggested_offset(err: f64) -> f32 {
    // `+ 0.0` turns -0.0 into 0.0 so it prints as "+0.0".
    options::offset_units(-err) + 0.0
}

/// "12 MS LATE" / "8 MS EARLY" / "ON TIME".
pub fn describe(err: f64) -> String {
    let ms = (err * 1000.0).round() as i64;
    match ms {
        0 => "ON TIME".into(),
        ms if ms > 0 => format!("{ms} MS LATE"),
        ms => format!("{} MS EARLY", -ms),
    }
}

/// Taps of one timing check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calibration {
    /// Errors of the most recent taps (s, + = late), oldest first.
    errors: Vec<f64>,
}

impl Calibration {
    /// Record a tap at audio position `pos`; returns its error, `None` if it was too early.
    pub fn tap(&mut self, pos: f64) -> Option<f64> {
        let e = tap_error(pos)?;
        if self.errors.len() == KEEP_TAPS {
            self.errors.remove(0);
        }
        self.errors.push(e);
        Some(e)
    }

    pub fn errors(&self) -> &[f64] {
        &self.errors
    }

    pub fn error(&self) -> Option<f64> {
        robust_error(&self.errors)
    }

    pub fn suggestion(&self) -> Option<f32> {
        self.error().map(suggested_offset)
    }
}

/// The click track as a BGM source: `click` at `LEAD_IN + k × PERIOD`, silence elsewhere.
#[derive(Debug)]
pub struct Metronome {
    click: Sfx,
    frame: u64,
}

impl Metronome {
    /// `click` is cut to start at its attack (leading silence would read as late taps).
    pub fn new(click: &Sfx) -> Self {
        Self {
            click: trim_onset(click),
            frame: 0,
        }
    }
}

/// `sfx` from its first sample above 10 % of its peak, without a loop region.
fn trim_onset(sfx: &Sfx) -> Sfx {
    let ch = usize::from(sfx.channels.max(1));
    let peak = sfx.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let first = sfx
        .samples
        .iter()
        .position(|s| s.abs() >= peak * 0.1)
        .map_or(0, |i| i / ch * ch);
    Sfx {
        samples: sfx.samples[first..].into(),
        loop_range: None,
        ..sfx.clone()
    }
}

/// A 25 ms 1.5 kHz blip at `rate`, for when the hit sounds are not available.
pub fn beep(rate: u32) -> Sfx {
    let n = rate as usize / 40;
    let samples = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            0.5 * (1.0 - i as f32 / n as f32) * (std::f32::consts::TAU * 1500.0 * t).sin()
        })
        .collect();
    Sfx {
        rate,
        channels: 1,
        samples,
        loop_range: None,
    }
}

/// Append frames `start..start + n` of the click track to `out`. A click longer than the period
/// is cut at the next click.
fn render(click: &Sfx, start: u64, n: u64, out: &mut Vec<f32>) {
    let ch = usize::from(click.channels.max(1));
    let rate = f64::from(click.rate);
    let lead = (LEAD_IN * rate).round() as u64;
    let period = ((PERIOD * rate).round() as u64).max(1);
    for f in start..start + n {
        let i = f
            .checked_sub(lead)
            .map(|d| (d % period) as usize * ch)
            .filter(|&i| i < click.samples.len());
        match i {
            Some(i) => out.extend_from_slice(&click.samples[i..i + ch]),
            None => out.extend(std::iter::repeat_n(0.0, ch)),
        }
    }
}

impl Decoder for Metronome {
    fn channels(&self) -> u16 {
        self.click.channels.max(1)
    }

    fn sample_rate(&self) -> u32 {
        self.click.rate
    }

    fn total_frames(&self) -> u64 {
        TRACK_SECS * u64::from(self.click.rate)
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
        let n = BLOCK.min(self.total_frames().saturating_sub(self.frame));
        if n == 0 {
            return Ok(false);
        }
        render(&self.click, self.frame, n, out);
        self.frame += n;
        Ok(true)
    }

    fn seek(&mut self, frame: u64) -> Result<(), Error> {
        self.frame = frame.min(self.total_frames());
        Ok(())
    }
}

/// Error lines shown, newest first.
const SHOWN: usize = 5;

/// The timing check in the options' style: header slab, a ring flashing on each click, the
/// latest tap errors, the suggestion against the current setting, BACK and APPLY.
pub fn view(cal: &Calibration, s: &Settings, pos: f64, area: Rect) -> View {
    let mut v = View::default();
    let c = Canvas::new(area);
    circle::background(&mut v, &c);
    c.panel(&mut v, &slab(350.0, 560.0, 420.0, 56.0), &[NAVY]);
    c.text(
        &mut v,
        "TIMING CHECK",
        [544.0, 566.0],
        40.0,
        WHITE,
        Align::Center,
    );
    c.panel(
        &mut v,
        &slab(320.0, 622.0, 420.0, 34.0),
        &[[0.0, 0.0, 0.0, 0.4]],
    );
    c.text_fit(
        &mut v,
        "TAP ON EVERY CLICK",
        [520.0, 626.0],
        24.0,
        PINK,
        380.0,
    );
    // Ring: a dim disc that lights up pink on each click (display vs audio check).
    let lit = flash(pos);
    let ring = |r: f32| -> Vec<[f32; 2]> {
        (0..48)
            .map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                [540.0 + r * a.cos(), 780.0 + r * a.sin()]
            })
            .collect()
    };
    c.panel(&mut v, &ring(80.0), &[[1.0, 1.0, 1.0, 0.15 + 0.85 * lit]]);
    c.panel(&mut v, &ring(60.0), &[NAVY]);
    let mut lit_pink = PINK;
    lit_pink[3] = lit;
    c.panel(&mut v, &ring(60.0), &[lit_pink]);

    let n = cal.errors().len();
    c.text(
        &mut v,
        &format!("TAPS {}/{MIN_TAPS}", n.min(MIN_TAPS)),
        [540.0, 880.0],
        30.0,
        WHITE,
        Align::Center,
    );
    for (i, &e) in cal.errors().iter().rev().take(SHOWN).enumerate() {
        let color = if i == 0 { WHITE } else { GREY };
        let y = 930.0 + 40.0 * i as f32;
        c.text(&mut v, &describe(e), [540.0, y], 28.0, color, Align::Center);
    }
    let now = options::value(options::Row::Offset, s, false);
    let (line, avail) = match (cal.error(), cal.suggestion()) {
        (Some(e), Some(o)) => (
            format!("{}  ->  TIMING {o:+.1} (NOW {now})", describe(e)),
            true,
        ),
        _ => (format!("KEEP TAPPING  (TIMING NOW {now})"), false),
    };
    c.panel(&mut v, &slab(250.0, 1150.0, 600.0, 50.0), &[NAVY]);
    c.text_fit(&mut v, &line, [535.0, 1158.0], 26.0, PINK, 540.0);

    circle::side_tab(
        &mut v,
        &c,
        "BACK",
        300.0,
        [0.55, 0.20, 0.80, 0.95],
        Action::Back,
    );
    let apply = if avail {
        [1.0, 0.25, 0.75, 1.0]
    } else {
        [0.4, 0.4, 0.45, 0.8]
    };
    circle::side_tab(&mut v, &c, "APPLY", 650.0, apply, Action::Confirm);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 900.0,
        h: 1600.0,
    };

    #[test]
    fn tap_errors_are_against_the_nearest_click() {
        assert_eq!(tap_error(0.2), None);
        assert!((tap_error(LEAD_IN + 0.02).unwrap() - 0.02).abs() < 1e-9);
        assert!((tap_error(LEAD_IN + 3.0 * PERIOD - 0.03).unwrap() + 0.03).abs() < 1e-9);
        // Just before the first click counts as early.
        assert!((tap_error(LEAD_IN - 0.1).unwrap() + 0.1).abs() < 1e-9);
        assert_eq!(tap_error(f64::NAN), None);
    }

    #[test]
    fn flash_fades_after_each_click() {
        assert_eq!(flash(0.5), 0.0);
        assert_eq!(flash(LEAD_IN), 1.0);
        assert!((flash(LEAD_IN + PERIOD + FLASH_SECS / 2.0) - 0.5).abs() < 1e-4);
        assert_eq!(flash(LEAD_IN + FLASH_SECS + 0.01), 0.0);
    }

    #[test]
    fn robust_error_needs_enough_taps_and_drops_outliers() {
        let steady: Vec<f64> = (0..15).map(|i| 0.030 + 0.001 * (i % 5) as f64).collect();
        assert_eq!(robust_error(&steady), None);
        let mut taps = steady.clone();
        taps.push(0.032);
        let m = robust_error(&taps).unwrap();
        assert!((m - 0.032).abs() < 1e-9, "{m}");
        // Wild taps (missed beats, double taps) on one side do not drag the result.
        taps.extend([0.24, 0.23, -0.2]);
        let m = robust_error(&taps).unwrap();
        assert!((m - 0.032).abs() < 1e-9, "{m}");
        // Identical taps: zero MAD still keeps them (and near neighbours).
        let same = [0.01; 20];
        assert_eq!(robust_error(&same), Some(0.01));
    }

    #[test]
    fn suggestion_cancels_the_error_in_option_units() {
        // 25 ms late → judgement clock 25 ms behind = −3.0 units of 8.33 ms.
        assert_eq!(suggested_offset(0.025), -3.0);
        assert_eq!(suggested_offset(-0.0125), 1.5);
        assert_eq!(suggested_offset(0.0), 0.0);
        assert!(suggested_offset(0.0).is_sign_positive());
        assert_eq!(suggested_offset(1.0), -10.0);
        assert_eq!(describe(0.0123), "12 MS LATE");
        assert_eq!(describe(-0.008), "8 MS EARLY");
        assert_eq!(describe(0.0004), "ON TIME");
    }

    #[test]
    fn calibration_keeps_recent_taps() {
        let mut c = Calibration::default();
        assert_eq!(c.tap(0.1), None);
        for k in 0..40 {
            c.tap(LEAD_IN + k as f64 * PERIOD + if k < 8 { -0.1 } else { 0.02 });
        }
        assert_eq!(c.errors().len(), KEEP_TAPS);
        assert!((c.error().unwrap() - 0.02).abs() < 1e-9);
        assert_eq!(c.suggestion(), Some(-2.4));
    }

    #[test]
    fn metronome_places_clicks_on_exact_frames() {
        let click = Sfx {
            rate: 1000,
            channels: 2,
            samples: vec![0.0, 0.0, 0.01, 0.01, 1.0, -1.0, 0.5, 0.5].into(),
            loop_range: Some((0, 1)),
        };
        let mut m = Metronome::new(&click);
        assert_eq!((m.channels(), m.sample_rate()), (2, 1000));
        let mut out = Vec::new();
        while out.len() < 2 * 1600 && m.decode(&mut out).unwrap() {}
        // Leading quiet frames trimmed: the attack lands on frame 1000, 1500, ...
        let frame = |f: usize| [out[2 * f], out[2 * f + 1]];
        assert_eq!(frame(999), [0.0, 0.0]);
        assert_eq!(frame(1000), [1.0, -1.0]);
        assert_eq!(frame(1001), [0.5, 0.5]);
        assert_eq!(frame(1002), [0.0, 0.0]);
        assert_eq!(frame(1500), [1.0, -1.0]);
        m.seek(u64::MAX).unwrap();
        assert!(!m.decode(&mut out).unwrap());
        assert_eq!(beep(48_000).frames(), 1200);
    }

    #[test]
    fn view_shows_progress_suggestion_and_buttons() {
        let mut cal = Calibration::default();
        let s = Settings::default();
        let t = view(&cal, &s, 0.0, AREA).texts().concat();
        assert!(t.contains("TAPS 0/16") && t.contains("KEEP TAPPING"), "{t}");
        for k in 0..16 {
            cal.tap(LEAD_IN + k as f64 * PERIOD + 0.025);
        }
        let v = view(&cal, &s, LEAD_IN, AREA);
        let t = v.texts().concat();
        assert!(
            t.contains("25 MS LATE") && t.contains("TIMING -3.0 (NOW +0.0)"),
            "{t}"
        );
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Confirm));
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Back));
        // The ring area is free to tap.
        let centre = crate::ui::canvas(AREA, [540.0, 780.0]);
        assert_eq!(v.hit(centre), None);
    }
}
