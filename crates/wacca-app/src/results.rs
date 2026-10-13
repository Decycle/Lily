//! Results screen (`docs/research/05-save-and-song-select.md` §4), laid out like the game's
//! `BP_Result_score` on the round screen: player plate on top, jacket block on the left,
//! judgement / R-note / FAST-LATE / COMBO panels and the score on the right, the rank band below,
//! the rating strip and GO at the bottom. Positions, colours and timings are measured from the E2
//! recordings (`runs/e2/*/video.mkv`, end of the song).

use wacca_chart::Difficulty;
use wacca_game::grade::{Grade, Lamp, PlayResult};
use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::circle::{self, Canvas, INK, PINK, ease, font_tex, slab};
use crate::labels;
use crate::ui::{Action, Item, View, WHITE};

/// Everything the results screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub title: String,
    pub artist: String,
    pub jacket: String,
    pub difficulty: Option<Difficulty>,
    pub level: Option<f32>,
    pub result: PlayResult,
    /// Played to the end (not given up).
    pub completed: bool,
    pub new_best: bool,
    /// Why the play was not saved (autoplay, unknown song, write error).
    pub not_saved: Option<String>,
    /// Name on the player plate (save settings).
    pub player: String,
    pub rating: Rating,
}

/// Player rating ([`wacca_game::rating::save_rating`]) before and after one play.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rating {
    pub before: f64,
    pub after: f64,
}

impl Rating {
    /// Rating as the game prints it: whole points, rounded down (E2: an A on a level-1 chart,
    /// 0.8, shows 0; MASTER, 4.0, shows 4).
    pub fn shown(r: f64) -> u32 {
        r.max(0.0).floor() as u32
    }

    /// Shown gain of this play, if any (the `+4` under the up-arrow).
    pub fn gain(&self) -> Option<u32> {
        let (b, a) = (Self::shown(self.before), Self::shown(self.after));
        (a > b).then(|| a - b)
    }
}

// Entry animation, seconds after the screen appears (E2 recording).
const INFO_AT: f32 = 0.4;
const LIST_AT: f32 = 0.9;
const SLIDE: f32 = 0.35;
const COUNT_AT: f32 = 1.25;
const COUNT_LEN: f32 = 1.35;
const RANK_AT: f32 = 4.6;
const FOOTER_AT: f32 = 5.0;

const PANEL: Rgba = [0.95, 0.95, 0.94, 1.0];
const PANEL_LOW: Rgba = [0.86, 0.86, 0.87, 1.0];
const DARK: Rgba = [0.24, 0.24, 0.25, 0.95];
const TAB: Rgba = [0.42, 0.42, 0.47, 1.0];
const GREY_TEXT: Rgba = [0.45, 0.44, 0.48, 1.0];
const TITLE: Rgba = [0.95, 0.10, 0.55, 1.0];
const R_NOTE: Rgba = [0.98, 0.22, 0.52, 1.0];
const GAIN: Rgba = [1.0, 0.86, 0.20, 1.0];
/// Left boundary of the right-hand panels: x at `y = 638`, leaning with [`circle::SLANT`].
const LIST_X: f32 = 470.0;
const LIST_TOP: f32 = 638.0;
/// Rank band: top-left corner x, y, top width, height.
const BAND: [f32; 4] = [255.0, 985.0, 745.0, 243.0];

/// Zero-padded counter as the game prints it (`0011`, `0135862`).
pub fn padded(n: u32, digits: usize) -> String {
    format!("{n:0digits$}")
}

/// Share of all notes, `007.6%`.
pub fn rate(n: u32, total: u32) -> String {
    let pct = if total == 0 {
        0.0
    } else {
        f64::from(n) * 100.0 / f64::from(total)
    };
    format!("{pct:05.1}%")
}

/// Score shown `t` seconds after the screen appears (counts up).
pub fn shown_score(score: u32, t: f32) -> u32 {
    let p = ((t - COUNT_AT) / COUNT_LEN).clamp(0.0, 1.0);
    (score as f32 * p).round() as u32
}

