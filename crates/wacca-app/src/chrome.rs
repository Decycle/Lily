//! Pieces shared by the menu screens: background, the user plate at the top (`BP_Circle_user`)
//! and the buttons around the rim (`BP_Circle_menu_btn`). Positions in design units
//! ([`crate::canvas`]); where the widget dumps leave them to Blueprint logic they are measured
//! on the real frames (`runs/e2/*/nav-*.png`, 0.625 px per unit, centre (337.5, 563)).

use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::canvas::{MID, Painter, hex};
use crate::ui::Action;

/// `uT_STmenu_*` plates are tinted with this (UMG linear (0.016, 0, 0.040)).
pub const INK: Rgba = hex(0x220038, 1.0);
/// The game's accent pink (linear (0.965, 0.0006, 0.392)).
pub const PINK: Rgba = hex(0xfb02a8, 1.0);
pub const WHITE: Rgba = hex(0xffffff, 1.0);
/// `ColorTable` `MenuBtnColor` / `DecideBtnColor`.
const MENU_BTN: Rgba = hex(0x147bcf, 1.0);
const DECIDE_BTN: Rgba = hex(0xe50485, 1.0);

/// Cell of the 8 × 4 icon atlas `uT_CMbtn_icn_UV`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Go,
    Back,
    Sort,
    Right,
    Left,
    Plus,
}

impl Icon {
    fn cell(self) -> (f32, f32) {
        match self {
            Icon::Go => (0.0, 0.0),
            Icon::Back => (1.0, 0.0),
            Icon::Sort => (6.0, 1.0),
            Icon::Right => (0.0, 3.0),
            Icon::Left => (1.0, 3.0),
            Icon::Plus => (2.0, 3.0),
        }
    }

    pub fn uv(self) -> [f32; 4] {
        let (c, r) = self.cell();
        [c / 8.0, r / 4.0, (c + 1.0) / 8.0, (r + 1.0) / 4.0]
    }
}

pub fn icon(p: &mut Painter, icon: Icon, at: [f32; 2], side: f32, degrees: f32) {
    p.tex_ex(
        "uT_CMbtn_icn_UV",
        at,
        [side, side],
        MID,
        icon.uv(),
        degrees,
        WHITE,
    );
}

/// Grid background behind the menus (the blue tile pattern of the select screens).
// UNVERIFIED: approximates the `uM_WCbg000`-style material backdrop with flat tiles.
pub fn background(p: &mut Painter) {
    p.fill([0.0, 0.0], [1080.0, 1080.0], MID, hex(0x2f5bcf, 1.0));
    let line = hex(0x1f0a5c, 0.85);
    let cell = 22.0;
    for i in -25..=25 {
        let x = i as f32 * cell;
        p.fill([x, 0.0], [3.0, 1080.0], MID, line);
        p.fill([0.0, x], [1080.0, 3.0], MID, line);
    }
    // Darker purple towards the rim, as on the frames.
    p.fill(
        [0.0, -540.0],
        [1080.0, 250.0],
        [0.5, 0.0],
        hex(0x2a0a6a, 0.55),
    );
    p.fill(
        [0.0, 540.0],
        [1080.0, 120.0],
        [0.5, 1.0],
        hex(0x3a0a70, 0.6),
    );
}

/// `BP_Circle_user`: plate `uT_US_1` hanging from the top, user icon and name.
pub fn user_plate(p: &mut Painter, name: &str) {
    let full = crate::canvas::FULL;
    let plate = "UI/Textures/USERPLATE/uT_US_1";
    p.image(
        plate,
        [0.0, -540.0],
        [1000.0, 250.0],
        [0.5, 0.0],
        full,
        0.0,
        WHITE,
    );
    // Guest icon: a black "C" in a ring.
    // UNVERIFIED: the real ring is a pink → blue → yellow gradient; drawn in the accent pink.
    let at = [-247.0, -360.0];
    p.tex("uT_CMen256_00", at, [118.0, 118.0], hex(0xdddddd, 1.0));
    p.tex("uT_CMen256_00", at, [104.0, 104.0], PINK);
    p.tex("uT_CMen256_00", at, [84.0, 84.0], WHITE);
    p.text(
        "C",
        [at[0], at[1] + 4.0],
        50.0,
        hex(0x111111, 1.0),
        Align::Center,
    );
    p.text(
        name,
        [-161.0, -350.0],
        22.0,
        hex(0x13142d, 1.0),
        Align::Left,
    );
}

