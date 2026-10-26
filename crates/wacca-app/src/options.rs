//! Options screen: note speed, judgement offset, mirror, autoplay, background video, bar lines and
//! the touch guide, stored in the save's [`Settings`] (autoplay is per session and never saved).
//!
//! This module is the one place that knows the judge-offset unit: everything else converts with
//! [`offset_secs`] / [`offset_units`].

use std::sync::Arc;

use wacca_game::save::Settings;
use wacca_render::text::Align;
use wacca_render::{Background, BgFrame, BgLibrary, BgState};

use wacca_game::save::SaveData;
use wacca_render::style::Rgba;

use crate::canvas::{FULL, MID, Painter, TEX, hex};
use crate::chrome::{self, Icon, PINK, WHITE};
use crate::select::SongList;
use crate::select_ui;
use crate::ui::{Action, View};

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
    /// macOS only: play with the trackpad as a touch ring ([`crate::trackpad`]).
    Trackpad,
    /// The performance overlay (F3 on a keyboard).
    PerfInfo,
}

const COMMON_ROWS: [Row; 9] = [
    Row::Speed,
    Row::Offset,
    Row::Check,
    Row::Mirror,
    Row::Autoplay,
    Row::Video,
    Row::BarLines,
    Row::TouchGuide,
    Row::PerfInfo,
];

/// The rows on this platform (TRACKPAD on macOS only).
pub const ROWS: &[Row] = if cfg!(target_os = "macos") {
    &[
        Row::Speed,
        Row::Offset,
        Row::Check,
        Row::Mirror,
        Row::Autoplay,
        Row::Video,
        Row::BarLines,
        Row::TouchGuide,
        Row::Trackpad,
        Row::PerfInfo,
    ]
} else {
    &COMMON_ROWS
};

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
            Row::Trackpad => s.trackpad_touch = !s.trackpad_touch,
            Row::PerfInfo => s.perf_info = !s.perf_info,
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
        Row::Trackpad => on_off(s.trackpad_touch).into(),
        Row::PerfInfo => on_off(s.perf_info).into(),
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
        Row::Trackpad => "TRACKPAD",
        Row::PerfInfo => "PERF INFO",
    }
}

/// Rows per page: the real final check shows four option rows beside the preview monitor.
pub const PAGE: usize = 4;
/// `Pick` index of the OPTIONS tab, which turns to the other page of rows.
pub const NEXT_PAGE: usize = 2 * ROWS.len();

/// Row `k` of a page (0..[`PAGE`]): centre of the `uT_STmenu_obj013` slab. Measured on
/// `runs/e3/*/nav-20-options.png`: the rows step down and right along the rim.
fn row_at(k: usize) -> [f32; 2] {
    [298.0 + 20.0 * k as f32, -136.0 + 38.0 * k as f32]
}
const ROW_SIZE: [f32; 2] = [268.0, 34.0];
/// Label right edge and value centre, relative to the row centre.
const LABEL_RIGHT: f32 = -46.0;
const VALUE_X: f32 = 66.0;
const VALUE_INK: u32 = 0x1c0f3a;

/// One line with fixed `pitch` between character centres, centred on `at`.
fn spaced(p: &mut Painter, text: &str, at: [f32; 2], size: f32, pitch: f32, color: Rgba) {
    let n = text.chars().count() as f32;
    for (i, ch) in text.chars().enumerate() {
        let x = at[0] + (i as f32 - (n - 1.0) / 2.0) * pitch;
        p.text(ch.to_string(), [x, at[1]], size, color, Align::Center);
    }
}