/// Word-art letters of a grade (`uT_FTrate*`): repeated letters for AA/SS…, `ST01` = "S+".
pub fn rank_art(g: Grade) -> Vec<&'static str> {
    let (a, s, sp) = ("uT_FTrateAT00", "uT_FTrateST00", "uT_FTrateST01");
    match g {
        Grade::D => vec!["uT_FTrateDT00"],
        Grade::C => vec!["uT_FTrateCT00"],
        Grade::B => vec!["uT_FTrateBT00"],
        Grade::A => vec![a],
        Grade::AA => vec![a, a],
        Grade::AAA => vec![a, a, a],
        Grade::S => vec![s],
        Grade::SPlus => vec![sp],
        Grade::SS => vec![s, s],
        Grade::SsPlus => vec![s, sp],
        Grade::SSS => vec![s, s, s],
        Grade::SssPlus => vec![s, s, sp],
        Grade::Master => vec![
            "uT_FTrateMT00",
            "uT_FTrateMT01",
            "uT_FTrateMT02",
            "uT_FTrateMT03",
            "uT_FTrateMT04",
            "uT_FTrateMT05",
        ],
    }
}

/// Look of the rank band (materials `uM_RTrate{M,S,A,B,C,D}*`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Band {
    /// A triangle-pattern texture (`uT_RTrate*BG00`) on a pale base.
    Pattern(&'static str, Rgba),
    /// A vertical gradient (top, bottom) crossed by the faint X shapes of `uT_RTrateBBG00`.
    Crossed(Rgba, Rgba),
}

fn band(g: Grade) -> Band {
    match g {
        Grade::Master => Band::Pattern("uT_RTrateMBG00", [1.0, 0.90, 0.96, 1.0]),
        // UNVERIFIED: no S-family result was recorded; the S material uses this texture.
        Grade::S | Grade::SPlus | Grade::SS | Grade::SsPlus | Grade::SSS | Grade::SssPlus => {
            Band::Pattern("uT_RTrateSBG00", [1.0, 0.93, 0.88, 1.0])
        }
        // A and B measured in E2 (runs -235920, -234221).
        Grade::A | Grade::AA | Grade::AAA => {
            Band::Crossed([0.97, 0.95, 0.45, 1.0], [0.91, 0.86, 0.09, 1.0])
        }
        Grade::B => Band::Crossed([0.27, 0.85, 0.92, 1.0], [0.03, 0.47, 0.75, 1.0]),
        // UNVERIFIED: C and D not recorded; colours follow their letters.
        Grade::C => Band::Crossed([0.45, 0.62, 1.0, 1.0], [0.12, 0.30, 0.85, 1.0]),
        Grade::D => Band::Crossed([0.66, 0.67, 0.70, 1.0], [0.38, 0.39, 0.43, 1.0]),
    }
}

/// Lamp word art (`uT_FTclear_09x`), its drawn size and the right edge of its letters (fraction
/// of the texture width): the game right-aligns the lamp words.
fn lamp_art(l: Lamp) -> (&'static str, [f32; 2], f32) {
    let (wide, small) = ([200.0, 50.0], [232.0, 58.0]);
    match l {
        Lamp::Failed => ("uT_FTclear_095", wide, 0.785),
        Lamp::Clear => ("uT_FTclear_094", wide, 0.804),
        Lamp::Missless => ("uT_FTclear_093", small, 0.887),
        Lamp::FullCombo => ("uT_FTclear_091", wide, 0.950),
        Lamp::AllMarvelous => ("uT_FTclear_092", small, 0.984),
    }
}

/// Judgement word art with the left edge of its letters (fraction of the texture width): the
/// game left-aligns the four words.
const JUDGE_ART: [(&str, f32); 4] = [
    ("uT_FTdecision01", 0.008),
    ("uT_FTdecision02", 0.211),
    ("uT_FTdecision03", 0.234),
    ("uT_FTdecision04", 0.273),
];

/// "New Record!!" letters (`uT_FTnewrecord_00..09`): letter left / right edge in the texture.
const NEW_RECORD: [(f32, f32); 10] = [
    (0.150, 0.845),
    (0.162, 0.835),
    (0.0, 1.0),
    (0.129, 0.883),
    (0.162, 0.835),
    (0.176, 0.712),
    (0.157, 0.842),
    (0.198, 0.799),
    (0.160, 0.850),
    (0.238, 0.754),
];

