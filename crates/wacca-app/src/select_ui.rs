//! Song select, `BP_Select_page02`: a horizontal wheel of song panels (`Select_menu_OBJ09`)
//! around the selected song (`Select_menu_OBJ08`), the jacket strip above (`Select_menu_OBJ11`),
//! genre tabs (`Select_menu_OBJ12`) and the difficulty plate with level, best score and song
//! info at the bottom (`Select_menu_OBJ07`). Difficulty is chosen here with the DIFFICULTY
//! button, as in the original (there is no separate difficulty screen).

use wacca_game::catalog::Song;
use wacca_game::save::SaveData;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::canvas::{Canvas, FULL, MID, MIRROR, Painter, hex};
use crate::category::{self, Genre};
use crate::chrome::{self, INK, Icon, PINK, RimButtons, WHITE};
use crate::hud::format_score;
use crate::labels;
use crate::select::{SongList, effective};
use crate::ui::{Action, View};

/// Tint of the selected song's frame (`Select_menu_OBJ08`, measured on the E3 frames).
const HIGHLIGHT: Rgba = hex(0xffe7ff, 1.0);
const VIOLET: Rgba = hex(0x8300a0, 1.0);
const PANEL_GREY: Rgba = hex(0x4d4d4d, 1.0);

/// `musicpanel` (0, 60) + `OBJ09_*`: offset from the cursor → position, brightness.
const PANELS: [(f32, [f32; 2], f32); 7] = [
    (-3.0, [-600.0, 130.0], 0.7),
    (-2.0, [-426.0, 92.0], 0.8),
    (-1.0, [-255.0, 60.0], 1.0),
    (0.0, [0.0, 60.0], 1.0),
    (1.0, [245.0, 60.0], 1.0),
    (2.0, [435.0, 85.0], 0.8),
    (3.0, [620.0, 150.0], 0.7),
];

/// `jaketPanel` (0, -155) + `Select_menu_OBJ11_*`: offset → position, scale.
const MINIS: [(f32, [f32; 2], f32); 15] = [
    (-7.0, [-520.0, -129.0], 0.65),
    (-6.0, [-450.0, -137.0], 0.65),
    (-5.0, [-380.0, -145.0], 0.65),
    (-4.0, [-310.0, -153.0], 0.65),
    (-3.0, [-240.0, -161.0], 0.65),
    (-2.0, [-170.0, -169.0], 0.65),
    (-1.0, [-100.0, -177.0], 0.65),
    (0.0, [0.0, -155.0], 1.0),
    (1.0, [100.0, -177.0], 0.65),
    (2.0, [170.0, -169.0], 0.65),
    (3.0, [240.0, -161.0], 0.65),
    (4.0, [310.0, -153.0], 0.65),
    (5.0, [380.0, -145.0], 0.65),
    (6.0, [450.0, -137.0], 0.65),
    (7.0, [580.0, -115.0], 0.65),
];

/// Interpolated `(position, scale-or-brightness)` at fractional offset `k` in `table`.
pub fn lerp_slot(table: &[(f32, [f32; 2], f32)], k: f32) -> Option<([f32; 2], f32)> {
    table.windows(2).find_map(|w| {
        let ((k0, p0, s0), (k1, p1, s1)) = (w[0], w[1]);
        (k0..=k1).contains(&k).then(|| {
            let t = k - k0;
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            ([lerp(p0[0], p1[0]), lerp(p0[1], p1[1])], lerp(s0, s1))
        })
    })
}

/// Shortest signed step from cursor `from` to `to` on a wheel of `len` entries.
pub fn wrap_delta(from: usize, to: usize, len: usize) -> i32 {
    if len == 0 {
        return 0;
    }
    let len = len as i64;
    let d = (to as i64 - from as i64).rem_euclid(len);
    (if d > len / 2 { d - len } else { d }) as i32
}

