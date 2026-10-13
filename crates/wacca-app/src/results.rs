//! Results screen (`docs/research/05-save-and-song-select.md` §4), laid out like the game's
//! `BP_Result_score` on the round screen: jacket block on the left, judgement / FAST-LATE / COMBO
//! panels and the score on the right, the rank band below, GO at the bottom. Positions, colours
//! and timings are measured from the E2 recordings (`runs/e2/*/video.mkv`, end of the song).

use wacca_chart::Difficulty;
use wacca_game::grade::{Grade, Lamp, PlayResult};
use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::circle::{self, Canvas, INK, PINK, ease, font_tex, slab};
use crate::hud::format_score;
use crate::labels;
use crate::ui::{Action, Item, View, WHITE};

/// Everything the results screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub title: String,
    pub jacket: String,
    pub difficulty: Option<Difficulty>,
    pub level: Option<f32>,
    pub result: PlayResult,
    /// Played to the end (not given up).
    pub completed: bool,
    /// Best score before this play, if the chart was played before.
    pub previous_best: Option<u32>,
    pub new_best: bool,
    /// Why the play was not saved (autoplay, unknown song, write error).
    pub not_saved: Option<String>,
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
const GREY_TEXT: Rgba = [0.45, 0.44, 0.48, 1.0];
/// Left boundary of the right-hand panels: x at `y = 638`, leaning with [`circle::SLANT`].
const LIST_X: f32 = 470.0;
const LIST_TOP: f32 = 638.0;

/// `+12,345` / `-500` against the previous best.
pub fn best_diff(score: u32, previous: Option<u32>) -> Option<String> {
    let p = previous?;
    Some(if score >= p {
        format!("+{}", format_score(score - p))
    } else {
        format!("-{}", format_score(p - score))
    })
}

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

