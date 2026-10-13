//! Category (genre folder) select, `BP_Select_page01`: a wheel of genre panels
//! (`Select_menu_OBJ13` in the middle, `Select_menu_OBJ14` around it), the difficulty band and
//! the folder's personal-best counts.

use wacca_chart::Difficulty;
use wacca_game::save::SaveData;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::canvas::{Canvas, MID, MIRROR, Painter, hex};
use crate::chrome::{self, INK, PINK, RimButtons, WHITE};
use crate::labels;
use crate::select::SongList;
use crate::ui::{Action, View};

/// A `ScoreGenre` folder: display name, panel art (`UI/Textures/GENRE`) and colours (`color1` of
/// its `uM_GR_b_*` panel material, `color2` for the header tab), sRGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Genre {
    pub id: i8,
    pub name: &'static str,
    pub art: &'static str,
    pub color: Rgba,
    pub tab: Rgba,
}

// UNVERIFIED: names and the genre → art mapping are read off the real category screen
// (ANIME / POP, VOCALOID, TOUHOU, 2.5D MUSICAL, VARIETY seen; ORIGINAL / HARDCORE TANO*C inferred).
pub const GENRES: [Genre; 7] = [
    genre(0, "ANIME / POP", "uT_GR_b_4", 0x6d9cda, 0x0069d4),
    genre(1, "VOCALOID", "uT_GR_b_5", 0x0197ab, 0xa5d8d1),
    genre(2, "TOUHOU", "uT_GR_b_6", 0xcf1948, 0xff002c),
    genre(3, "2.5D MUSICAL", "uT_GR_b_3", 0xff7400, 0xf68b00),
    genre(4, "VARIETY", "uT_GR_b_7", 0x85d454, 0x63c3a4),
    genre(5, "ORIGINAL", "uT_GR_b_8", 0x4f4f4f, 0x2a2a2a),
    genre(6, "HARDCORE TANO*C", "uT_GR_b_1", 0x313131, 0x111111),
];

const fn genre(id: i8, name: &'static str, art: &'static str, color: u32, tab: u32) -> Genre {
    Genre {
        id,
        name,
        art,
        color: hex(color, 1.0),
        tab: hex(tab, 1.0),
    }
}

/// Unknown genres get a neutral entry.
pub fn info(id: i8) -> Genre {
    GENRES.iter().find(|g| g.id == id).copied().unwrap_or(genre(
        id,
        "OTHER",
        "uT_GR_b_0",
        0x4f4f4f,
        0x2a2a2a,
    ))
}

/// Side panel slots (`BP_Select_page01` `panel`): offset from the cursor → position, scale.
const SLOTS: [(f32, [f32; 2], f32); 5] = [
    (-2.0, [-430.0, -160.0], 0.8),
    (-1.0, [-320.0, 0.0], 1.0),
    (0.0, [0.0, 0.0], 1.0),
    (1.0, [320.0, 0.0], 1.0),
    (2.0, [430.0, -160.0], 0.8),
];

/// Position and scale of a panel `k` places from the cursor (fractional while scrolling).
pub fn slot(k: f32) -> Option<([f32; 2], f32)> {
    let k = k.clamp(-2.0, 2.0);
    SLOTS.windows(2).find_map(|w| {
        let ((k0, p0, s0), (k1, p1, s1)) = (w[0], w[1]);
        (k0..=k1).contains(&k).then(|| {
            let t = k - k0;
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            ([lerp(p0[0], p1[0]), lerp(p0[1], p1[1])], lerp(s0, s1))
        })
    })
}

/// Genre art over its coloured panel, as in `Select_menu_OBJ14` (scaled by `s` around `at`).
fn small_panel(p: &mut Painter, g: &Genre, at: [f32; 2], s: f32) {
    let o = |dy: f32| [at[0], at[1] + dy * s];
    p.tex("uT_STmenu_obj053", at, [259.0 * s, 221.0 * s], INK);
    // Measured on frames: the art sits lower than the widget offsets suggest.
    p.tex(
        "uT_STmenu_obj052",
        o(-13.0),
        [209.0 * s, 157.0 * s],
        g.color,
    );
    p.tex(
        &format!("GENRE/{}", g.art),
        o(-49.5),
        [255.0 * s, 255.0 * s],
        WHITE,
    );
    p.text(g.name, o(77.0), 15.0 * s, WHITE, Align::Center);
}

/// `Select_menu_OBJ13` at (0, -25): the selected genre, large and lit.
fn centre_panel(p: &mut Painter, g: &Genre, alpha: f32) {
    let a = |c: Rgba| [c[0], c[1], c[2], c[3] * alpha];
    p.tex(
        "uT_STmenu_obj050",
        [0.0, -25.0],
        [564.0, 444.0],
        a(hex(0xffe7ff, 1.0)),
    );
    p.tex("uT_STmenu_obj051", [0.0, -51.0], [418.0, 318.0], a(INK));
    p.tex("uT_STmenu_obj052", [0.0, -53.0], [400.0, 300.0], a(g.color));
    p.tex(
        &format!("GENRE/{}", g.art),
        [0.0, -147.0],
        [510.0, 510.0],
        a(WHITE),
    );
    p.text(g.name, [0.0, 143.0], 30.0, a(INK), Align::Center);
}

/// Difficulty band across the circle with the difficulty name (`listALL` `t_difficulty`).
pub fn difficulty_band(p: &mut Painter, d: Difficulty, y: f32) {
    let [flag, _] = labels::difficulty_flags(d);
    p.fill([0.0, y], [1080.0, 44.0], MID, flag);
    p.text(labels::difficulty(d), [0.0, y], 24.0, WHITE, Align::Center);
}

