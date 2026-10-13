//! Title / attract screen, `Title\BP_Title_1`: the WACCA Reverse key art (the guide character
//! `uT_Chara000_RT000` in front of `uT_Chara000_EV001`), the `uT_LG04` logo, the message plate and
//! the START button at the bottom. Coin / credit / Aime texts of the arcade build are left out.
//!
//! The key art: the ghost behind is the material `uM_Title012` (ported, `title_glitch.wgsl`),
//! lit by `uM_Chara_2` smoke glows (ported, `chara_glow.wgsl`). The guide character in front is
//! not a widget of BP_Title_1 (its `Image_9` stays hidden), so he is a layer placed on the real
//! attract frame (`runs/explore/20261012-225558/nav-00-attract.png`) with his steady
//! magenta / cyan fringes and slipping strips in bursts; plus the halftone dots and the copyright
//! / version lines along the left rim (`runs/reference/20261012-223942/video.mkv`, 10–21 s).

use wacca_render::style::Rgba;
use wacca_render::text::{Align, UiMaterial};

use crate::canvas::{Canvas, FULL, MID, Painter, hex};
use crate::chrome::{self, Icon, WHITE};
use crate::ui::{Action, View};

/// The purple flash before the title appears (attract recording, 7.3 s: ~0.25 s).
pub const INTRO: f32 = 0.25;

const CHARA: &str = "CHARACTER/chara000/";
const BACKDROP: u32 = 0x310c9a;
/// Message plate text (UMG text colour of `t_coment` on the frame).
const PLATE_TEXT: u32 = 0x6a0fa0;
/// `ColorTable` `DecideBtnColor`.
const START_BTN: u32 = 0xe50485;

/// Copyright line along the left edge of the circle (attract frame; the start is hidden there).
const COPYRIGHT: &str = "Marvelous Inc. / Supported by HARDCORE TANO*C";
/// Light grey of the edge texts.
const EDGE_TEXT: u32 = 0xd8d8f0;

/// The title screen `t` seconds after it appeared. Any tap starts.
pub fn view(c: Canvas, t: f32) -> View {
    let mut p = Painter::new(c);
    p.fill([0.0, 0.0], [1080.0, 1080.0], MID, hex(BACKDROP, 1.0));
    // Glow behind the character.
    glow(&mut p, [0.0, -40.0], 900.0, [0.45, 0.05, 0.9, 1.0], t);
    // Halftone dots of the key art's background: a band behind the head, a patch on the left.
    halftone(&mut p, [124.0, -393.0], [600.0, 104.0], 14.0, 8.0);
    halftone(&mut p, [-330.0, -70.0], [230.0, 130.0], 24.0, 14.0);
    let g = glitch(t);
    // The second character behind, `uM_Title012`: each colour channel of her art shifted by its
    // own glitch. Placement measured on the attract frame (edge correlation). UNVERIFIED: the
    // material's default channel colours (lavender / pink / violet) do not match that frame
    // (teal crescents, grey-blue hair), so the widget must set them at run time; the art's own
    // colours with a measured blue tint stand in.
    p.material(
        UiMaterial::TitleGlitch,
        ([124.0, 62.0], [1220.0, 1220.0]),
        &[
            "uT_TTlogo07",
            "uT_CM06",
            &format!("{CHARA}uT_Chara000_EV001"),
        ],
        &title012(t),
        [0.6, 0.69, 1.0, 0.75],
    );
    let chara = format!("UI/Textures/{CHARA}uT_Chara000_RT000");
    layer(&mut p, &chara, [0.0, -17.0], 1080.0, WHITE, g);
    // Copyright and version along the left rim, reading downwards (attract frame).
    let edge = hex(EDGE_TEXT, 0.85);
    arc_text(&mut p, COPYRIGHT, 480.0, 210.0, 6.3, 6.0, edge);
    let version = concat!("Ver.", env!("CARGO_PKG_VERSION"));
    arc_text(&mut p, version, 482.0, 160.0, 9.6, 7.0, edge);
    // The purple light falling on the character's clothes.
    glow(&mut p, [0.0, 90.0], 700.0, [0.5, 0.0, 0.7, 1.0], t);
    // `Kerare` vignette and the dark gradient up from the bottom (`Image_10`, `uT_CM00`).
    p.tex("uT_CM04", [0.0, 0.0], [1080.0, 1080.0], hex(0x0a0b1c, 1.0));
    p.tex_ex(
        "uT_CM00",
        [0.0, 540.0],
        [1080.0, 435.0],
        [0.5, 1.0],
        FULL,
        0.0,
        hex(0x210038, 1.0),
    );
    p.image(
        "UI/Textures/LOGO/uT_LG04",
        [0.0, -5.0],
        [993.0, 248.0],
        MID,
        FULL,
        0.0,
        WHITE,
    );
    plate(&mut p);
    start_button(&mut p);
    if t < INTRO {
        let a = 1.0 - t / INTRO;
        p.fill([0.0, 0.0], [1080.0, 1080.0], MID, hex(0x4a10d0, a));
    }
    p.button([0.0, 0.0], [1080.0, 1080.0], Action::Confirm);
    p.circle_mask();
    p.finish()
}