/// Which rim buttons a screen shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RimButtons {
    pub back: bool,
    /// SORT button with the current order's name.
    pub sort: Option<&'static str>,
}

/// A coloured wedge on the rim at `degrees` from the bottom (positive = towards the left).
fn wedge(p: &mut Painter, degrees: f32, radius: f32, size: [f32; 2], tint: Rgba) {
    let a = degrees.to_radians();
    let at = [-radius * a.sin(), radius * a.cos()];
    p.tex_ex(
        "uT_CMbtn005",
        at,
        size,
        MID,
        crate::canvas::FULL,
        degrees,
        tint,
    );
}

/// The buttons around the rim: DIFFICULTY (left), BACK, SORT, the two arrows and GO.
pub fn rim(p: &mut Painter, b: RimButtons) {
    wedge(p, 0.0, 480.0, [230.0, 110.0], DECIDE_BTN);
    for d in [27.0, -27.0] {
        wedge(p, d, 470.0, [190.0, 120.0], hex(0x1a4fb8, 0.9));
    }
    if b.back {
        wedge(p, 51.0, 470.0, [170.0, 110.0], hex(0x6a1fb0, 0.9));
    }
    if b.sort.is_some() {
        wedge(p, -46.0, 470.0, [170.0, 110.0], MENU_BTN);
    }

    icon(p, Icon::Plus, [-492.0, -2.0], 30.0, 0.0);
    p.text_turned("DIFFICULTY", [-488.0, 63.0], 13.0, WHITE, 90.0);
    p.button([-480.0, 50.0], [70.0, 140.0], Action::Difficulty);

    icon(p, Icon::Left, [-223.0, 438.0], 36.0, 45.0);
    p.button([-223.0, 438.0], [110.0, 90.0], Action::Left);
    icon(p, Icon::Right, [222.0, 437.0], 36.0, -45.0);
    p.button([222.0, 437.0], [110.0, 90.0], Action::Right);

    icon(p, Icon::Go, [-20.0, 487.0], 40.0, 0.0);
    p.text("GO", [22.0, 487.0], 22.0, WHITE, Align::Center);
    p.button([0.0, 480.0], [200.0, 90.0], Action::Confirm);

    if b.back {
        icon(p, Icon::Back, [-380.0, 310.0], 34.0, 0.0);
        p.text_turned("BACK", [-352.0, 336.0], 12.0, WHITE, 51.0);
        p.button([-365.0, 325.0], [110.0, 90.0], Action::Back);
    }
    if let Some(order) = b.sort {
        icon(p, Icon::Sort, [348.0, 341.0], 34.0, 0.0);
        p.text_turned("SORT", [374.0, 314.0], 12.0, WHITE, -41.0);
        p.text_turned(order, [398.0, 342.0], 7.0, WHITE, -41.0);
        p.button([362.0, 325.0], [110.0, 90.0], Action::NextSort);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;

    #[test]
    fn icon_cells_tile_the_atlas() {
        assert_eq!(Icon::Go.uv(), [0.0, 0.0, 0.125, 0.25]);
        assert_eq!(Icon::Plus.uv(), [0.25, 0.75, 0.375, 1.0]);
    }

    #[test]
    fn rim_buttons_map_to_actions() {
        let c = Canvas::fit(1080.0, 1080.0);
        let mut p = Painter::new(c);
        rim(
            &mut p,
            RimButtons {
                back: true,
                sort: None,
            },
        );
        let v = p.finish();
        let at = |x: f32, y: f32| v.hit(c.pt([x, y]));
        assert_eq!(at(-492.0, 0.0), Some(Action::Difficulty));
        assert_eq!(at(0.0, 487.0), Some(Action::Confirm));
        assert_eq!(at(-380.0, 310.0), Some(Action::Back));
        assert_eq!(at(348.0, 341.0), None);
        assert_eq!(at(222.0, 437.0), Some(Action::Right));
    }
}