/// Lamp words (`FONT/uT_FTclear_*`): CLEAR, Missless!, Full Combo!.
const LAMP_ART: [&str; 3] = ["uT_FTclear_094", "uT_FTclear_093", "uT_FTclear_091"];
const GRADE_NAMES: [&str; 6] = ["S", "SS", "SSS", "S+", "SS+", "SSS+"];

/// The folder's best counts under the band (`listALL` `list10`..`list18`).
fn stats(p: &mut Painter, list: &SongList, genre: i8, d: Difficulty, save: &SaveData) {
    let st = list.stats(genre, d, save);
    p.fill([0.0, 312.0], [1200.0, 160.0], MID, hex(0x0b0b1d, 0.4));
    p.fill([0.0, 312.0], [630.0, 152.0], MID, INK);
    p.tex_ex(
        "uT_STmenu_obj055",
        [-315.0, 312.0],
        [160.0, 160.0],
        [1.0, 0.5],
        crate::canvas::FULL,
        0.0,
        INK,
    );
    p.tex_ex(
        "uT_STmenu_obj055",
        [315.0, 312.0],
        [160.0, 160.0],
        [0.0, 0.5],
        MIRROR,
        0.0,
        INK,
    );
    let count = |n: usize| format!("{n}/{}", st.total);
    for (row, art) in LAMP_ART.iter().enumerate() {
        let y = 267.0 + 44.0 * row as f32;
        p.tex_ex(
            &format!("FONT/{art}"),
            [-208.0, y],
            [100.0, 25.0],
            [1.0, 0.5],
            crate::canvas::FULL,
            0.0,
            WHITE,
        );
        p.text(count(st.lamps[row]), [-78.0, y], 13.0, WHITE, Align::Right);
        for (col, x) in [(0, -28.0), (1, 152.0)] {
            let g = row + 3 * col;
            p.tex("uT_CMen64_00", [x, y], [38.0, 38.0], PINK);
            p.text(GRADE_NAMES[g], [x, y], 9.0, INK, Align::Center);
            p.text(
                count(st.grades[g]),
                [x + 130.0, y],
                13.0,
                WHITE,
                Align::Right,
            );
        }
    }
}

/// The category screen. `cursor` indexes [`SongList::genres`]; `shift` is the wheel's visual
/// offset in panels (eases to 0 after a move).
pub fn view(c: Canvas, list: &SongList, cursor: usize, shift: f32, save: &SaveData) -> View {
    let mut p = Painter::new(c);
    chrome::background(&mut p);
    let genres = list.genres();
    let n = genres.len() as i64;
    if n > 0 {
        let at = |k: i64| info(genres[(cursor as i64 + k).rem_euclid(n) as usize]);
        // Far panels first so nearer ones overlap them.
        for k in [-2, 2, -1, 1, 0] {
            let Some((pos, s)) = slot(k as f32 + shift) else {
                continue;
            };
            if k == 0 && shift.abs() < 0.5 {
                continue;
            }
            small_panel(&mut p, &at(k), pos, s);
            if k != 0 {
                p.button(pos, [259.0 * s, 221.0 * s], Action::Scroll(k as i32));
            }
        }
        let current = at(0);
        centre_panel(&mut p, &current, (1.0 - 2.0 * shift.abs()).clamp(0.0, 1.0));
        p.button([0.0, -25.0], [418.0, 400.0], Action::Confirm);
        let d = list.wanted();
        difficulty_band(&mut p, d, 210.0);
        stats(&mut p, list, current.id, d, save);
    }
    chrome::user_plate(&mut p, "GUEST");
    chrome::rim(
        &mut p,
        RimButtons {
            back: false,
            sort: Some(list.sort().label()),
        },
    );
    p.circle_mask();
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::tests::list;

    #[test]
    fn slots_interpolate_between_panels() {
        assert_eq!(slot(0.0), Some(([0.0, 0.0], 1.0)));
        assert_eq!(slot(-2.0), Some(([-430.0, -160.0], 0.8)));
        let (p, s) = slot(1.5).unwrap();
        assert_eq!((p, s), ([375.0, -80.0], 0.9));
        assert_eq!(slot(5.0), slot(2.0));
    }

    #[test]
    fn genre_table() {
        assert_eq!(info(2).name, "TOUHOU");
        assert_eq!(info(-1).name, "OTHER");
        assert!(GENRES.iter().enumerate().all(|(i, g)| g.id as usize == i));
    }

    #[test]
    fn view_shows_the_genre_its_stats_and_tappable_neighbours() {
        let mut l = list(); // genres 0 and 2
        l.set_difficulty(Difficulty::Expert);
        let c = Canvas::fit(1080.0, 1080.0);
        let v = view(c, &l, 1, 0.0, &SaveData::default());
        let texts = v.texts();
        assert!(texts.contains(&"TOUHOU") && texts.contains(&"ANIME / POP"));
        assert!(texts.contains(&"EXPERT") && texts.contains(&"0/1"));
        assert_eq!(v.hit(c.pt([0.0, -25.0])), Some(Action::Confirm));
        assert_eq!(v.hit(c.pt([320.0, 0.0])), Some(Action::Scroll(1)));
        assert_eq!(v.hit(c.pt([-430.0, -160.0])), Some(Action::Scroll(-2)));
    }
}
