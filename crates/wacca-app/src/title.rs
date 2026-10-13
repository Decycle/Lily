//! Title / attract screen, `Title\BP_Title_1`: the WACCA Reverse key art (the guide character
//! `uT_Chara000_RT000` in front of `uT_Chara000_EV001`), the `uT_LG04` logo, the message plate and
//! the START button at the bottom. Coin / credit / Aime texts of the arcade build are left out.
//!
//! The key art is the material `uM_Title012`, which composites the character textures with a
//! glitch effect; it is drawn here as plain layers placed on the real attract frame
//! (`runs/explore/20261012-225558/nav-00-attract.png`).

use wacca_render::text::Align;

use crate::canvas::{Canvas, FULL, MID, Painter, hex};
use crate::chrome::{self, Icon, WHITE};
use crate::ui::{Action, View};

/// The purple flash before the title appears (attract recording, 7.3 s: ~0.25 s).
pub const INTRO: f32 = 0.25;

const CHARA: &str = "CHARACTER/chara000/";
const BACKDROP: u32 = 0x3a05c8;
/// Message plate text (UMG text colour of `t_coment` on the frame).
const PLATE_TEXT: u32 = 0x6a0fa0;
/// `ColorTable` `DecideBtnColor`.
const START_BTN: u32 = 0xe50485;

/// The title screen `t` seconds after it appeared. Any tap starts.
pub fn view(c: Canvas, t: f32) -> View {
    let mut p = Painter::new(c);
    p.fill([0.0, 0.0], [1080.0, 1080.0], MID, hex(BACKDROP, 1.0));
    // Glow behind the character.
    p.tex("uT_CM19", [0.0, -40.0], [900.0, 900.0], hex(0xb060ff, 0.7));
    // The second character, a dim ghost on the right with her coloured hair streaks.
    p.image(
        &format!("UI/Textures/{CHARA}uT_Chara000_EV001"),
        [76.0, 95.0],
        [1300.0, 1300.0],
        MID,
        FULL,
        0.0,
        hex(0xd0d8ff, 0.75),
    );
    p.image(
        &format!("UI/Textures/{CHARA}uT_Chara000_RT000"),
        [0.0, -17.0],
        [1080.0, 1080.0],
        MID,
        FULL,
        0.0,
        WHITE,
    );
    // The purple light falling on the character's clothes.
    p.tex("uT_CM19", [0.0, 90.0], [700.0, 700.0], hex(0xc070ff, 0.45));
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
        [0.0, 6.0],
        [1060.0, 265.0],
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

/// The lilac message plate (`uT_CMfukidasi00`, angled left end) under the logo.
fn plate(p: &mut Painter) {
    p.tex_ex(
        "uT_CMfukidasi00",
        [-360.0, 344.0],
        [1000.0, 230.0],
        [0.0, 0.5],
        FULL,
        0.0,
        hex(0xe4d8ff, 1.0),
    );
    let ink = hex(PLATE_TEXT, 1.0);
    p.text(
        "Touch the glowing area",
        [57.0, 310.0],
        21.0,
        ink,
        Align::Center,
    );
    p.text("to start!", [57.0, 346.0], 21.0, ink, Align::Center);
}

/// The glowing START area at the bottom of the rim (the decide wedge of `BP_Circle_menu_btn`).
fn start_button(p: &mut Painter) {
    // The trapezoid fills the lower half of the texture; drawn twice for the solid pink.
    for _ in 0..2 {
        p.tex(
            "uT_CMbtn005",
            [0.0, 455.0],
            [270.0, 270.0],
            hex(START_BTN, 1.0),
        );
    }
    chrome::icon(p, Icon::Go, [-60.0, 491.0], 44.0, 0.0);
    p.text("START", [25.0, 491.0], 26.0, WHITE, Align::Center);
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
}