/// `uM_Chara_2`'s smoky additive glow, `side` square at `at`, tinted (linear).
fn glow(p: &mut Painter, at: [f32; 2], side: f32, tint: [f32; 4], t: f32) {
    p.material(
        UiMaterial::CharaGlow,
        (at, [side, side]),
        &["uT_TTlogo07", "Vfx/Textures/vT_FogNoise00_00_m"],
        &[[t, 0.0, 0.0, 0.0]],
        tint,
    );
}

/// `uM_Title012`'s parameters at material time `t`, packed as `title_glitch.wgsl` reads them:
/// the material's defaults for the glitches (`ue4_material.py --uniforms`), durations clamped to
/// their span as the material does. The channel colours are not the defaults (color_1 =
/// (0.577, 0.537, 0.594), color_2 = (1, 0, 0.140), color_4 = (0.044, 0, 0.227)) but each channel
/// as itself (see the call site).
fn title012(t: f32) -> [[f32; 4]; 10] {
    let (span_1, duration_1, move_5) = (3.0, 1.0, 0.006);
    let (speed_1, width_1, offset_1, ran_1) = (0.02, 6.0, 0.005, 0.0);
    let (span_2, duration_2, speed_2, width_2, offset_2, ran_2) = (0.0, 0.0, 0.01, 1.0, 0.002, 0.0);
    let (span_3, duration_3, rotator_3, width_3, speed_3) = (0.0, 1.0f32, 0.0f32, 1.0, 0.0);
    let (offset_3, ran_3) = (0.01, 1.0);
    let turn = rotator_3 * std::f32::consts::TAU;
    [
        [t, span_1, f32::clamp(duration_1, 0.0, span_1), move_5],
        [speed_1, 1.0 / width_1, offset_1, ran_1],
        [
            span_2,
            f32::clamp(duration_2, 0.0, span_2),
            speed_2,
            1.0 / width_2,
        ],
        [offset_2, ran_2, span_3, duration_3.clamp(0.0, span_3)],
        [turn.cos(), turn.sin(), 1.0 / width_3, speed_3],
        [offset_3, ran_3, 0.0, 0.0],
        // Blue, red and green channel colours (linear), constant.
        [0.0, 0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0; 4],
    ]
}

/// Blue halftone dots in a `size` box around `at`: dots on a `pitch` grid, `max` wide on the
/// box's middle line and shrinking towards its edges (attract frame).
fn halftone(p: &mut Painter, at: [f32; 2], size: [f32; 2], pitch: f32, max: f32) {
    let color = hex(0x4f78ff, 0.55);
    let (cols, rows) = ((size[0] / pitch) as i32, (size[1] / pitch) as i32);
    for j in 0..=rows {
        let fy = (j as f32 / rows.max(1) as f32) * 2.0 - 1.0;
        for i in 0..=cols {
            let fx = (i as f32 / cols.max(1) as f32) * 2.0 - 1.0;
            let d = max * (1.0 - fy * fy) * (1.0 - fx.abs().powi(3));
            if d < 1.5 {
                continue;
            }
            let x = at[0] - size[0] / 2.0
                + i as f32 * pitch
                + if j % 2 == 1 { pitch / 2.0 } else { 0.0 };
            let y = at[1] - size[1] / 2.0 + j as f32 * pitch;
            p.tex("uT_CMen16_00", [x, y], [d, d], color);
        }
    }
}