/// Top of a line of `px` text whose digits are centred on `y`.
fn mid(y: f32, px: f32) -> f32 {
    y - 0.58 * px
}

/// Texture key of a root `UI/Textures` image.
fn ui_tex(name: &str) -> String {
    format!("UI/Textures/{name}")
}

/// Results for `s`, `t` seconds after the screen appeared.
pub fn view(s: &Summary, area: Rect, t: f32) -> View {
    let mut v = View::default();
    let c = Canvas::new(area);
    circle::background(&mut v, &c);
    let e = ease(t, INFO_AT, SLIDE);
    header(&mut v, &c.moved([0.0, -120.0 * (1.0 - e)], e), s);
    info(&mut v, &c.moved([700.0 * (1.0 - e), 0.0], e), s);
    let e = ease(t, LIST_AT, SLIDE);
    let d = 420.0 * (1.0 - e);
    list(&mut v, &c.moved([0.6 * d, 0.8 * d], e), s, t);
    rank(&mut v, &c, s.result.grade, t);
    footer(&mut v, &c.moved([0.0, 0.0], ease(t, FOOTER_AT, 0.3)), s);
    v
}

/// Regular polygon approximating a circle (for the player icon).
fn round(center: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            [center[0] + r * a.cos(), center[1] + r * a.sin()]
        })
        .collect()
}

/// Player plate: title and level bars, icon, name and the rating box (no credit display).
fn header(v: &mut View, c: &Canvas, s: &Summary) {
    c.panel(
        v,
        &slab(330.0, 420.0, 600.0, 44.0),
        &[[0.30, 0.30, 0.33, 0.95]],
    );
    c.text(
        v,
        "Title",
        [312.0, 434.0],
        14.0,
        [0.70, 0.70, 0.73, 1.0],
        Align::Left,
    );
    c.panel(
        v,
        &slab(352.0, 467.0, 580.0, 32.0),
        &[[0.17, 0.17, 0.19, 0.97]],
    );
    let plate = [0.90, 0.90, 0.91, 1.0];
    let plate_low = [0.78, 0.78, 0.80, 1.0];
    c.panel(
        v,
        &slab(285.0, 512.0, 475.0, 76.0),
        &[plate, plate, plate_low, plate_low],
    );
    c.panel(
        v,
        &slab(762.0, 512.0, 160.0, 76.0),
        &[[0.32, 0.32, 0.34, 0.95]],
    );
    c.spaced(v, &s.player, [380.0, mid(550.0, 28.0)], 28.0, 21.0, INK);
    c.panel(v, &round([292.0, 548.0], 68.0), &[WHITE]);
    c.panel(v, &round([292.0, 548.0], 56.0), &[PINK]);
    c.panel(v, &round([292.0, 548.0], 46.0), &[WHITE]);
    c.panel(
        v,
        &slab(830.0, 551.0, 60.0, 13.0),
        &[[0.20, 0.20, 0.22, 1.0]],
    );
    c.text(v, "Rate:", [852.0, 551.0], 12.0, WHITE, Align::Center);
    c.panel(v, &slab(822.0, 565.0, 100.0, 32.0), &[WHITE]);
    let after = Rating::shown(s.rating.after).to_string();
    c.numerals(
        v,
        &after,
        [884.0, mid(582.0, 30.0)],
        30.0,
        INK,
        Align::Right,
    );
}

