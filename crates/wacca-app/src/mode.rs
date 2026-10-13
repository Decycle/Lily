//! Mode select, `Mode\BP_Select_mode`: a wheel of mode discs around the centre dial
//! (`uT_STmode_obj01`) under the `uT_STmode_obj10` arches. Only single play exists here, so it is
//! the selected disc at the bottom; Shop and Multiplayer keep their place on the wheel, drawn
//! locked and greyed as the original shows them for a guest, and cannot be selected (the arrows do
//! nothing). Positions are measured on the real frame
//! (`runs/e2/20261012-233738/nav-08-mode-select.png`, 0.625 px per unit, centre (337.5, 563)).
// UNVERIFIED: "USE ITEM" and the mode-art material (`uM_AD_6`) are left out (the countdown is
// drawn by the shell with the `menu_timers` setting);
// the art ring uses `uT_RGclear03`, which matches its look.

use wacca_render::text::Align;

use crate::canvas::{Canvas, FULL, MID, Painter, hex};
use crate::chrome::{self, Icon, PINK, WHITE};
use crate::ui::{Action, View};

/// Centre of the dial (the widget's sub-panel sits slightly above the circle centre).
const DIAL: [f32; 2] = [0.0, -20.0];
/// Mode discs on the wheel: Shop (left), Multiplayer (right), Singleplayer (bottom, selected).
const SIDE_DISC: f32 = 500.0;
const DISC_SIZE: f32 = 480.0;
const LOCKED_TEXT: u32 = 0x8a0f45;
/// The selected mode's name (UMG linear (0.73, 0.0, 0.14) ≈ sRGB).
const MODE_TEXT: u32 = 0xc8106a;
/// `t_mode_manual00` (linear (0.29, 0.24, 0.52)).
const MANUAL_TEXT: u32 = 0x9287bf;
/// `ColorTable` `DecideBtnColor`.
const DECIDE_BTN: u32 = 0xe50485;

/// The mode select screen `t` seconds after it appeared.
pub fn view(c: Canvas, t: f32, who: &chrome::Player) -> View {
    let mut p = Painter::new(c);
    p.tex("uT_WCbg00", [0.0, 0.0], [1080.0, 1080.0], WHITE);
    // The mode art in a ring around the dial.
    disc_image(&mut p, "uT_RGclear03", DIAL, 620.0);
    discs(&mut p);
    dial(&mut p, t);
    pills(&mut p);
    chrome::user_plate(&mut p, who);
    bottom(&mut p);
    p.button([0.0, 283.0], [400.0, 170.0], Action::Confirm);
    p.button(DIAL, [340.0, 340.0], Action::Confirm);
    p.button([0.0, 480.0], [200.0, 90.0], Action::Confirm);
    p.circle_mask();
    p.finish()
}

/// Rows a round image is cut into.
const DISC_ROWS: usize = 48;

/// Texture `name` cut to a disc of `diameter` at `at`, as horizontal strips (the overlay
/// renderer has no masks).
fn disc_image(p: &mut Painter, name: &str, at: [f32; 2], diameter: f32) {
    let r = diameter / 2.0;
    for (y0, y1, half) in disc_rows(DISC_ROWS) {
        let [u0, u1] = [0.5 - half / 2.0, 0.5 + half / 2.0];
        let size = [diameter * half, diameter * (y1 - y0)];
        let centre = [at[0], at[1] - r + diameter * (y0 + y1) / 2.0];
        p.tex_ex(name, centre, size, MID, [u0, y0, u1, y1], 0.0, WHITE);
    }
}