fn dim(c: Rgba, k: f32) -> Rgba {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// `Select_menu_OBJ09`: a song panel (grey plate, framed jacket, level, best, title strip).
fn song_panel(
    p: &mut Painter,
    song: &Song,
    list: &SongList,
    save: &SaveData,
    at: [f32; 2],
    k: f32,
) {
    let o = |dx: f32, dy: f32| [at[0] + dx, at[1] + dy];
    p.tex("uT_STmenu_obj042", at, [240.0, 300.0], dim(PANEL_GREY, k));
    p.fill(o(0.0, 144.0), [226.0, 50.0], [0.5, 1.0], INK);
    p.fill(o(0.0, -30.0), [198.0, 198.0], MID, INK);
    p.fill(o(0.0, -30.0), [194.0, 194.0], MID, dim(WHITE, k));
    p.jacket(&song.jacket, o(0.0, -30.0), 190.0);
    if k < 1.0 {
        p.fill(o(0.0, -30.0), [190.0, 190.0], MID, hex(0, 1.0 - k));
    }
    let max = Some(193.0);
    p.text_fit(&song.title, o(0.0, 108.0), 11.0, WHITE, Align::Center, max);
    p.fill(o(0.0, 120.0), [198.0, 1.0], MID, WHITE);
    p.text_fit(&song.artist, o(0.0, 132.0), 8.0, PINK, Align::Center, max);
    if let Some(c) = effective(song, list.wanted()) {
        p.tex("uT_STmenu_obj008", o(-85.0, 79.0), [28.0, 28.0], INK);
        p.text(
            labels::level(c.level),
            o(-85.0, 84.0),
            8.0,
            WHITE,
            Align::Center,
        );
        let best = save
            .record(song.id, c.difficulty)
            .filter(|r| r.play_count > 0)
            .map_or(0, |r| r.best_score);
        p.text(format_score(best), o(31.0, 83.0), 8.0, WHITE, Align::Center);
    }
}

/// `Select_menu_OBJ11`: small jacket framed in its genre colour.
fn mini(p: &mut Painter, song: &Song, at: [f32; 2], s: f32) {
    p.fill(
        at,
        [105.0 * s, 105.0 * s],
        MID,
        category::info(song.genre).color,
    );
    p.fill(at, [94.0 * s, 94.0 * s], MID, INK);
    p.jacket(&song.jacket, at, 90.0 * s);
}

/// Connector lines between the jacket strip and the panels (`lineALL`), measured on frames.
fn connectors(p: &mut Painter) {
    for side in [-1.0, 1.0] {
        let x = |v: f32| v * side;
        for (dot, corner, end) in [
            ([-170.0, -117.0], [-424.0, -73.0], [-424.0, -45.0]),
            ([-101.0, -117.0], [-368.0, -78.0], [-368.0, -60.0]),
        ] {
            let (d, c, e) = (
                [x(dot[0]), dot[1]],
                [x(corner[0]), corner[1]],
                [x(end[0]), end[1]],
            );
            p.line(d, c, 5.0, INK);
            p.line(c, e, 5.0, INK);
            p.line(d, [d[0], -140.0], 5.0, INK);
            p.tex("uT_CMen16_00", d, [16.0, 16.0], INK);
        }
    }
    p.line([0.0, -105.0], [0.0, -71.0], 5.0, INK);
    p.tex("uT_CMen16_00", [0.0, -88.0], [16.0, 16.0], INK);
}

/// `Select_menu_OBJ12`: previous / current / next genre tabs.
fn genre_tabs(p: &mut Painter, list: &SongList) {
    let genres = list.genres();
    let Some(g) = list.genre() else { return };
    let Some(i) = genres.iter().position(|&x| x == g) else {
        return;
    };
    let n = genres.len() as i64;
    let at = |k: i64| category::info(genres[(i as i64 + k).rem_euclid(n) as usize]);
    let tab = |p: &mut Painter, g: &Genre, x: f32, s: f32| {
        p.fill([x, -248.0], [136.0 * s, 32.0 * s], MID, g.tab);
        p.text_outlined(g.name, [x, -248.0], 13.0 * s, WHITE, INK, 1.5 * s);
    };
    tab(p, &at(-1), -120.0, 0.6);
    tab(p, &at(1), 120.0, 0.6);
    tab(p, &at(0), 0.0, 1.0);
    p.tex("uT_STmenu_obj045", [-178.0, -248.0], [20.0, 20.0], PINK);
    p.tex_ex(
        "uT_STmenu_obj045",
        [178.0, -248.0],
        [20.0, 20.0],
        MID,
        MIRROR,
        0.0,
        PINK,
    );
    p.button([-150.0, -248.0], [90.0, 40.0], Action::Genre(-1));
    p.button([150.0, -248.0], [90.0, 40.0], Action::Genre(1));
}

/// Song number in the folder, top right (`VB_music` counter).
fn counter(p: &mut Painter, list: &SongList) {
    p.fill([350.0, -250.0], [275.0, 66.0], MID, INK);
    let cur = (list.cursor() + 1).to_string();
    p.text(cur, [300.0, -252.0], 50.0, VIOLET, Align::Right);
    p.text(
        format!("/{}", list.len()),
        [304.0, -242.0],
        30.0,
        VIOLET,
        Align::Left,
    );
}

/// `Select_menu_OBJ07` at (0, 190): difficulty plate, level, best score, song info.
pub fn info_plate(p: &mut Painter, list: &SongList, save: &SaveData) {
    let Some((song, chart)) = list.chart() else {
        return;
    };
    let d = chart.difficulty;
    let [flag, font] = labels::difficulty_flags(d);
    p.mirrored("uT_STmenu_obj038", [0.0, 190.0], [600.0, 400.0], 0.0, flag);
    // `clip_shadow`: the plate darkens below the info bar.
    p.fill([0.0, 315.0], [1080.0, 280.0], [0.5, 0.0], hex(0, 0.25));

    // `LV`: triangle with the level, "Lv." and the difficulty name.
    p.tex("uT_STmenu_obj008", [0.0, 260.0], [90.0, 90.0], INK);
    let lv = labels::level(chart.level);
    let x = if lv.ends_with('+') { 8.0 } else { 0.0 };
    p.text_outlined(&lv, [x, 280.0], 32.0, font, INK, 2.0);
    p.text("Lv.", [-75.0, 291.0], 25.0, INK, Align::Center);
    p.text(
        labels::difficulty(d),
        [-70.0, 267.0],
        7.0,
        INK,
        Align::Center,
    );

    // `info_score`.
    let best = save
        .record(song.id, d)
        .filter(|r| r.play_count > 0)
        .map_or(0, |r| r.best_score);
    p.text("S C O R E", [248.0, 275.0], 10.0, WHITE, Align::Center);
    p.text(
        format_score(best),
        [248.0, 299.0],
        15.0,
        WHITE,
        Align::Center,
    );

    // `info_music` at (0, 350): title bar with crescent caps, BPM pill.
    p.fill([0.0, 350.0], [1080.0, 70.0], MID, hex(0x0b0b1d, 0.3));
    p.tex_ex(
        "uT_STmenu_obj055",
        [-279.0, 350.0],
        [70.0, 70.0],
        [1.0, 0.5],
        FULL,
        0.0,
        INK,
    );
    p.tex_ex(
        "uT_STmenu_obj055",
        [279.0, 350.0],
        [70.0, 70.0],
        [0.0, 0.5],
        MIRROR,
        0.0,
        INK,
    );
    p.fill([0.0, 350.0], [560.0, 70.0], MID, INK);
    let max = Some(410.0);
    p.text_fit(&song.title, [-47.0, 333.0], 13.0, WHITE, Align::Center, max);
    p.text_fit(&song.artist, [-47.0, 364.0], 10.0, PINK, Align::Center, max);
    p.tex(
        "uT_CMen64_00",
        [195.0, 350.0],
        [26.0, 26.0],
        hex(0x696969, 1.0),
    );
    p.tex(
        "uT_CMen64_00",
        [272.0, 350.0],
        [26.0, 26.0],
        hex(0x696969, 1.0),
    );
    p.fill([234.0, 350.0], [77.0, 26.0], MID, hex(0x696969, 1.0));
    p.fill([234.0, 350.0], [121.0, 22.0], MID, INK);
    p.text("BPM", [196.0, 351.0], 6.0, PINK, Align::Center);
    p.text(&song.bpm, [251.0, 351.0], 9.0, WHITE, Align::Center);
}

/// The song select screen. `shift` is the wheel's visual offset in songs (eases to 0).
pub fn view(c: Canvas, list: &SongList, shift: f32, save: &SaveData, who: &chrome::Player) -> View {
    let mut p = Painter::new(c);
    chrome::background(&mut p, chrome::SELECT_BACKDROP);
    // `plate01` (dark) and `plate00` (grey zigzag behind the jacket strip), the selected song's
    // frame, then the purple band and the dark strip holding the genre tabs over the frame's
    // top (E3 frames: purple -305..-269, dark -268..-213).
    p.mirrored("uT_STmenu_obj015", [0.0, -295.0], [656.0, 316.0], 0.5, INK);
    p.mirrored(
        "uT_STmenu_obj015",
        [0.0, -216.0],
        [547.0, 351.0],
        0.5,
        WHITE,
    );
    if list.len() > 0 {
        // `Select_menu_OBJ08`: the selected song.
        p.mirrored(
            "uT_STmenu_obj041",
            [0.0, 22.0],
            [196.0, 604.0],
            0.5,
            HIGHLIGHT,
        );
    }
    p.fill([0.0, -287.0], [1080.0, 37.0], MID, hex(0x7a0cff, 1.0));
    p.fill([0.0, -240.0], [1080.0, 54.0], MID, INK);
    connectors(&mut p);

    if list.len() > 0 {
        for k in [-3, 3, -2, 2, -1, 1, 0] {
            let off = k as f32 + shift;
            let Some((at, bright)) = lerp_slot(&PANELS, off) else {
                continue;
            };
            if k == 0 && shift.abs() < 0.5 {
                continue;
            }
            if let Some(song) = list.around(k) {
                song_panel(&mut p, song, list, save, at, bright);
                if k != 0 {
                    p.button([at[0], at[1] - 30.0], [230.0, 280.0], Action::Scroll(k));
                }
            }
        }
        p.tex("uT_STmenu_obj045", [-158.0, 53.0], [25.0, 32.0], INK);
        p.tex_ex(
            "uT_STmenu_obj045",
            [160.0, 53.0],
            [26.0, 32.0],
            MID,
            MIRROR,
            0.0,
            INK,
        );
        if let Some(song) = list.current() {
            let a = (1.0 - 2.0 * shift.abs()).clamp(0.0, 1.0);
            p.fill([0.0, 60.0], [264.0, 264.0], MID, INK);
            p.fill(
                [0.0, 60.0],
                [254.0, 254.0],
                MID,
                category::info(song.genre).color,
            );
            p.jacket(&song.jacket, [0.0, 60.0], 244.0);
            if a < 1.0 {
                p.fill(
                    [0.0, 60.0],
                    [244.0, 244.0],
                    MID,
                    [HIGHLIGHT[0], HIGHLIGHT[1], HIGHLIGHT[2], 1.0 - a],
                );
            }
            p.button([0.0, 60.0], [264.0, 264.0], Action::Confirm);
        }
        for k in -7..=7 {
            if let (Some((at, s)), Some(song)) =
                (lerp_slot(&MINIS, k as f32 + shift), list.around(k))
            {
                mini(&mut p, song, at, s);
                if k != 0 {
                    p.button(at, [68.0, 68.0], Action::Scroll(k));
                }
            }
        }
        info_plate(&mut p, list, save);
    }
    genre_tabs(&mut p, list);
    counter(&mut p, list);
    chrome::icon(&mut p, Icon::Left, [-476.0, -117.0], 30.0, 0.0);
    p.button([-476.0, -117.0], [60.0, 60.0], Action::Genre(-1));
    chrome::icon(&mut p, Icon::Right, [474.0, -118.0], 30.0, 0.0);
    p.button([474.0, -118.0], [60.0, 60.0], Action::Genre(1));
    chrome::user_plate(&mut p, who);
    chrome::rim(
        &mut p,
        RimButtons {
            back: true,
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
    fn wheel_steps_take_the_short_way_round() {
        assert_eq!(wrap_delta(0, 1, 10), 1);
        assert_eq!(wrap_delta(0, 9, 10), -1);
        assert_eq!(wrap_delta(8, 1, 10), 3);
        assert_eq!(wrap_delta(3, 3, 10), 0);
        assert_eq!(wrap_delta(0, 0, 0), 0);
    }

    #[test]
    fn slot_tables_interpolate() {
        assert_eq!(lerp_slot(&PANELS, 0.0), Some(([0.0, 60.0], 1.0)));
        assert_eq!(lerp_slot(&PANELS, 0.5), Some(([122.5, 60.0], 1.0)));
        assert_eq!(lerp_slot(&PANELS, 3.5), None);
        assert_eq!(lerp_slot(&MINIS, 0.0), Some(([0.0, -155.0], 1.0)));
    }

    #[test]
    fn view_shows_the_selected_chart_and_neighbours() {
        let mut l = list(); // all songs; cursor on song 2 (Hard/Expert 13.7/Inferno)
        l.set_genre(Some(0));
        l.set_difficulty(wacca_chart::Difficulty::Expert);
        let c = Canvas::fit(1080.0, 1080.0);
        let v = view(c, &l, 0.0, &SaveData::default(), &chrome::Player::default());
        let texts = v.texts();
        assert!(texts.contains(&"A") && texts.contains(&"13+") && texts.contains(&"EXPERT"));
        assert!(texts.contains(&"C") && texts.contains(&"ANIME / POP"));
        assert!(texts.contains(&"1") && texts.contains(&"/2"));
        assert_eq!(v.hit(c.pt([0.0, 60.0])), Some(Action::Confirm));
        assert_eq!(v.hit(c.pt([245.0, 30.0])), Some(Action::Scroll(1)));
        assert_eq!(v.hit(c.pt([-492.0, 0.0])), Some(Action::Difficulty));
        assert_eq!(v.hit(c.pt([150.0, -248.0])), Some(Action::Genre(1)));
    }
}
