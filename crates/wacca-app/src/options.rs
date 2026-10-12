//! Options screen: note speed, judgement offset, mirror, autoplay and background video, stored in the save's
//! [`Settings`] (autoplay is per session and never saved).
//!
//! This module is the one place that knows the judge-offset unit: everything else converts with
//! [`offset_secs`] / [`offset_units`].

use wacca_game::save::Settings;
use wacca_render::layout::Rect;
use wacca_render::text::Align;

use crate::select::buttons;
use crate::ui::{Action, GREY, HIGHLIGHT, PANEL, View, WHITE, row};

pub const SPEED_RANGE: std::ops::RangeInclusive<f32> = 1.0..=6.0;
// UNVERIFIED: step 0.5 (doc 05 §1.5).
pub const SPEED_STEP: f32 = 0.5;
/// Judgement adjustment −10.0..=+10.0 in steps of 0.1 (doc 05 §1.5).
pub const OFFSET_RANGE: std::ops::RangeInclusive<f32> = -10.0..=10.0;
pub const OFFSET_STEP: f32 = 0.1;
/// Seconds of chart time per judgement-adjustment unit: half a 60 fps frame (8.33 ms), so the
/// setting spans ±83 ms. A positive setting runs the judgement clock ahead of the music: a tap
/// in time with the audio is judged late, so taps must come that much earlier.
// UNVERIFIED: unit and sign (doc 04 §8, PLAN §5 #9); 8.33 ms/unit is the doc's more plausible
// reading of Saturn's contradictory 83.3 vs 8.33 ms.
const OFFSET_UNIT_SECS: f64 = 1.0 / 120.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Speed,
    Offset,
    Mirror,
    Autoplay,
    Video,
}

pub const ROWS: [Row; 5] = [
    Row::Speed,
    Row::Offset,
    Row::Mirror,
    Row::Autoplay,
    Row::Video,
];

/// Judge offset setting → user offset in seconds (added to chart time).
pub fn offset_secs(judge_offset: f32) -> f64 {
    f64::from(judge_offset) * OFFSET_UNIT_SECS
}

/// Offset in seconds → the nearest setting (one decimal, clamped to [`OFFSET_RANGE`]).
pub fn offset_units(secs: f64) -> f32 {
    let units = if secs.is_finite() {
        tenth((secs / OFFSET_UNIT_SECS) as f32)
    } else {
        0.0
    };
    units.clamp(*OFFSET_RANGE.start(), *OFFSET_RANGE.end())
}