/// The final check (`BP_Select01`, `nav-20` in E3): the navigator (`uT_Chara002_00`) on the left,
/// the preview monitor in the middle with the option rows beside it, the selected song's plate
/// at the bottom and DIFFICULTY / BACK / OPTIONS / GO on the rim. Our eight options take two
/// pages of the four rows; the OPTIONS tab turns the page (the original opens its option wheel
/// there). The selected row (keyboard) is flanked by arrows; tapping left / right of a value's
/// centre steps it down / up.
pub fn view(
    c: crate::canvas::Canvas,
    menu: &OptionsMenu,
    (s, autoplay): (&Settings, bool),
    list: &SongList,
    save: &SaveData,
    who: &chrome::Player,
    live_monitor: bool,
) -> View {
    let mut p = Painter::new(c);
    chrome::background(&mut p, chrome::SELECT_BACKDROP);
    // Character art, then the panel around the monitor and the rows' stripes.
    // UNVERIFIED: placed by matching the E3 frame (eyes and bow), not from the widget.
    p.image(
        &format!("{TEX}CHARACTER/chara002/uT_Chara002_00"),
        [-307.0, 390.0],
        [900.0, 1800.0],
        MID,
        FULL,
        0.0,
        WHITE,
    );
    p.tex_ex(
        "uT_STmenu_obj006",
        [-40.0, -80.0],
        [780.0, 200.0],
        [0.0, 0.5],
        FULL,
        0.0,
        WHITE,
    );
    p.tex_ex(
        "uT_STmenu_obj000",
        [-185.0, -182.0],
        [717.0, 382.0],
        [0.0, 0.0],
        FULL,
        0.0,
        WHITE,
    );
    monitor(&mut p, live_monitor);
    let page = menu.row / PAGE;
    for k in 0..PAGE {
        let i = page * PAGE + k;
        let Some(&r) = ROWS.get(i) else { break };
        let at = row_at(k);
        p.tex("uT_STmenu_obj013", at, ROW_SIZE, WHITE);
        let name = label(r);
        let size = if name.len() > 8 { 11.0 } else { 15.0 };
        p.text(name, [at[0] + LABEL_RIGHT, at[1]], size, PINK, Align::Right);
        let v = [at[0] + VALUE_X, at[1]];
        let ink = hex(VALUE_INK, 1.0);
        spaced(&mut p, &value(r, s, autoplay), v, 15.0, 24.0, ink);
        if i == menu.row {
            chrome::icon(&mut p, Icon::Left, [v[0] - 82.0, v[1]], 22.0, 0.0);
            chrome::icon(&mut p, Icon::Right, [v[0] + 82.0, v[1]], 22.0, 0.0);
        }
        // Pick(2i) = decrease row i, Pick(2i + 1) = increase.
        p.button([v[0] - 70.0, at[1]], [140.0, 36.0], Action::Pick(2 * i));
        p.button([v[0] + 45.0, at[1]], [90.0, 36.0], Action::Pick(2 * i + 1));
    }
    if let Some(song) = list.current() {
        p.fill([260.0, 186.0], [110.0, 110.0], MID, WHITE);
        p.jacket(&song.jacket, [260.0, 186.0], 106.0);
    }
    select_ui::info_plate(&mut p, list, save);
    chrome::user_plate(&mut p, who);
    rim(&mut p);
    p.circle_mask();
    p.finish()
}

/// UI image key of the live monitor (the renderer's background display, registered by the shell).
pub const MONITOR_KEY: &str = "live/background";

/// The live picture behind the final check's monitor: the selected chart's no-movie background
/// scene, in its Normal state as a play starts it.
#[derive(Debug)]
pub struct Monitor {
    bg: Background,
    pub frame: BgFrame,
    time: f32,
}

impl Monitor {
    pub fn new(lib: Arc<BgLibrary>) -> Self {
        let mut bg = Background::new(lib);
        bg.set_state(BgState::Normal);
        Self {
            bg,
            frame: BgFrame::default(),
            time: 0.0,
        }
    }

    /// Advances the scene by `dt` seconds. The materials' beat and bar run at the song's `bpm`
    /// from the monitor's start (UNVERIFIED in the menus).
    pub fn update(&mut self, dt: f32, bpm: f32) {
        self.bg.update(dt);
        self.time += dt;
        self.bg.build(&mut self.frame);
        let beats = self.time * bpm / 60.0;
        self.frame.time = self.time;
        self.frame.beat = beats.fract();
        self.frame.bar = (beats / 4.0).fract();
    }
}

/// The first number of a catalog BPM text (`"150"`, `"120-180"`), else 120.
pub fn bpm_of(text: &str) -> f32 {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok().filter(|b| *b > 0.0).unwrap_or(120.0)
}

/// `uM_STmoniter00`: the round preview, a disc (`uT_CMen512_00`) of the live play-background
/// capture (`BgCaptureTextureRenderTarget2D`). `live`: the shell renders the background scene
/// into [`MONITOR_KEY`] (charts without a movie).
// UNVERIFIED otherwise (movie charts): drawn as its look on the E3 frame, a dark blue disc
// brightening to violet in the middle.
fn monitor(p: &mut Painter, live: bool) {
    let at = [0.0, 3.0];
    if live {
        p.disc_image(MONITOR_KEY, at, 300.0, WHITE);
        return;
    }
    p.tex("uT_CMen512_00", at, [300.0, 300.0], hex(0x120d3c, 1.0));
    for k in 1..=6 {
        let d = 300.0 - 40.0 * k as f32;
        p.tex("uT_CMen512_00", at, [d, d], hex(0x4c45d8, 0.16));
    }
}