/// One glitch burst of the key art (material `uM_Title012`, `uT_CM_Glitch_*` displacement).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glitch {
    /// Varies the strip offsets per burst.
    seed: u32,
}

/// Length of one glitch time slot.
const GLITCH_SLOT: f32 = 0.08;

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// The burst at `t`, if any: the attract recording shows short (≈ 0.05–0.5 s) bursts at
/// irregular intervals, ≈ 1 s apart on average (frame differences, 10–18 s of the recording).
// UNVERIFIED: the material's real schedule; this is a deterministic pseudo-random one.
fn glitch(t: f32) -> Option<Glitch> {
    if t < INTRO {
        return None;
    }
    let slot = (t / GLITCH_SLOT) as u32;
    (hash(slot) % 100 < 9).then_some(Glitch {
        seed: hash(slot ^ 0x9e37_79b9),
    })
}

/// Strips a glitching layer is cut into.
const STRIPS: u32 = 14;

/// A key-art layer, square `side` centred at `at`. During a burst cyan and magenta copies peek out
/// left and right of it and some horizontal strips slip sideways.
fn layer(p: &mut Painter, key: &str, at: [f32; 2], side: f32, tint: Rgba, g: Option<Glitch>) {
    // Magenta and cyan copies always peek out left and right of the art (attract recording,
    // 12-21 s: steady fringes, wider during bursts).
    let split = if g.is_some() { 9.0 } else { 5.0 };
    for (d, rgb) in [([-split, -2.0], 0xff30c8), ([split, 0.0], 0x30f0ff)] {
        let ghost = hex(rgb, 0.6 * tint[3]);
        let at = [at[0] + d[0], at[1] + d[1]];
        p.image(key, at, [side, side], MID, FULL, 0.0, ghost);
    }
    let Some(g) = g else {
        p.image(key, at, [side, side], MID, FULL, 0.0, tint);
        return;
    };
    for i in 0..STRIPS {
        let h = hash(g.seed.wrapping_add(i));
        let shift = if h.is_multiple_of(3) {
            ((h >> 8) % 41) as f32 - 20.0
        } else {
            0.0
        };
        let (v0, v1) = (i as f32 / STRIPS as f32, (i + 1) as f32 / STRIPS as f32);
        let y = at[1] - side / 2.0 + side * (v0 + v1) / 2.0;
        let size = [side, side * (v1 - v0)];
        p.image(
            key,
            [at[0] + shift, y],
            size,
            MID,
            [0.0, v0, 1.0, v1],
            0.0,
            tint,
        );
    }
}

/// `s` along the circle of radius `r` from `start` degrees (screen angles, y down) downwards on
/// the left rim, `advance` design units per character, letters' tops towards the centre.
fn arc_text(p: &mut Painter, s: &str, r: f32, start: f32, advance: f32, size: f32, color: Rgba) {
    for (i, ch) in s.chars().enumerate().filter(|(_, c)| !c.is_whitespace()) {
        let a = start.to_radians() - (i as f32 + 0.5) * advance / r;
        let (sin, cos) = a.sin_cos();
        let turn = (-cos).atan2(sin).to_degrees();
        p.text_turned(&ch.to_string(), [r * cos, r * sin], size, color, turn);
    }
}

/// The lilac message plate (`uT_CMfukidasi00`, angled left end) under the logo.
fn plate(p: &mut Painter) {
    p.tex_ex(
        "uT_CMfukidasi00",
        [-344.0, 320.0],
        [860.0, 198.0],
        [0.0, 0.5],
        FULL,
        0.0,
        hex(0xe4d8ff, 1.0),
    );
    let ink = hex(PLATE_TEXT, 1.0);
    p.text(
        "Touch the glowing area",
        [76.0, 299.0],
        16.0,
        ink,
        Align::Center,
    );
    p.text("to start!", [76.0, 329.0], 16.0, ink, Align::Center);
}

