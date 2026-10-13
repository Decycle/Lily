//! Pieces shared by the menu screens: background, the user plate at the top (`BP_Circle_user`)
//! and the buttons around the rim (`BP_Circle_menu_btn`). Positions in design units
//! ([`crate::canvas`]); where the widget dumps leave them to Blueprint logic they are measured
//! on the real frames (`runs/e2/*/nav-*.png`, 0.625 px per unit, centre (337.5, 563)).

use wacca_render::style::Rgba;
use wacca_render::text::{Align, Face};

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
    Options,
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
            Icon::Options => (1.0, 1.0),
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

/// Side of the `uM_WCbg000` backdrop in BP_Select01 (genre / song select, final check): its
/// image is 1404 units (`umg_layout.py`), so the dot tiles are 1.3 x the mode screen's.
pub const SELECT_BACKDROP: f32 = 1404.0;
/// Side of the backdrop in BP_Select_level (mode select) and BP_Welcome.
pub const MODE_BACKDROP: f32 = 1080.0;

/// Backdrop of the select menus: the animated `uM_WCbg000` material on a `side` square.
pub fn background(p: &mut Painter, side: f32) {
    p.backdrop(side);
}

/// What the user plate shows (from the save: settings, play count, RP and rating).
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub name: String,
    /// Title line; empty shows the bare "Title" label.
    pub title: String,
    /// [`wacca_game::points::player_level`].
    pub level: u32,
    /// Reverse Points earned so far.
    pub rp: u64,
    /// Player rating as shown (whole points).
    pub rate: u32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            name: "GUEST".into(),
            title: String::new(),
            level: 1,
            rp: 0,
            rate: 0,
        }
    }
}

/// Plate label grey (`Title`, `Lv.`, `RP`).
const PLATE_LABEL: Rgba = hex(0x8f8f8f, 1.0);
/// `t_v_lv` / `t_v_wp` (linear 0.070) and the name (`TextBlock_5`, linear (0.0065, 0.007, 0.026)).
const VALUE_INK: Rgba = hex(0x4b4b4b, 1.0);
const NAME_INK: Rgba = hex(0x15162c, 1.0);

/// `BP_Circle_user`: plate `uT_US_1` hanging from the top, title line, level and RP, user icon,
/// name and rating. Text positions measured on `runs/e3/*/nav-08-mode-select.png`.
pub fn user_plate(p: &mut Painter, who: &Player) {
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
    // The guest's user icon (the "C" in the gradient ring), measured on the E3 frames.
    p.image(
        "UI/Textures/USERICON/S01/uT_UICN_S01_00_001",
        [-249.0, -360.0],
        [130.0, 130.0],
        MID,
        full,
        0.0,
        WHITE,
    );
    p.kakugo(&who.name, [-159.0, -350.0], 20.0, 4.0, NAME_INK);
    let title = hex(0xc4c4cc, 1.0);
    p.text("Title", [-232.0, -459.0], 9.0, title, Align::Left);
    p.text_fit(
        who.title.clone(),
        [-193.0, -459.0],
        8.0,
        title,
        Align::Left,
        Some(330.0),
    );
    p.text("Lv.", [-190.0, -413.0], 9.0, PLATE_LABEL, Align::Left);
    let ink = VALUE_INK;
    p.text(
        who.level.to_string(),
        [-104.0, -412.0],
        15.0,
        ink,
        Align::Right,
    );
    p.text("RP", [-82.0, -413.0], 9.0, PLATE_LABEL, Align::Left);
    p.text(who.rp.to_string(), [5.0, -412.0], 15.0, ink, Align::Right);
    p.text("Rate:", [308.0, -342.0], 9.0, WHITE, Align::Center);
    p.numerals(
        who.rate.to_string(),
        [345.0, -318.0],
        28.0,
        ink,
        Align::Right,
    );
}

/// The arcade's menu countdown (`BP_Time02`): two pink `SHINBI` digits, UMG size 25 with a
/// 1-unit violet outline, above GO (`LimitationTimeTable`; E3 frames, centre (0, 450)). Only
/// drawn with the `menu_timers` setting.
pub fn countdown(p: &mut Painter, secs: u32) {
    let text = format!("{:02}", secs.min(99));
    let (at, size) = ([0.0, 449.0], 25.0);
    let style = (Align::Center, Face::Shinbi);
    // Outline (linear (0.227, 0, 0.352)) as offset copies.
    let rim = hex(0x8300a0, 1.0);
    for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
        p.text_face(text.clone(), [at[0] + dx, at[1] + dy], size, rim, style);
    }
    p.text_face(text, at, size, PINK, style);
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

/// The pink GO wedge at the bottom of the rim (timer icon, "GO").
pub fn go(p: &mut Painter) {
    wedge(p, 0.0, 480.0, [230.0, 110.0], DECIDE_BTN);
    icon(p, Icon::Go, [-20.0, 487.0], 40.0, 0.0);
    p.text("GO", [22.0, 487.0], 27.0, WHITE, Align::Center);
    p.button([0.0, 480.0], [200.0, 90.0], Action::Confirm);
}

/// The results screen's RETRY tab, styled and placed like the game's FAVORITE tab (a pale-blue
/// rim wedge right of GO, a disc icon and a small turned label; E3 result frames).
/// The original toggles a favourite there; this build keeps RETRY in its place.
pub fn retry_tab(p: &mut Painter) {
    let deg = -21.0;
    wedge(p, deg, 478.0, [150.0, 122.0], hex(0x3d9be6, 0.8));
    icon(p, Icon::Back, [172.0, 459.0], 40.0, 0.0);
    p.text_turned("RETRY", [222.0, 434.0], 10.0, hex(0xe8f0ff, 1.0), deg);
    p.button([190.0, 452.0], [130.0, 90.0], Action::Retry);
}

/// The DIFFICULTY button on the left rim.
pub fn difficulty(p: &mut Painter) {
    icon(p, Icon::Plus, [-492.0, -2.0], 30.0, 0.0);
    p.text_turned("DIFFICULTY", [-488.0, 63.0], 13.0, WHITE, 90.0);
    p.button([-480.0, 50.0], [70.0, 140.0], Action::Difficulty);
}

/// The BACK wedge, lower left.
pub fn back(p: &mut Painter) {
    wedge(p, 51.0, 470.0, [170.0, 110.0], hex(0x6a1fb0, 0.9));
    icon(p, Icon::Back, [-380.0, 310.0], 34.0, 0.0);
    p.text_turned("BACK", [-352.0, 336.0], 12.0, WHITE, 51.0);
    p.button([-365.0, 325.0], [110.0, 90.0], Action::Back);
}

/// The buttons around the rim: DIFFICULTY (left), BACK, SORT, the two arrows and GO.
pub fn rim(p: &mut Painter, b: RimButtons) {
    go(p);
    for d in [27.0, -27.0] {
        wedge(p, d, 470.0, [190.0, 120.0], hex(0x1a4fb8, 0.9));
    }
    if b.sort.is_some() {
        wedge(p, -46.0, 470.0, [170.0, 110.0], MENU_BTN);
    }
    difficulty(p);

    icon(p, Icon::Left, [-223.0, 438.0], 36.0, 45.0);
    p.button([-223.0, 438.0], [110.0, 90.0], Action::Left);
    icon(p, Icon::Right, [222.0, 437.0], 36.0, -45.0);
    p.button([222.0, 437.0], [110.0, 90.0], Action::Right);

    if b.back {
        back(p);
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