/// Rank band colours (top, bottom) per grade family.
// UNVERIFIED: only B (cyan) and MASTER (rainbow) were seen in E2; the others follow the letters.
fn band_colors(g: Grade) -> [Rgba; 4] {
    let pair = |t: Rgba, b: Rgba| [t, t, b, b];
    match g {
        Grade::D => pair([0.62, 0.63, 0.66, 1.0], [0.38, 0.39, 0.43, 1.0]),
        Grade::C => pair([0.30, 0.55, 1.0, 1.0], [0.10, 0.30, 0.85, 1.0]),
        Grade::B => pair([0.21, 0.84, 0.93, 1.0], [0.08, 0.55, 0.80, 1.0]),
        Grade::A | Grade::AA | Grade::AAA => pair([1.0, 0.88, 0.35, 1.0], [0.95, 0.60, 0.10, 1.0]),
        Grade::Master => [
            [1.0, 0.45, 0.80, 1.0],
            [0.45, 0.85, 1.0, 1.0],
            [1.0, 0.85, 0.30, 1.0],
            [1.0, 0.35, 0.55, 1.0],
        ],
        _ => pair([1.0, 0.55, 0.85, 1.0], [0.92, 0.18, 0.62, 1.0]),
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

/// Results for `s`, `t` seconds after the screen appeared.
pub fn view(s: &Summary, area: Rect, t: f32) -> View {
    let mut v = View::default();
    let c = Canvas::new(area);
    circle::background(&mut v, &c);
    let e = ease(t, INFO_AT, SLIDE);
    info(&mut v, &c.moved([700.0 * (1.0 - e), 0.0], e), s);
    let e = ease(t, LIST_AT, SLIDE);
    let d = 420.0 * (1.0 - e);
    list(&mut v, &c.moved([0.6 * d, 0.8 * d], e), s, t);
    rank(&mut v, &c, s.result.grade, t);
    footer(&mut v, &c.moved([0.0, 0.0], ease(t, FOOTER_AT, 0.3)), s);
    v
}

/// Jacket, difficulty tag, title and lamp.
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
    c.panel(v, &slab(0.0, 822.0, 335.0, 24.0), &[WHITE]);
    c.text_fit(
        v,
        &s.title,
        [215.0, 824.0],
        20.0,
        [0.95, 0.10, 0.55, 1.0],
        250.0,
    );
    c.panel(
        v,
        &slab(0.0, 848.0, 320.0, 24.0),
        &[[0.30, 0.29, 0.34, 0.95]],
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

/// Judgement rows, FAST/LATE, COMBO and the score.
fn list(v: &mut View, c: &Canvas, s: &Summary, t: f32) {
    let r = &s.result;
    let n = &r.counts;
    let left = |y: f32| LIST_X - (y - LIST_TOP) * circle::SLANT;
    c.panel(
        v,
        &slab(left(LIST_TOP), LIST_TOP - 4.0, 900.0, 164.0),
        &[circle::NAVY],
    );
    let tab = [0.42, 0.42, 0.47, 1.0];
    c.panel(v, &slab(890.0, 616.0, 92.0, 20.0), &[tab]);
    c.text(v, "Rate", [930.0, 617.0], 15.0, WHITE, Align::Center);
    for (i, count) in [n.marvelous, n.great, n.good, n.miss]
        .into_iter()
        .enumerate()
    {
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
        c.spaced(
            v,
            &padded(count, 4),
            [645.0, mid(ym, 32.0)],
            32.0,
            27.0,
            INK,
        );
        c.spaced(
            v,
            &rate(count, r.total_notes),
            [893.0, mid(ym, 18.0)],
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
    c.spaced(
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
    c.spaced(
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
    c.spaced(
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
    c.spaced(v, &score, [590.0, mid(950.0, 66.0)], 66.0, 60.0, INK);
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
    let band = ease(t, RANK_AT, 0.15);
    if band <= 0.0 {
        return;
    }
    let mut colors = band_colors(g);
    for col in &mut colors {
        col[3] *= band;
    }
    c.panel(v, &slab(255.0, 985.0, 745.0, 255.0), &colors);
    let shine = [1.0, 1.0, 1.0, 0.18 * band];
    c.panel(v, &slab(255.0, 985.0, 745.0, 60.0), &[shine]);
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

/// Best-score strip, RETRY tab and GO.
fn footer(v: &mut View, c: &Canvas, s: &Summary) {
    let r = &s.result;
    c.panel(
        v,
        &slab(150.0, 1240.0, 750.0, 50.0),
        &[
            [0.92, 0.92, 0.92, 1.0],
            [0.92; 4],
            [0.75, 0.75, 0.77, 1.0],
            [0.75, 0.75, 0.77, 1.0],
        ],
    );
    c.spaced(v, "BEST", [160.0, 1256.0], 17.0, 15.0, GREY_TEXT);
    let best = s.previous_best.map_or("-".to_string(), format_score);
    c.text(
        v,
        &best,
        [470.0, mid(1265.0, 34.0)],
        34.0,
        INK,
        Align::Right,
    );
    let right = match (&s.not_saved, best_diff(r.score, s.previous_best)) {
        (Some(why), _) => Some((why.clone(), 20.0, PINK)),
        (None, Some(d)) => Some((d, 34.0, if s.new_best { PINK } else { INK })),
        (None, None) => None,
    };
    if let Some((text, px, col)) = right {
        c.text(v, &text, [800.0, mid(1265.0, px)], px, col, Align::Right);
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

    fn summary(new_best: bool, previous_best: Option<u32>) -> Summary {
        Summary {
            title: "Song".into(),
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
                max_combo: 60,
                total_notes: 100,
                gauge: 0.85,
            },
            completed: true,
            previous_best,
            new_best,
            not_saved: None,
        }
    }

    fn images(v: &View) -> Vec<&str> {
        v.items
            .iter()
            .filter_map(|i| match i {
                Item::Image { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Text with the single-character cells of [`Canvas::spaced`] joined.
    fn joined(v: &View) -> String {
        v.texts().concat()
    }

    #[test]
    fn diff_against_previous_best() {
        assert_eq!(best_diff(950_000, Some(900_000)).unwrap(), "+50,000");
        assert_eq!(best_diff(900_000, Some(950_000)).unwrap(), "-50,000");
        assert_eq!(best_diff(1, None), None);
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
        let v = view(&summary(true, Some(940_000)), AREA, 10.0);
        let text = joined(&v);
        for want in [
            "Song", "EXPERT", "12+", "0090", "0005", "0002", "0003", "090.0%",
        ] {
            assert!(text.contains(want), "{want} missing from {text}");
        }
        for want in ["0004", "0060", "0950000", "940,000", "+10,000"] {
            assert!(text.contains(want), "{want} missing from {text}");
        }
        let img = images(&v);
        for want in [
            "UI/Textures/FONT/uT_FTdecision01",
            "UI/Textures/FONT/uT_FTclear_094",
            "UI/Textures/FONT/uT_FTrateST00",
            "UI/Textures/FONT/uT_FTnewrecord_00",
            "UI/Textures/FONT/uT_FTcombo00",
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
    fn animation_hides_rank_until_stamped_and_counts_score() {
        let s = summary(false, Some(990_000));
        let early = view(&s, AREA, 0.0);
        assert!(!images(&early).iter().any(|k| k.contains("uT_FTrate")));
        assert!(joined(&early).contains("0000000"));
        assert!(!images(&early).iter().any(|k| k.contains("newrecord")));
        let late = view(&s, AREA, 10.0);
        assert!(images(&late).iter().any(|k| k.contains("uT_FTrateST00")));
    }

    #[test]
    fn give_up_and_not_saved_are_shown() {
        let mut s = summary(false, Some(990_000));
        s.completed = false;
        s.result.lamp = Lamp::Failed;
        s.not_saved = Some("AUTOPLAY: NOT SAVED".into());
        let v = view(&s, AREA, 10.0);
        let t = v.texts();
        assert!(t.contains(&"GIVEN UP") && t.contains(&"AUTOPLAY: NOT SAVED"));
        assert!(t.contains(&"990,000"));
        assert!(images(&v).contains(&"UI/Textures/FONT/uT_FTclear_095"));
    }
}
