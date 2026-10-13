//! Mode select, `Mode\BP_Select_mode`: a wheel of mode discs around the centre dial
//! (`uT_STmode_obj01`) under the `uT_STmode_obj10` arches. Only single play exists here, so it is
//! the selected disc at the bottom; Shop and Multiplayer keep their place on the wheel, drawn
//! locked and greyed as the original shows them for a guest, and cannot be selected (the arrows do
//! nothing). Positions are measured on the real frame
//! (`runs/e2/20261012-233738/nav-08-mode-select.png`, 0.625 px per unit, centre (337.5, 563)).
// UNVERIFIED: "USE ITEM" is left out (the countdown is
// drawn by the shell with the `menu_timers` setting);
// the art ring is the ported `uM_CMcircle001` (`uM_AD_6` belongs to the collapsed BP_Popup00).

use wacca_render::text::{Align, UiMaterial};

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
const MODE_TEXT: u32 = 0xa5127a;
/// `t_mode_manual00` (linear (0.29, 0.24, 0.52)).
const MANUAL_TEXT: u32 = 0x9287bf;
/// `ColorTable` `DecideBtnColor`.
const DECIDE_BTN: u32 = 0xe50485;

/// The mode select screen `t` seconds after it appeared.
pub fn view(c: Canvas, t: f32, who: &chrome::Player) -> View {
    let mut p = Painter::new(c);
    chrome::background(&mut p);
    // The mode art in a ring around the dial.
    art_ring(&mut p, t);
    discs(&mut p);
    dial(&mut p, t);
    pills(&mut p, t);
    chrome::user_plate(&mut p, who);
    bottom(&mut p);
    p.button([0.0, 283.0], [400.0, 170.0], Action::Confirm);
    p.button(DIAL, [340.0, 340.0], Action::Confirm);
    p.button([0.0, 480.0], [200.0, 90.0], Action::Confirm);
    p.circle_mask();
    p.finish()
}

/// The art ring around the dial, `uM_CMcircle001` (`ring10` of `BP_Select_level`): uT_RGclear03
/// turning at 0.05 turns/s and swirled (Radius_1 0.5, Hardness 0), material time `t`.
// UNVERIFIED: the widget's slot is 496 units with the material's ring mask (uT_TTIogo04,
// radii 0.4-0.5); the E3 frame shows the swirled art from the dial out to r ~310, so it is
// drawn as a 620-unit disc (the "in" animation's scale was not evaluated).
fn art_ring(p: &mut Painter, t: f32) {
    p.material(
        UiMaterial::CircleSwirl,
        (DIAL, [620.0, 620.0]),
        &["uT_RGclear03", "uT_TTIogo04"],
        &[[t, 0.05, 1.0 / 0.5, 1.0], [1.0, 0.0, 0.0, 0.0]],
        [1.0; 4],
    );
}

fn discs(p: &mut Painter) {
    let size = [DISC_SIZE, DISC_SIZE];
    p.tex("uT_STmode002", [-SIDE_DISC, 0.0], size, WHITE);
    p.tex("uT_STmode002", [SIDE_DISC, 0.0], size, WHITE);
    p.tex("uT_STmode001", [0.0, SIDE_DISC], size, WHITE);
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

/// Mode name plates: the selected one in its frame with the pulsing `uM_CM_active01` glow
/// (emissive `(1 + sin(2π · 2.2 · t)) · color1` over the frame), the locked ones dark with a
/// lock. Pill fill and text colours measured on the E3 frame.
// UNVERIFIED: glow colour / strength from the frame (magenta at about 55 %); the pulse rate is
// the material's default `speed` 2.2.
fn pills(p: &mut Painter, t: f32) {
    p.tex("uT_STmode_obj03", [0.0, 280.0], [470.0, 235.0], WHITE);
    let pulse = 1.0 + 0.35 * (t * 2.2 * std::f32::consts::TAU).sin();
    p.tex(
        "uT_STmode_obj04",
        [0.0, 280.0],
        [650.0, 395.0],
        hex(0xfe07fd, 0.55 * pulse),
    );
    p.tex(
        "uT_STmode_obj02",
        [0.0, 280.0],
        [455.0, 228.0],
        hex(0xe7e7e7, 1.0),
    );
    p.text(
        "Singleplayer",
        [0.0, 283.0],
        27.0,
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
    p.text("GO", [22.0, 487.0], 27.0, WHITE, Align::Center);
    for (x, y, icon, deg) in [
        (-223.0, 438.0, Icon::Left, 45.0),
        (222.0, 437.0, Icon::Right, -45.0),
    ] {
        p.tex("uT_CMen64_00", [x, y], [40.0, 40.0], WHITE);
        chrome::icon(p, icon, [x, y], 28.0, deg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_ring_is_the_swirl_material_on_the_dial() {
        let v = view(Canvas::fit(1080.0, 1920.0), 3.0, &chrome::Player::default());
        let ring = v.items.iter().find_map(|i| match i {
            crate::ui::Item::Material {
                material: UiMaterial::CircleSwirl,
                textures,
                params,
                ..
            } => Some((textures.clone(), params[0][0])),
            _ => None,
        });
        let (textures, time) = ring.expect("uM_CMcircle001");
        assert!(textures[0].ends_with("uT_RGclear03"));
        assert_eq!(time, 3.0);
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