/// Jacket, difficulty tag, title, artist and lamp.
fn info(v: &mut View, c: &Canvas, s: &Summary) {
    c.panel(v, &slab(-200.0, 612.0, 545.0, 210.0), &[DARK]);
    v.fill(c.rect(102.0, 614.0, 208.0, 208.0), [0.0, 0.0, 0.0, c.alpha]);
    v.items.push(Item::Jacket {
        rect: c.rect(108.0, 620.0, 196.0, 196.0),
        key: s.jacket.clone(),
    });
    if let Some(d) = s.difficulty {
        let mut col = labels::difficulty_color(d);
        col[3] = 0.95;
        c.panel(v, &slab(323.0, 618.0, 125.0, 22.0), &[col]);
        c.text(
            v,
            labels::difficulty(d),
            [318.0, 620.0],
            17.0,
            WHITE,
            Align::Left,
        );
        if let Some(l) = s.level {
            c.text(
                v,
                &labels::level(l),
                [425.0, 615.0],
                24.0,
                WHITE,
                Align::Right,
            );
        }
    }
    // Title and artist end on the same right edge (E2 frames).
    c.panel(v, &slab(0.0, 822.0, 335.0, 24.0), &[WHITE]);
    c.fit(
        v,
        &s.title,
        [274.0, 825.0],
        18.0,
        TITLE,
        250.0,
        Align::Right,
    );
    c.panel(
        v,
        &slab(0.0, 848.0, 320.0, 24.0),
        &[[0.30, 0.29, 0.34, 0.95]],
    );
    let artist = [0.80, 0.80, 0.84, 1.0];
    c.fit(
        v,
        &s.artist,
        [276.0, 852.0],
        14.0,
        artist,
        240.0,
        Align::Right,
    );
    c.panel(v, &slab(0.0, 878.0, 300.0, 54.0), &[[0.0, 0.0, 0.0, 0.35]]);
    let (art, size, right) = lamp_art(s.result.lamp);
    let x = 254.0 - right * size[0] + size[0] / 2.0;
    c.image(v, font_tex(art), [x, 905.0], size, 1.0);
    if !s.completed {
        c.text(
            v,
            "GIVEN UP",
            [150.0, 936.0],
            18.0,
            [0.8, 0.8, 0.85, 1.0],
            Align::Center,
        );
    }
}

/// Column tab above the list (`R-Notes`, `Rate`).
fn tab(v: &mut View, c: &Canvas, label: &str, x: f32, w: f32) {
    c.panel(v, &slab(x, 616.0, w, 20.0), &[TAB]);
    c.text(
        v,
        label,
        [x + w / 2.0 - 6.0, 617.0],
        15.0,
        WHITE,
        Align::Center,
    );
}

/// Judgement rows with their R-note counts, FAST/LATE, COMBO and the score.
fn list(v: &mut View, c: &Canvas, s: &Summary, t: f32) {
    let r = &s.result;
    let n = &r.counts;
    let rn = &r.r_counts;
    let left = |y: f32| LIST_X - (y - LIST_TOP) * circle::SLANT;
    c.panel(
        v,
        &slab(left(LIST_TOP), LIST_TOP - 4.0, 900.0, 164.0),
        &[circle::NAVY],
    );
    tab(v, c, "R-Notes", 782.0, 74.0);
    tab(v, c, "Rate", 886.0, 92.0);
    let rows = [
        (n.marvelous, rn.marvelous),
        (n.great, rn.great),
        (n.good, rn.good),
        (n.miss, rn.miss),
    ];
    for (i, (count, r_count)) in rows.into_iter().enumerate() {
        let y = LIST_TOP + 40.0 * i as f32;
        let ym = y + 18.0;
        c.panel(
            v,
            &slab(left(y), y, 900.0, 36.0),
            &[PANEL, PANEL, PANEL_LOW, PANEL_LOW],
        );
        let (art, l) = JUDGE_ART[i];
        let (w, h) = (155.0, 39.0);
        c.image(v, font_tex(art), [468.0 - l * w + w / 2.0, ym], [w, h], 1.0);
        c.counter(
            v,
            &padded(count, 4),
            [645.0, mid(ym, 32.0)],
            32.0,
            27.0,
            INK,
        );
        let r_text = format!("({})", padded(r_count, 4));
        c.counter(v, &r_text, [783.0, mid(ym + 2.0, 18.0)], 18.0, 12.0, R_NOTE);
        c.counter(
            v,
            &rate(count, r.total_notes),
            [893.0, mid(ym + 2.0, 18.0)],
            18.0,
            13.7,
            INK,
        );
    }
    let y = 800.0;
    c.panel(v, &slab(left(y), y, 790.0 - left(y), 38.0), &[PANEL]);
    c.image(
        v,
        font_tex("uT_FTfastslow00"),
        [420.0, 819.0],
        [88.0, 22.0],
        1.0,
    );
    c.counter(
        v,
        &padded(n.fast, 4),
        [478.0, mid(819.0, 26.0)],
        26.0,
        18.0,
        INK,
    );
    c.image(
        v,
        font_tex("uT_FTfastslow01"),
        [605.0, 819.0],
        [88.0, 22.0],
        1.0,
    );
    c.counter(
        v,
        &padded(n.late, 4),
        [668.0, mid(819.0, 26.0)],
        26.0,
        18.0,
        INK,
    );
    let y = 843.0;
    c.panel(v, &slab(left(y), y, 720.0 - left(y), 37.0), &[PANEL]);
    c.image(
        v,
        font_tex("uT_FTcombo00"),
        [442.0, 862.0],
        [124.0, 31.0],
        1.0,
    );
    c.counter(
        v,
        &padded(r.max_combo, 4),
        [552.0, mid(862.0, 32.0)],
        32.0,
        27.0,
        INK,
    );

    c.panel(v, &slab(left(884.0) - 30.0, 884.0, 260.0, 101.0), &[DARK]);
    c.panel(v, &slab(330.0, 884.0, 900.0, 32.0), &[WHITE]);
    c.panel(v, &slab(560.0, 916.0, 900.0, 69.0), &[WHITE]);
    c.spaced(v, "SCORE", [352.0, 892.0], 17.0, 15.0, GREY_TEXT);
    if s.new_best {
        new_record(v, &c.moved([0.0, 0.0], ease(t, COUNT_AT + COUNT_LEN, 0.2)));
    }
    let score = padded(shown_score(r.score, t), 7);
    c.counter(v, &score, [590.0, mid(950.0, 66.0)], 66.0, 60.0, INK);
}