/// DIFFICULTY (left), BACK, the OPTIONS tab (right) and GO, as on the real final check.
fn rim(p: &mut Painter) {
    chrome::go(p);
    chrome::difficulty(p);
    chrome::back(p);
    chrome::icon(p, Icon::Options, [484.0, 43.0], 30.0, 0.0);
    p.text_turned("OPTIONS", [486.0, -18.0], 9.0, WHITE, -90.0);
    p.button([480.0, 20.0], [70.0, 130.0], Action::Pick(NEXT_PAGE));
}

/// Handle a tap on [`view`]'s `Pick(n)`: select row `n / 2` and step it down (even) or up (odd).
pub fn pick(menu: &mut OptionsMenu, n: usize, s: &mut Settings, autoplay: &mut bool) {
    if n == NEXT_PAGE {
        // The next page's first row; after the last (possibly partial) page, the first.
        let next = (menu.row / PAGE + 1) * PAGE;
        menu.row = if next < ROWS.len() { next } else { 0 };
    } else if n / 2 < ROWS.len() {
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
        if cfg!(target_os = "macos") {
            m.move_by(1);
            assert_eq!(value(Row::Trackpad, &s, auto), "OFF");
            m.adjust(&mut s, &mut auto, 1);
            assert!(s.trackpad_touch);
        }
        m.move_by(1);
        assert_eq!(value(Row::PerfInfo, &s, auto), "OFF");
        m.adjust(&mut s, &mut auto, 1);
        assert!(s.perf_info);
        m.move_by(1);
        assert_eq!(m.row, 0);
        m.move_by(-1);
        assert_eq!(ROWS[m.row], *ROWS.last().unwrap());
    }

    #[test]
    fn rows_and_their_arrows_stay_inside_the_circle() {
        for k in 0..PAGE {
            let [x, y] = row_at(k);
            let right = [x + VALUE_X + 93.0, y + ROW_SIZE[1] / 2.0];
            let d = right[0].hypot(right[1]);
            assert!(d < crate::canvas::CIRCLE_R, "row {k}: {d}");
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
    fn live_monitor_shows_the_background_disc() {
        let (s, m) = (Settings::default(), OptionsMenu::default());
        let list = crate::select::tests::list();
        let (who, save) = (crate::chrome::Player::default(), SaveData::default());
        let c = crate::canvas::Canvas::fit(900.0, 1600.0);
        let has_live = |live| {
            view(c, &m, (&s, false), &list, &save, &who, live)
                .items
                .iter()
                .any(|i| matches!(i, crate::ui::Item::ImagePoly { key, .. } if key == MONITOR_KEY))
        };
        assert!(has_live(true) && !has_live(false));
        assert_eq!(bpm_of("150"), 150.0);
        assert_eq!(bpm_of(" 120-180"), 120.0);
        assert_eq!(bpm_of("???"), 120.0);
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
        let list = crate::select::tests::list();
        let who = crate::chrome::Player::default();
        let save = SaveData::default();
        let c = crate::canvas::Canvas::fit(900.0, 1600.0);
        let v = view(c, &m, (&s, auto), &list, &save, &who, false);
        let t = v.texts().concat();
        assert!(t.contains("TIMING+0.1") && !t.contains("BG MOVIE"), "{t}");
        // The OPTIONS tab turns to the second page of rows and back.
        pick(&mut m, NEXT_PAGE, &mut s, &mut auto);
        assert_eq!(m.row, PAGE);
        let t = view(c, &m, (&s, auto), &list, &save, &who, false)
            .texts()
            .concat();
        assert!(t.contains("BG MOVIE") && !t.contains("SPEED"), "{t}");
        // A partial last page (macOS: TRACKPAD), then back to the first.
        if ROWS.len() > 2 * PAGE {
            pick(&mut m, NEXT_PAGE, &mut s, &mut auto);
            assert_eq!(m.row, 2 * PAGE);
        }
        pick(&mut m, NEXT_PAGE, &mut s, &mut auto);
        assert_eq!(m.row, 0);
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