/// The glowing START area at the bottom of the rim (the decide wedge of `BP_Circle_menu_btn`).
fn start_button(p: &mut Painter) {
    // The trapezoid fills the lower half of the texture; drawn twice for the solid pink.
    for _ in 0..2 {
        p.tex(
            "uT_CMbtn005",
            [0.0, 440.0],
            [300.0, 300.0],
            hex(START_BTN, 1.0),
        );
    }
    chrome::icon(p, Icon::Go, [-60.0, 475.0], 44.0, 0.0);
    p.text("START", [19.0, 475.0], 32.0, WHITE, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Item;

    #[test]
    fn any_tap_starts_and_there_is_no_coin_text() {
        let c = Canvas::fit(1080.0, 1920.0);
        let v = view(c, 1.0);
        assert_eq!(v.hit(c.pt([0.0, 0.0])), Some(Action::Confirm));
        assert_eq!(v.hit(c.pt([300.0, -300.0])), Some(Action::Confirm));
        let texts = v.texts();
        assert!(texts.contains(&"START"));
        assert!(
            texts
                .iter()
                .all(|t| !t.contains("CREDIT") && !t.contains("COIN"))
        );
    }

    #[test]
    fn intro_flash_fades_out() {
        let c = Canvas::fit(1080.0, 1920.0);
        let flash = |t| {
            view(c, t).items.iter().any(
                |i| matches!(i, Item::Fill { color, .. } if *color == hex(0x4a10d0, 1.0 - t / INTRO)),
            )
        };
        assert!(flash(0.0) && flash(0.1));
        assert!(!view(c, INTRO).items.iter().any(|i| matches!(
            i,
            Item::Fill { color, .. } if color[..3] == hex(0x4a10d0, 1.0)[..3]
        )));
    }

    #[test]
    fn glitch_bursts_are_occasional_and_split_the_art() {
        let slots = (0..2000).map(|i| i as f32 * GLITCH_SLOT + 1.0);
        let on = slots.clone().filter(|&t| glitch(t).is_some()).count();
        assert!((100..300).contains(&on), "{on} of 2000 slots glitch");
        assert_eq!(glitch(0.1), None);
        let t = slots.into_iter().find(|&t| glitch(t).is_some()).unwrap() + 0.01;
        let strips = |t| {
            let v = view(Canvas::fit(1080.0, 1920.0), t);
            v.items
                .iter()
                .filter(|i| {
                    matches!(i, Item::Image { key, uv, .. } if key.contains("RT000") && *uv != FULL)
                })
                .count()
        };
        assert_eq!(strips(t), STRIPS as usize);
        let calm = (0..2000)
            .map(|i| i as f32 * GLITCH_SLOT + 1.01)
            .find(|&t| glitch(t).is_none())
            .unwrap();
        assert_eq!(strips(calm), 0);
    }

    #[test]
    fn ghost_is_the_ported_glitch_material_with_its_clock() {
        let v = view(Canvas::fit(1080.0, 1920.0), 2.5);
        let ghost = v.items.iter().find_map(|i| match i {
            Item::Material {
                material: UiMaterial::TitleGlitch,
                textures,
                params,
                ..
            } => Some((textures, params)),
            _ => None,
        });
        let (textures, params) = ghost.expect("uM_Title012 layer");
        assert!(textures[2].ends_with("uT_Chara000_EV001"));
        assert_eq!(params[0][0], 2.5);
        // Duration_3 (1) is clamped to Span_3 (0) like the material does.
        assert_eq!(params[3][3], 0.0);
        let glows = v
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    Item::Material {
                        material: UiMaterial::CharaGlow,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(glows, 2);
    }

    #[test]
    fn edge_texts_follow_the_left_rim() {
        let mut p = Painter::new(Canvas::new([0.0, 0.0], 1.0));
        arc_text(&mut p, "ab", 480.0, 180.0, 10.0, 6.0, WHITE);
        let v = p.finish();
        let turned: Vec<_> = v
            .items
            .iter()
            .filter_map(|i| match i {
                Item::TurnedText { angle, .. } => Some(angle.to_degrees()),
                _ => None,
            })
            .collect();
        // Reading downwards on the left edge: letters turned a quarter clockwise.
        assert_eq!(turned.len(), 2);
        assert!(turned.iter().all(|a| (a - 90.0).abs() < 2.0), "{turned:?}");
        let texts = view(Canvas::fit(1080.0, 1920.0), 1.0).texts().concat();
        assert!(texts.contains("HARDCORETANO*C") && texts.contains("Ver."));
    }
}
