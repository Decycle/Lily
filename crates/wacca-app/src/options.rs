//! Options screen: note speed, judgement offset, mirror, autoplay, background video, bar lines and
//! the touch guide, stored in the save's [`Settings`] (autoplay is per session and never saved).
//!
//! This module is the one place that knows the judge-offset unit: everything else converts with
//! [`offset_secs`] / [`offset_units`].

use wacca_game::save::Settings;
use wacca_render::text::Align;

use crate::circle::{self, Canvas, NAVY, PINK, slab};
use crate::ui::{Action, View, WHITE};

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
    /// Opens the timing check ([`crate::calibrate`]); has no value of its own.
    Check,
    Mirror,
    Autoplay,
    Video,
    BarLines,
    TouchGuide,
}

pub const ROWS: [Row; 8] = [
    Row::Speed,
    Row::Offset,
    Row::Check,
    Row::Mirror,
    Row::Autoplay,
    Row::Video,
    Row::BarLines,
    Row::TouchGuide,
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
            Row::Check => {}
            Row::Mirror => s.mirror = !s.mirror,
            Row::Autoplay => *autoplay = !*autoplay,
            Row::Video => s.background_video = !s.background_video,
            Row::BarLines => s.bar_lines = !s.bar_lines,
            Row::TouchGuide => s.touch_guide = !s.touch_guide,
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
        Row::Check => "START".into(),
        Row::Mirror => on_off(s.mirror).into(),
        Row::Autoplay => on_off(autoplay).into(),
        Row::Video => on_off(s.background_video).into(),
        Row::BarLines => on_off(s.bar_lines).into(),
        Row::TouchGuide => on_off(s.touch_guide).into(),
    }
}

fn label(row: Row) -> &'static str {
    match row {
        Row::Speed => "SPEED",
        Row::Offset => "TIMING",
        Row::Check => "TIMING CHECK",
        Row::Mirror => "MIRROR",
        Row::Autoplay => "AUTOPLAY",
        Row::Video => "BG MOVIE",
        Row::BarLines => "BAR LINES",
        Row::TouchGuide => "TOUCH GUIDE",
    }
}