/// "New Record!!" from its letter textures, letters overlapping like the game's word.
fn new_record(v: &mut View, c: &Canvas) {
    let size = 40.0;
    let mut x = 445.0;
    for (i, (l, r)) in NEW_RECORD.into_iter().enumerate() {
        if i == 3 {
            x += 8.0;
        }
        let key = font_tex(&format!("uT_FTnewrecord_{i:02}"));
        c.image(
            v,
            key,
            [x - l * size + size / 2.0, 902.0],
            [size, size],
            1.0,
        );
        x += (r - l) * size * 0.66;
    }
}

/// Rank band with the grade letters stamped in.
fn rank(v: &mut View, c: &Canvas, g: Grade, t: f32) {
    let shown = ease(t, RANK_AT, 0.15);
    if shown <= 0.0 {
        return;
    }
    let c = c.moved([0.0, 0.0], shown);
    let [x, y, w, h] = BAND;
    let shape = slab(x, y, w, h);
    let left = x - h * circle::SLANT;
    match band(g) {
        Band::Pattern(key, base) => {
            c.panel(v, &shape, &[base]);
            // Tiles of the 2:1 texture as tall as the band (E2: triangles ≈ 21 canvas units).
            c.tiled(v, ui_tex(key), &shape, [left, y, 2.0 * h, h], false, 1.0);
        }
        Band::Crossed(top, bottom) => {
            c.panel(v, &shape, &[top, top, bottom, bottom]);
            // The "\" shapes plus their mirror image cross twice, 320 units apart (E2).
            // UNVERIFIED: the material's blend; drawn as a faint shade.
            let tile = [left + 74.0, y - 64.0, w, w / 2.0];
            for flip in [false, true] {
                c.tiled(v, ui_tex("uT_RTrateBBG00"), &shape, tile, flip, 0.10);
            }
        }
    }
    let stamp = ease(t, RANK_AT + 0.1, 0.4);
    let letters = rank_art(g);
    let (size, pitch) = if g == Grade::Master {
        (82.0, 72.0)
    } else {
        (131.0, 100.0)
    };
    let size = size * (1.0 + 0.6 * (1.0 - stamp));
    let x0 = 566.0 - pitch * (letters.len() as f32 - 1.0) / 2.0;
    for (i, key) in letters.into_iter().enumerate() {
        let x = x0 + pitch * i as f32;
        c.image(v, font_tex(key), [x, 1098.0], [size, size], stamp);
    }
}