/// Round to one decimal so repeated steps do not drift.
fn tenth(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptionsMenu {
    pub row: usize,
}

impl OptionsMenu {
    pub fn move_by(&mut self, n: i32) {
        self.row = (self.row as i32 + n).rem_euclid(ROWS.len() as i32) as usize;
    }

    /// Change the value of the selected row by `dir` steps (±1).
    pub fn adjust(&self, s: &mut Settings, autoplay: &mut bool, dir: i32) {
        let d = dir.signum() as f32;
        match ROWS[self.row] {
            Row::Speed => {
                s.note_speed = tenth(s.note_speed + d * SPEED_STEP)
                    .clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end());
            }
            Row::Offset => {
                s.judge_offset = tenth(s.judge_offset + d * OFFSET_STEP)
                    .clamp(*OFFSET_RANGE.start(), *OFFSET_RANGE.end());
            }
            Row::Mirror => s.mirror = !s.mirror,
            Row::Autoplay => *autoplay = !*autoplay,
            Row::Video => s.background_video = !s.background_video,
        }
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

pub fn value(row: Row, s: &Settings, autoplay: bool) -> String {
    match row {
        Row::Speed => format!("{:.1}", s.note_speed),
        Row::Offset => format!("{:+.1}", s.judge_offset),
        Row::Mirror => on_off(s.mirror).into(),
        Row::Autoplay => on_off(autoplay).into(),
        Row::Video => on_off(s.background_video).into(),
    }
}

fn label(row: Row) -> &'static str {
    match row {
        Row::Speed => "NOTE SPEED",
        Row::Offset => "JUDGE OFFSET",
        Row::Mirror => "MIRROR",
        Row::Autoplay => "AUTOPLAY",
        Row::Video => "BACKGROUND VIDEO",
    }
}

/// Each row: label, `◀ value ▶` with tappable halves (left = −, right = +).
pub fn view(menu: &OptionsMenu, s: &Settings, autoplay: bool, title: &str, area: Rect) -> View {
    let u = area.w / 100.0;
    let mut v = View::default();
    let cx = area.x + area.w / 2.0;
    v.text_in(
        "OPTIONS",
        [cx, area.y + 4.0 * u],
        7.0 * u,
        WHITE,
        Align::Center,
        None,
    );
    v.text_in(
        title,
        [cx, area.y + 14.0 * u],
        4.5 * u,
        GREY,
        Align::Center,
        Some(area.w - 6.0 * u),
    );
    let top = area.y + 26.0 * u;
    let h = 18.0 * u;
    for (k, &r) in ROWS.iter().enumerate() {
        let rect = row(area, top, h, k);
        v.fill(rect, if k == menu.row { HIGHLIGHT } else { PANEL });
        v.text(
            label(r),
            [rect.x + 3.0 * u, rect.y + 2.0 * u],
            4.5 * u,
            GREY,
        );
        v.text_in(
            format!("◀  {}  ▶", value(r, s, autoplay)),
            [cx, rect.y + 8.0 * u],
            6.0 * u,
            WHITE,
            Align::Center,
            None,
        );
        let half = |n: f32| Rect {
            x: rect.x + n * rect.w / 2.0,
            w: rect.w / 2.0,
            ..rect
        };
        // Pick(2k) = decrease row k, Pick(2k + 1) = increase.
        v.button(half(0.0), Action::Pick(2 * k));
        v.button(half(1.0), Action::Pick(2 * k + 1));
    }
    buttons(&mut v, area, "BACK", "START");
    v
}

/// Handle a tap on [`view`]'s `Pick(n)`: select row `n / 2` and step it down (even) or up (odd).
pub fn pick(menu: &mut OptionsMenu, n: usize, s: &mut Settings, autoplay: &mut bool) {
    if n / 2 < ROWS.len() {
        menu.row = n / 2;
        menu.adjust(s, autoplay, if n.is_multiple_of(2) { -1 } else { 1 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjusts_and_clamps_values() {
        let mut s = Settings::default();
        let mut auto = false;
        let mut m = OptionsMenu::default();
        m.adjust(&mut s, &mut auto, 1);
        assert_eq!(s.note_speed, 4.5);
        for _ in 0..10 {
            m.adjust(&mut s, &mut auto, 1);
        }
        assert_eq!(s.note_speed, 6.0);
        m.move_by(1);
        for _ in 0..3 {
            m.adjust(&mut s, &mut auto, -1);
        }
        assert_eq!(s.judge_offset, -0.3);
        for _ in 0..200 {
            m.adjust(&mut s, &mut auto, -1);
        }
        assert_eq!(s.judge_offset, -10.0);
        m.move_by(1);
        m.adjust(&mut s, &mut auto, 1);
        assert!(s.mirror);
        m.move_by(1);
        m.adjust(&mut s, &mut auto, -1);
        assert!(auto);
        m.move_by(1);
        m.adjust(&mut s, &mut auto, 1);
        assert!(!s.background_video);
        assert_eq!(value(Row::Video, &s, auto), "OFF");
        m.move_by(1);
        assert_eq!(m.row, 0);
        m.move_by(-1);
        assert_eq!(ROWS[m.row], Row::Video);
    }

    #[test]
    fn offset_unit_and_labels() {
        assert!((offset_secs(1.5) - 0.0125).abs() < 1e-9);
        assert_eq!(offset_units(0.025), 3.0);
        assert_eq!(offset_units(offset_secs(-2.4)), -2.4);
        assert_eq!(offset_units(1.0), 10.0);
        assert_eq!(offset_units(f64::NAN), 0.0);
        let s = Settings::default();
        assert_eq!(value(Row::Speed, &s, false), "4.0");
        assert_eq!(value(Row::Offset, &s, false), "+0.0");
        assert_eq!(value(Row::Autoplay, &s, true), "ON");
    }

    #[test]
    fn taps_step_rows_down_and_up() {
        let mut s = Settings::default();
        let (mut auto, mut m) = (false, OptionsMenu::default());
        pick(&mut m, 0, &mut s, &mut auto);
        assert_eq!((m.row, s.note_speed), (0, 3.5));
        pick(&mut m, 3, &mut s, &mut auto);
        assert_eq!((m.row, s.judge_offset), (1, 0.1));
        pick(&mut m, 99, &mut s, &mut auto);
        assert_eq!(m.row, 1);
        let v = view(
            &m,
            &s,
            auto,
            "Song",
            Rect {
                x: 0.0,
                y: 0.0,
                w: 900.0,
                h: 1600.0,
            },
        );
        assert!(v.texts().contains(&"◀  +0.1  ▶"));
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Confirm));
    }
}