/// Rows styled like the game's option panel (`nav-20-options` in E2): a navy label slab with
/// pink text and a pink value slab with spaced navy text, stepping right down the list. The
/// selected row is lit and flanked by arrows; tapping left / right of the value's centre steps
/// it down / up. BACK and GO sit at the bottom of the circle.
pub fn view(
    menu: &OptionsMenu,
    s: &Settings,
    autoplay: bool,
    title: &str,
    frame: crate::canvas::Canvas,
) -> View {
    let mut v = View::default();
    let c = Canvas::new(frame);
    circle::background(&mut v, &c);
    c.panel(&mut v, &slab(350.0, 560.0, 420.0, 56.0), &[NAVY]);
    c.text(
        &mut v,
        "OPTIONS",
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
    c.text_fit(&mut v, title, [520.0, 626.0], 24.0, PINK, 380.0);
    let h = ROW_H;
    for (k, &r) in ROWS.iter().enumerate() {
        let sel = k == menu.row;
        let (x, y) = row_origin(k);
        let ym = y + h / 2.0;
        let label_bg = if sel { [0.25, 0.18, 0.55, 1.0] } else { NAVY };
        c.panel(&mut v, &slab(x, y, LABEL_W, h), &[label_bg]);
        let lx = x + LABEL_W - h * circle::SLANT - 18.0;
        c.text(&mut v, label(r), [lx, ym - 17.0], 28.0, PINK, Align::Right);
        let vx = x + LABEL_W + 30.0;
        let mut pink = PINK;
        pink[3] = if sel { 1.0 } else { 0.8 };
        c.panel(&mut v, &slab(vx, y, VALUE_W, h), &[pink]);
        let text = value(r, s, autoplay);
        let pitch = 26.0;
        let vc = vx + VALUE_W / 2.0 - h * circle::SLANT / 2.0;
        let start = vc - pitch * text.chars().count() as f32 / 2.0;
        c.spaced(&mut v, &text, [start, ym - 20.0], 34.0, pitch, NAVY);
        if sel {
            let (l, r) = (vx - h * circle::SLANT / 2.0 - 22.0, vx + VALUE_W + 2.0);
            c.panel(
                &mut v,
                &[[l, ym], [l + 20.0, ym - 16.0], [l + 20.0, ym + 16.0]],
                &[WHITE],
            );
            c.panel(
                &mut v,
                &[[r + 20.0, ym], [r, ym + 16.0], [r, ym - 16.0]],
                &[WHITE],
            );
        }
        // Pick(2k) = decrease row k, Pick(2k + 1) = increase.
        let x0 = x - h * circle::SLANT;
        c.button(&mut v, [x0, y, vc - x0, h], Action::Pick(2 * k));
        c.button(
            &mut v,
            [vc, y, vx + VALUE_W + 40.0 - vc, h],
            Action::Pick(2 * k + 1),
        );
    }
    circle::side_tab(
        &mut v,
        &c,
        "BACK",
        300.0,
        [0.55, 0.20, 0.80, 0.95],
        Action::Back,
    );
    circle::go_button(&mut v, &c, Action::Confirm);
    v
}

// Eight rows fit above the BACK tab (y 1300) and inside the circle.
const ROW_TOP: f32 = 680.0;
const ROW_PITCH: f32 = 70.0;
const ROW_H: f32 = 56.0;
const LABEL_W: f32 = 290.0;
const VALUE_W: f32 = 280.0;

/// Top-left of row `k`'s label slab; rows step right like the game's panel.
fn row_origin(k: usize) -> (f32, f32) {
    (210.0 + 14.0 * k as f32, ROW_TOP + ROW_PITCH * k as f32)
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
        assert_eq!(value(Row::Check, &s, auto), "START");
        let before = s.clone();
        m.adjust(&mut s, &mut auto, 1);
        assert_eq!(s, before);
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
        // Bar lines start on (like real guest play), the touch guide off; both toggle.
        m.move_by(1);
        assert_eq!(value(Row::BarLines, &s, auto), "ON");
        m.adjust(&mut s, &mut auto, 1);
        assert!(!s.bar_lines);
        m.move_by(1);
        assert_eq!(value(Row::TouchGuide, &s, auto), "OFF");
        m.adjust(&mut s, &mut auto, -1);
        assert!(s.touch_guide);
        assert_eq!(value(Row::TouchGuide, &s, auto), "ON");
        m.move_by(1);
        assert_eq!(m.row, 0);
        m.move_by(-1);
        assert_eq!(ROWS[m.row], Row::TouchGuide);
    }

    #[test]
    fn all_rows_fit_above_the_back_tab_inside_the_circle() {
        for k in 0..ROWS.len() {
            let (x, y) = row_origin(k);
            assert!(y + ROW_H < 1300.0, "row {k}");
            // The value slab's right edge and the arrow stay inside the circle.
            let right = [x + LABEL_W + 30.0 + VALUE_W + 22.0, y + ROW_H];
            let d = (right[0] - circle::CENTER[0]).hypot(right[1] - circle::CENTER[1]);
            assert!(d < circle::RADIUS, "row {k}: {d}");
        }
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
            crate::canvas::Canvas::fit(900.0, 1600.0),
        );
        let t = v.texts().concat();
        assert!(t.contains("TIMING+0.1") && t.contains("Song"), "{t}");
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Confirm));
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Back));
        // Left of the value's centre steps down, right of it steps up.
        let (down, _) = v
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Pick(2))
            .unwrap();
        let (up, _) = v
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Pick(3))
            .unwrap();
        assert!(down.x < up.x && (down.x + down.w - up.x).abs() < 0.01);
        assert_eq!(
            v.hit([up.x + 1.0, up.y + up.h / 2.0]),
            Some(Action::Pick(3))
        );
    }
}