/// `n` rows of a unit disc: (top, bottom, width), all as fractions of the diameter.
fn disc_rows(n: usize) -> impl Iterator<Item = (f32, f32, f32)> {
    (0..n).map(move |i| {
        let (y0, y1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let y = (y0 + y1) / 2.0 - 0.5;
        (y0, y1, 2.0 * (0.25 - y * y).sqrt())
    })
}

fn discs(p: &mut Painter) {
    let size = [DISC_SIZE, DISC_SIZE];
    p.tex("uT_STmode002", [-SIDE_DISC, 0.0], size, WHITE);
    p.tex("uT_STmode002", [SIDE_DISC, 0.0], size, WHITE);
    p.tex("uT_STmode003", [0.0, SIDE_DISC], size, WHITE);
}

/// Dark band, teal ring, the glowing dial and its texts; the down chevron bobs (`uM_CM_move01`).
fn dial(p: &mut Painter, t: f32) {
    p.tex("uT_CMen512_00", DIAL, [440.0, 440.0], hex(0x2a0870, 1.0));
    p.tex("uT_CMflame_L04", DIAL, [384.0, 384.0], hex(0x30c8d0, 0.7));
    p.tex_ex(
        "uT_STmode_obj10",
        [DIAL[0], DIAL[1] - 230.0],
        [1024.0, 512.0],
        MID,
        FULL,
        0.0,
        WHITE,
    );
    p.tex("uT_STmode_obj01", DIAL, [490.0, 490.0], WHITE);
    let manual = hex(MANUAL_TEXT, 1.0);
    let lines = [
        "Slide the glowing",
        "area to select",
        "the mode you",
        "wish to play.",
    ];
    for (i, l) in lines.iter().enumerate() {
        let y = DIAL[1] - 76.0 + 16.0 * i as f32;
        p.text(*l, [0.0, y], 10.0, manual, Align::Center);
    }
    for (i, l) in ["Play songs", "of your choice", "alone."]
        .iter()
        .enumerate()
    {
        let y = DIAL[1] + 41.0 + 24.0 * i as f32;
        p.text(*l, [0.0, y], 15.0, PINK, Align::Center);
    }
    // UNVERIFIED: bob period and amplitude.
    let bob = 4.0 * (t * std::f32::consts::TAU).sin();
    let y = DIAL[1] + 151.0 + bob;
    p.line([-12.0, y - 6.0], [0.0, y + 4.0], 3.0, PINK);
    p.line([12.0, y - 6.0], [0.0, y + 4.0], 3.0, PINK);
}

/// Mode name plates: the selected one framed in pink, the locked ones dark with a lock.
fn pills(p: &mut Painter) {
    p.tex("uT_STmode_obj03", [0.0, 300.0], [470.0, 235.0], WHITE);
    p.tex("uT_STmode_obj02", [0.0, 300.0], [455.0, 228.0], WHITE);
    p.text(
        "Singleplayer",
        [0.0, 283.0],
        26.0,
        hex(MODE_TEXT, 1.0),
        Align::Center,
    );
    let locked = hex(LOCKED_TEXT, 1.0);
    for (x, name) in [(-361.0, "Shop"), (361.0, "Multiplayer")] {
        p.tex("uT_STmode_obj05", [x, 0.0], [390.0, 195.0], WHITE);
        p.text(name, [x, 0.0], 18.0, locked, Align::Center);
    }
}

/// The GO area and the arrow buttons at the bottom of the rim.
fn bottom(p: &mut Painter) {
    // The trapezoid fills the lower half of the texture; drawn twice for the solid pink.
    for _ in 0..2 {
        p.tex(
            "uT_CMbtn005",
            [0.0, 420.0],
            [330.0, 330.0],
            hex(DECIDE_BTN, 1.0),
        );
    }
    chrome::icon(p, Icon::Go, [-20.0, 487.0], 40.0, 0.0);
    p.text("GO", [22.0, 487.0], 22.0, WHITE, Align::Center);
    for (x, y, icon, deg) in [
        (-223.0, 438.0, Icon::Left, 45.0),
        (222.0, 437.0, Icon::Right, -45.0),
    ] {
        p.tex("uT_CMen64_00", [x, y], [52.0, 52.0], WHITE);
        chrome::icon(p, icon, [x, y], 36.0, deg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_rows_follow_the_circle() {
        let rows: Vec<_> = disc_rows(4).collect();
        assert_eq!(rows.len(), 4);
        assert_eq!((rows[0].0, rows[3].1), (0.0, 1.0));
        // Symmetric, widest in the middle.
        assert!((rows[0].2 - rows[3].2).abs() < 1e-6);
        assert!(rows[1].2 > rows[0].2 && rows[1].2 < 1.0);
    }

    #[test]
    fn single_play_is_the_only_choice() {
        let c = Canvas::fit(1080.0, 1920.0);
        let v = view(c, 0.0, &chrome::Player::default());
        let texts = v.texts();
        for name in ["Singleplayer", "Shop", "Multiplayer", "GO"] {
            assert!(texts.contains(&name), "{name}");
        }
        for at in [[0.0, 283.0], DIAL, [0.0, 487.0]] {
            assert_eq!(v.hit(c.pt(at)), Some(Action::Confirm));
        }
        // Locked modes and the arrows are not buttons.
        for at in [[-361.0, 0.0], [361.0, 0.0], [-223.0, 438.0]] {
            assert_eq!(v.hit(c.pt(at)), None);
        }
    }
}