/// Rating strip (rating, up-arrow and gain), RETRY tab and GO.
fn footer(v: &mut View, c: &Canvas, s: &Summary) {
    let grey = [0.84, 0.84, 0.86, 1.0];
    c.panel(
        v,
        &slab(150.0, 1240.0, 750.0, 50.0),
        &[grey, [0.92; 4], [0.75, 0.75, 0.77, 1.0], grey],
    );
    c.panel(v, &slab(545.0, 1240.0, 400.0, 50.0), &[WHITE]);
    c.spaced(
        v,
        "Rate:",
        [200.0, mid(1270.0, 16.0)],
        16.0,
        13.0,
        GREY_TEXT,
    );
    let after = Rating::shown(s.rating.after).to_string();
    let grey_ink = [0.38, 0.38, 0.42, 1.0];
    c.numerals(
        v,
        &after,
        [440.0, mid(1266.0, 42.0)],
        42.0,
        grey_ink,
        Align::Right,
    );
    if let Some(gain) = s.rating.gain() {
        c.image(
            v,
            ui_tex("uT_RTrateUP00"),
            [462.0, 1266.0],
            [40.0, 40.0],
            1.0,
        );
        c.panel(v, &slab(372.0, 1289.0, 76.0, 18.0), &[DARK]);
        let text = format!("+{gain}");
        c.numerals(
            v,
            &text,
            [418.0, mid(1298.0, 22.0)],
            22.0,
            GAIN,
            Align::Center,
        );
    }
    if let Some(why) = &s.not_saved {
        c.text(v, why, [800.0, mid(1265.0, 20.0)], 20.0, PINK, Align::Right);
    }
    circle::side_tab(v, c, "RETRY", 700.0, [0.50, 0.75, 1.0, 0.9], Action::Retry);
    circle::go_button(v, c, Action::Confirm);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_game::score::Counts;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 900.0,
        h: 1600.0,
    };

    fn summary(new_best: bool) -> Summary {
        Summary {
            title: "Song".into(),
            artist: "Artist".into(),
            jacket: "J".into(),
            difficulty: Some(Difficulty::Expert),
            level: Some(12.7),
            result: PlayResult {
                score: 950_000,
                grade: Grade::SS,
                lamp: Lamp::Clear,
                counts: Counts {
                    marvelous: 90,
                    great: 5,
                    good: 2,
                    miss: 3,
                    fast: 4,
                    late: 3,
                },
                r_counts: Counts {
                    marvelous: 7,
                    miss: 1,
                    ..Counts::default()
                },
                max_combo: 60,
                total_notes: 100,
                gauge: 0.85,
            },
            completed: true,
            new_best,
            not_saved: None,
            player: "PLAYER".into(),
            rating: Rating {
                before: 1203.9,
                after: 1210.2,
            },
        }
    }

    fn images(v: &View) -> Vec<&str> {
        v.items
            .iter()
            .filter_map(|i| match i {
                Item::Image { key, .. } | Item::ImagePoly { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Text with the single-character cells of [`Canvas::spaced`] joined.
    fn joined(v: &View) -> String {
        v.texts().concat()
    }

    #[test]
    fn rating_is_shown_in_whole_points() {
        assert_eq!(Rating::shown(0.8), 0);
        assert_eq!(Rating::shown(4.0), 4);
        assert_eq!(Rating::shown(-1.0), 0);
        let r = |before, after| Rating { before, after };
        assert_eq!(r(0.2, 4.0).gain(), Some(4));
        assert_eq!(r(0.2, 0.8).gain(), None);
        assert_eq!(r(5.0, 5.0).gain(), None);
    }

    #[test]
    fn counters_rates_and_count_up() {
        assert_eq!(padded(11, 4), "0011");
        assert_eq!(padded(135_862, 7), "0135862");
        assert_eq!(rate(11, 145), "007.6%");
        assert_eq!(rate(145, 145), "100.0%");
        assert_eq!(rate(0, 0), "000.0%");
        assert_eq!(shown_score(1_000_000, 0.0), 0);
        assert!(shown_score(1_000_000, COUNT_AT + COUNT_LEN / 2.0) > 0);
        assert_eq!(shown_score(1_000_000, 10.0), 1_000_000);
    }

    #[test]
    fn rank_art_spells_the_grade() {
        assert_eq!(rank_art(Grade::B), ["uT_FTrateBT00"]);
        assert_eq!(rank_art(Grade::SsPlus), ["uT_FTrateST00", "uT_FTrateST01"]);
        assert_eq!(rank_art(Grade::AAA).len(), 3);
        assert_eq!(rank_art(Grade::Master).len(), 6);
    }

    #[test]
    fn settled_screen_shows_counts_score_rank_and_record() {
        let v = view(&summary(true), AREA, 10.0);
        let text = joined(&v);
        for want in [
            "Song", "Artist", "PLAYER", "EXPERT", "12+", "0090", "0005", "0002", "0003", "090.0%",
        ] {
            assert!(text.contains(want), "{want} missing from {text}");
        }
        for want in ["0004", "0060", "0950000", "(0007)", "(0001)", "1210", "+7"] {
            assert!(text.contains(want), "{want} missing from {text}");
        }
        let img = images(&v);
        for want in [
            "UI/Textures/FONT/uT_FTdecision01",
            "UI/Textures/FONT/uT_FTclear_094",
            "UI/Textures/FONT/uT_FTrateST00",
            "UI/Textures/FONT/uT_FTnewrecord_00",
            "UI/Textures/FONT/uT_FTcombo00",
            "UI/Textures/uT_RTrateSBG00",
            "UI/Textures/uT_RTrateUP00",
        ] {
            assert!(img.contains(&want), "{want} missing from {img:?}");
        }
        assert_eq!(v.hit(c_point(&v, Action::Retry)), Some(Action::Retry));
        assert_eq!(v.hit(c_point(&v, Action::Confirm)), Some(Action::Confirm));
    }

    fn c_point(v: &View, a: Action) -> [f32; 2] {
        let (r, _) = v.buttons.iter().find(|(_, b)| *b == a).unwrap();
        [r.x + r.w / 2.0, r.y + r.h / 2.0]
    }

    #[test]
    fn counters_use_the_numeral_face() {
        use wacca_render::text::Face;
        let v = view(&summary(false), AREA, 10.0);
        let face = |want: &str| {
            v.items.iter().find_map(|i| match i {
                Item::Text { text, face, .. } if text == want => Some(*face),
                _ => None,
            })
        };
        assert_eq!(face("9"), Some(Face::Numerals));
        assert_eq!(face("Song"), Some(Face::Ui));
    }

    #[test]
    fn bands_follow_the_grade_family() {
        let keys = |g: Grade| {
            let mut s = summary(false);
            s.result.grade = g;
            images(&view(&s, AREA, 10.0))
                .into_iter()
                .filter(|k| k.contains("RTrate") && k.contains("BG"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert!(keys(Grade::Master).iter().all(|k| k.ends_with("MBG00")));
        assert!(!keys(Grade::Master).is_empty());
        assert!(keys(Grade::B).iter().all(|k| k.ends_with("BBG00")));
        assert!(!keys(Grade::A).is_empty());
    }

    #[test]
    fn animation_hides_rank_until_stamped_and_counts_score() {
        let s = summary(false);
        let early = view(&s, AREA, 0.0);
        assert!(!images(&early).iter().any(|k| k.contains("uT_FTrate")));
        assert!(!images(&early).iter().any(|k| k.contains("BG00")));
        assert!(joined(&early).contains("0000000"));
        assert!(!images(&early).iter().any(|k| k.contains("newrecord")));
        let late = view(&s, AREA, 10.0);
        assert!(images(&late).iter().any(|k| k.contains("uT_FTrateST00")));
    }

    #[test]
    fn give_up_not_saved_and_no_gain_are_shown() {
        let mut s = summary(false);
        s.completed = false;
        s.result.lamp = Lamp::Failed;
        s.not_saved = Some("AUTOPLAY: NOT SAVED".into());
        s.rating = Rating {
            before: 3.0,
            after: 3.0,
        };
        let v = view(&s, AREA, 10.0);
        let t = v.texts();
        assert!(t.contains(&"GIVEN UP") && t.contains(&"AUTOPLAY: NOT SAVED"));
        assert!(images(&v).contains(&"UI/Textures/FONT/uT_FTclear_095"));
        assert!(!images(&v).iter().any(|k| k.contains("RTrateUP")));
    }
}
