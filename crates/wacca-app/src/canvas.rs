//! The original menus' coordinate space and a small drawing helper on top of [`View`].
//!
//! The game lays its menus out in 1080 × 1080 UMG panels centred on the round screen (x right,
//! y down, origin = circle centre; positions in `docs/research/06-visuals.md` widget dumps),
//! inside a 1080 × 1920 portrait frame with the circle at (540, 900). The menu circle is drawn
//! exactly on the play screen's ring ([`wacca_render::layout::ring`], one function for both), so
//! menus and play have the same scale. On a portrait window that is the original frame,
//! letterboxed; on a landscape window the circle spans the height and the parts of the frame
//! above and below it ([`OFF_CIRCLE`]) move into the free space left and right of it.
//! Everything outside the circle is masked black like on the cabinet.

use std::f32::consts::PI;

use wacca_render::layout::{PORTRAIT_FRAME, PORTRAIT_RING_CENTER, Rect, ring};
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::circle;
use crate::ui::{Action, Item, View};

/// Side of the design canvas.
pub const DESIGN: f32 = 1080.0;
/// Radius of the visible circle (measured on real frames).
pub const CIRCLE_R: f32 = circle::RADIUS;
/// Exported textures (`wacca-convert --export-ui UI/Textures`), relative to `data/ui`.
pub const TEX: &str = "UI/Textures/";
/// Whole texture.
pub const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
/// Whole texture, mirrored horizontally (UMG `Scale (-1, 1)`).
pub const MIRROR: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// `0xRRGGBB` (sRGB) with alpha `a`.
pub const fn hex(rgb: u32, a: f32) -> Rgba {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        a,
    ]
}

/// UMG font size (points at 96 dpi) to design pixels.
pub fn font_px(size: f32) -> f32 {
    size * 4.0 / 3.0
}

/// Half of the 1080 × 1080 square around the circle: elements anchored inside it stay on the
/// circle.
const HALF: f32 = DESIGN / 2.0;
/// The portrait frame's top and bottom edge in design units.
const FRAME_TOP: f32 = -PORTRAIT_RING_CENTER[1];
const FRAME_BOTTOM: f32 = PORTRAIT_FRAME[1] - PORTRAIT_RING_CENTER[1];

/// Left or right of the circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Landscape placement of the portrait frame outside the circle: the band above it (design
/// `y` range) goes to the left, the band below to the right, each keeping its inner layout.
/// (The screens built so far keep everything on the circle, like the original.)
pub const OFF_CIRCLE: [(f32, f32, Side); 2] = [
    (FRAME_TOP, -HALF, Side::Left),
    (HALF, FRAME_BOTTOM, Side::Right),
];

/// Gap between a moved band and the circle / the window edge, as a fraction of the height.
const SIDE_MARGIN: f32 = 0.02;

/// Design → window pixels for one element: `from` (design) lands on `to` (pixels), scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub scale: f32,
}

impl Place {
    pub fn pt(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            self.to[0] + (x - self.from[0]) * self.scale,
            self.to[1] + (y - self.from[1]) * self.scale,
        ]
    }
}

/// Maps design coordinates to window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub cx: f32,
    pub cy: f32,
    pub scale: f32,
    /// Where each [`OFF_CIRCLE`] band goes (landscape windows only).
    bands: Option<[Place; 2]>,
}

impl Canvas {
    /// The circle at `center` with `scale` pixels per design unit; nothing moved (the portrait
    /// frame).
    pub fn new(center: [f32; 2], scale: f32) -> Self {
        Self {
            cx: center[0],
            cy: center[1],
            scale,
            bands: None,
        }
    }

    /// The menus on a `w × h` window (see the module docs).
    pub fn fit(w: f32, h: f32) -> Self {
        let r = ring(w, h);
        let mut c = Self::new(r.center, r.radius / CIRCLE_R);
        if w >= h {
            let margin = SIDE_MARGIN * h;
            c.bands = Some(OFF_CIRCLE.map(|(top, bottom, side)| {
                let (x0, x1) = match side {
                    Side::Left => (0.0, r.center[0] - r.radius),
                    Side::Right => (r.center[0] + r.radius, w),
                };
                let room = [x1 - x0 - 2.0 * margin, h - 2.0 * margin];
                let fit = (room[0] / DESIGN).min(room[1] / (bottom - top));
                Place {
                    from: [0.0, (top + bottom) / 2.0],
                    to: [(x0 + x1) / 2.0, h / 2.0],
                    scale: c.scale.min(fit).max(0.0),
                }
            }));
        }
        c
    }

    /// The circle's own mapping (also for shapes clipped to the circle).
    pub fn ring(&self) -> Place {
        Place {
            from: [0.0, 0.0],
            to: [self.cx, self.cy],
            scale: self.scale,
        }
    }

    /// Mapping for an element anchored at `at`: its band's on a landscape window when `at` lies
    /// above or below the circle's square, else the circle's.
    pub fn place(&self, at: [f32; 2]) -> Place {
        let Some(bands) = self.bands else {
            return self.ring();
        };
        OFF_CIRCLE
            .iter()
            .zip(bands)
            .find(|((top, bottom, _), _)| at[1].abs() > HALF && (*top..=*bottom).contains(&at[1]))
            .map_or_else(|| self.ring(), |(_, p)| p)
    }

    /// A point on the circle's mapping.
    pub fn pt(&self, p: [f32; 2]) -> [f32; 2] {
        self.ring().pt(p)
    }

    /// Rectangle of `size` whose point `align` (0..1 per axis, UMG "Alignment") is at `at`.
    pub fn rect(&self, at: [f32; 2], size: [f32; 2], align: [f32; 2]) -> Rect {
        let m = self.place(at);
        let [x, y] = m.pt([at[0] - size[0] * align[0], at[1] - size[1] * align[1]]);
        Rect {
            x,
            y,
            w: size[0] * m.scale,
            h: size[1] * m.scale,
        }
    }
}

/// Builds a [`View`] in design coordinates.
#[derive(Debug)]
pub struct Painter {
    pub c: Canvas,
    pub v: View,
    /// Elements moved beside the circle; drawn last (over the circle mask).
    side: View,
}

/// Centre alignment.
pub const MID: [f32; 2] = [0.5, 0.5];

impl Painter {
    pub fn new(c: Canvas) -> Self {
        Self {
            c,
            v: View::default(),
            side: View::default(),
        }
    }

    /// The draw list for an element anchored at `at` (see [`Canvas::place`]).
    fn layer(&mut self, at: [f32; 2]) -> &mut View {
        if self.c.place(at) == self.c.ring() {
            &mut self.v
        } else {
            &mut self.side
        }
    }

    /// The select menus' animated backdrop (`uM_WCbg000`) on the 1080 square around the circle.
    pub fn backdrop(&mut self) {
        self.v.backdrop = Some(self.c.rect([0.0, 0.0], [DESIGN, DESIGN], MID));
    }

    /// Texture `name` (under [`TEX`]) centred at `at`.
    pub fn tex(&mut self, name: &str, at: [f32; 2], size: [f32; 2], tint: Rgba) {
        self.tex_ex(name, at, size, MID, FULL, 0.0, tint);
    }

    /// Texture with UMG alignment, uv region and angle (degrees, clockwise).
    #[allow(clippy::too_many_arguments)]
    pub fn tex_ex(
        &mut self,
        name: &str,
        at: [f32; 2],
        size: [f32; 2],
        align: [f32; 2],
        uv: [f32; 4],
        degrees: f32,
        tint: Rgba,
    ) {
        self.image(&format!("{TEX}{name}"), at, size, align, uv, degrees, tint);
    }

    /// Any exported texture by its full key under `data/ui` (see [`Item::Image`]).
    #[allow(clippy::too_many_arguments)]
    pub fn image(
        &mut self,
        key: &str,
        at: [f32; 2],
        size: [f32; 2],
        align: [f32; 2],
        uv: [f32; 4],
        degrees: f32,
        tint: Rgba,
    ) {
        let rect = self.c.rect(at, size, align);
        self.layer(at).items.push(Item::Image {
            key: key.to_string(),
            rect,
            uv,
            angle: degrees * PI / 180.0,
            tint,
        });
    }

    /// A pair of textures meeting at `at`: the left one as is, the right one mirrored (the UMG
    /// idiom for symmetric plates: alignment (1, a) + alignment (0, a) with scale (-1, 1)).
    pub fn mirrored(&mut self, name: &str, at: [f32; 2], size: [f32; 2], ay: f32, tint: Rgba) {
        self.tex_ex(name, at, size, [1.0, ay], FULL, 0.0, tint);
        self.tex_ex(name, at, size, [0.0, ay], MIRROR, 0.0, tint);
    }

    /// A straight line of `width` from `a` to `b` (a turned white texture).
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: Rgba) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let degrees = dx.atan2(-dy).to_degrees();
        let size = [width, dx.hypot(dy)];
        self.tex_ex("uT_STmenu_obj042", mid, size, MID, FULL, degrees, color);
    }

    pub fn fill(&mut self, at: [f32; 2], size: [f32; 2], align: [f32; 2], color: Rgba) {
        let rect = self.c.rect(at, size, align);
        self.layer(at).fill(rect, color);
    }

    pub fn jacket(&mut self, key: &str, at: [f32; 2], side: f32) {
        let rect = self.c.rect(at, [side, side], MID);
        self.layer(at).items.push(Item::Jacket {
            rect,
            key: key.to_string(),
        });
    }

    /// One line of text vertically centred on `at[1]` at UMG font `size`.
    pub fn text(
        &mut self,
        s: impl Into<String>,
        at: [f32; 2],
        size: f32,
        color: Rgba,
        align: Align,
    ) {
        self.text_fit(s, at, size, color, align, None);
    }

    /// Like [`Painter::text`], cut with an ellipsis to `max_w` design units.
    pub fn text_fit(
        &mut self,
        s: impl Into<String>,
        at: [f32; 2],
        size: f32,
        color: Rgba,
        align: Align,
        max_w: Option<f32>,
    ) {
        let px = font_px(size);
        let m = self.c.place(at);
        let [x, y] = m.pt([at[0], at[1] - px * 0.62]);
        let k = m.scale;
        self.layer(at)
            .text_in(s, [x, y], px * k, color, align, max_w.map(|w| w * k));
    }

    /// One line in the game's numeral face, vertically centred on `at[1]`, `px` design units.
    pub fn numerals(
        &mut self,
        s: impl Into<String>,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
    ) {
        let m = self.c.place(at);
        let p = m.pt([at[0], at[1] - px * 0.58]);
        self.layer(at).numerals(s, p, px * m.scale, color, align);
    }

    /// One line centred on `at`, turned by `degrees` (clockwise).
    pub fn text_turned(&mut self, s: &str, at: [f32; 2], size: f32, color: Rgba, degrees: f32) {
        let px = font_px(size);
        let a = degrees.to_radians();
        // Rotate the line's top-centre about `at`; the renderer turns the line about it.
        let up = 0.62 * px;
        let m = self.c.place(at);
        let top = m.pt([at[0] + up * a.sin(), at[1] - up * a.cos()]);
        self.layer(at).items.push(Item::TurnedText {
            text: s.to_string(),
            at: top,
            px: px * m.scale,
            color,
            align: Align::Center,
            angle: a,
        });
    }

    /// Text with a UMG outline of `width` design units.
    pub fn text_outlined(
        &mut self,
        s: &str,
        at: [f32; 2],
        size: f32,
        color: Rgba,
        outline: Rgba,
        width: f32,
    ) {
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
            .into_iter()
            .chain([(-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)])
        {
            self.text(
                s,
                [at[0] + dx * width, at[1] + dy * width],
                size,
                outline,
                Align::Center,
            );
        }
        self.text(s, at, size, color, Align::Center);
    }

    pub fn button(&mut self, at: [f32; 2], size: [f32; 2], action: Action) {
        let rect = self.c.rect(at, size, MID);
        self.layer(at).button(rect, action);
    }

    /// Black outside the visible circle (the cabinet's round screen; `uT_Mask00` is white
    /// outside a circle, transparent inside).
    pub fn circle_mask(&mut self) {
        let side = 2.0 * CIRCLE_R;
        self.tex_ex(
            "uT_Mask00",
            [0.0, 0.0],
            [side, side],
            MID,
            FULL,
            0.0,
            hex(0, 1.0),
        );
        // The mask texture stops at its square; the window is larger.
        let (r, far) = (CIRCLE_R, 4.0 * DESIGN);
        for (at, size, align) in [
            ([-r, 0.0], [far, far], [1.0, 0.5]),
            ([r, 0.0], [far, far], [0.0, 0.5]),
            ([0.0, -r], [far, far], [0.5, 1.0]),
            ([0.0, r], [far, far], [0.5, 0.0]),
        ] {
            self.fill(at, size, align, hex(0, 1.0));
        }
    }

    pub fn finish(mut self) -> View {
        self.v.items.append(&mut self.side.items);
        self.v.buttons.append(&mut self.side.buttons);
        self.v
    }
}

/// Exponential approach of a visual offset to 0 (wheel scrolling): `offset` after `dt` seconds
/// with time constant `tau`.
pub fn ease(offset: f32, dt: f32, tau: f32) -> f32 {
    let o = offset * (-dt / tau).exp();
    if o.abs() < 1e-3 { 0.0 } else { o }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wacca_render::layout::{playfield_viewport, ring};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// 16:10 (the laptop), 16:9, 4:3, 21:9, square, 9:16, a 3:4 window and the 9:16 frame.
    const WINDOWS: [[f32; 2]; 8] = [
        [1920.0, 1200.0],
        [1920.0, 1080.0],
        [1600.0, 1200.0],
        [2560.0, 1080.0],
        [1000.0, 1000.0],
        [900.0, 1600.0],
        [900.0, 1200.0],
        [1080.0, 1920.0],
    ];

    fn overlap(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    }

    fn inside(r: Rect, [w, h]: [f32; 2]) -> bool {
        r.x >= -1e-3 && r.y >= -1e-3 && r.x + r.w <= w + 1e-3 && r.y + r.h <= h + 1e-3
    }

    #[test]
    fn menu_circle_is_the_play_ring() {
        for [w, h] in WINDOWS {
            let (c, r) = (Canvas::fit(w, h), ring(w, h));
            assert_eq!(c.pt([0.0, 0.0]), r.center, "{w}x{h}");
            assert!(close(CIRCLE_R * c.scale, r.radius), "{w}x{h}");
            // The circle's square is inside the play viewport (touch maps through it too).
            let square = c.rect([0.0, 0.0], [2.0 * CIRCLE_R; 2], MID);
            let vp = playfield_viewport(w, h);
            assert!(inside(square, [w, h]) && square.x >= vp.x && square.y >= vp.y);
        }
        // The laptop: the circle spans the screen height like the play ring.
        let c = Canvas::fit(1920.0, 1200.0);
        assert!(CIRCLE_R * c.scale > 0.48 * 1200.0);
    }

    #[test]
    fn portrait_windows_keep_the_original_frame() {
        for [w, h] in [[900.0, 1600.0], [1080.0, 1920.0], [900.0, 1200.0]] {
            let c = Canvas::fit(w, h);
            // Nothing moves: one mapping for the whole frame, which fits the window.
            for at in [[0.0, FRAME_TOP], [0.0, FRAME_BOTTOM], [-HALF, 700.0]] {
                assert_eq!(c.place(at), c.ring(), "{w}x{h} {at:?}");
            }
            let frame = c.rect(
                [0.0, FRAME_TOP],
                [DESIGN, FRAME_BOTTOM - FRAME_TOP],
                [0.5, 0.0],
            );
            assert!(inside(frame, [w, h]), "{w}x{h} {frame:?}");
        }
    }

    #[test]
    fn bands_beside_the_circle_are_on_screen_and_apart() {
        for size @ [w, h] in [
            [1920.0, 1200.0],
            [1920.0, 1080.0],
            [1600.0, 1200.0],
            [2560.0, 1080.0],
        ] {
            let c = Canvas::fit(w, h);
            let circle = c.rect([0.0, 0.0], [2.0 * CIRCLE_R; 2], MID);
            let bands: Vec<Rect> = OFF_CIRCLE
                .iter()
                .map(|&(top, bottom, side)| {
                    let mid = (top + bottom) / 2.0;
                    let at = [0.0, mid];
                    let r = c.rect(at, [DESIGN, bottom - top], MID);
                    // Moved, to its side, never scaled up beyond the circle's scale.
                    let m = c.place(at);
                    assert!(m != c.ring() && m.scale <= c.scale && m.scale > 0.0);
                    match side {
                        Side::Left => assert!(r.x + r.w <= circle.x),
                        Side::Right => assert!(r.x >= circle.x + circle.w),
                    }
                    r
                })
                .collect();
            for b in &bands {
                assert!(inside(*b, size), "{w}x{h} {b:?}");
                assert!(!overlap(*b, circle), "{w}x{h} {b:?}");
            }
            assert!(!overlap(bands[0], bands[1]));
            // Elements keep their order inside a band.
            let [a, b] = [[-200.0, -800.0], [200.0, -600.0]].map(|p| c.rect(p, [1.0; 2], MID));
            assert!(a.x < b.x && a.y < b.y);
        }
    }

    #[test]
    fn off_circle_elements_are_drawn_over_the_mask_and_stay_tappable() {
        let c = Canvas::fit(1920.0, 1200.0);
        let mut p = Painter::new(c);
        p.text(
            "TOP",
            [0.0, -700.0],
            20.0,
            hex(0xffffff, 1.0),
            Align::Center,
        );
        p.button([0.0, -700.0], [300.0, 100.0], Action::Back);
        p.circle_mask();
        let v = p.finish();
        assert!(matches!(v.items.last(), Some(Item::Text { text, .. }) if text == "TOP"));
        let at = c.place([0.0, -700.0]).pt([0.0, -700.0]);
        assert!(at[0] < 1920.0 / 2.0 - ring(1920.0, 1200.0).radius);
        assert_eq!(v.hit(at), Some(Action::Back));
    }

    #[test]
    fn every_menu_button_is_on_screen() {
        use crate::select::tests::list;
        use wacca_game::save::{SaveData, Settings};
        let l = list();
        let save = SaveData::default();
        let who = crate::chrome::Player::default();
        for size @ [w, h] in [
            [1920.0, 1200.0],
            [1920.0, 1080.0],
            [1600.0, 1200.0],
            [900.0, 1600.0],
        ] {
            let c = Canvas::fit(w, h);
            let menus = crate::options::OptionsMenu::default();
            let views = [
                crate::title::view(c, 1.0),
                crate::mode::view(c, 1.0, &who),
                crate::category::view(c, &l, 0, 0.0, &save, &who),
                crate::select_ui::view(c, &l, 0.0, &save, &who),
                crate::options::view(c, &menus, (&Settings::default(), false), &l, &save, &who),
            ];
            // The circle is on screen and every tap area is centred on screen, except the
            // wheel's outer slots, which sit past the circle under the mask as on the cabinet.
            assert!(inside(c.rect([0.0, 0.0], [2.0 * CIRCLE_R; 2], MID), size));
            for v in views {
                assert!(!v.buttons.is_empty());
                let shown = |a: &Action| !matches!(a, Action::Scroll(n) if n.abs() >= 3);
                for (b, a) in v.buttons.iter().filter(|(_, a)| shown(a)) {
                    let mid = [b.x + b.w / 2.0, b.y + b.h / 2.0];
                    let on = (0.0..w).contains(&mid[0]) && (0.0..h).contains(&mid[1]);
                    assert!(on, "{w}x{h} {a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn colours_and_fonts() {
        assert_eq!(hex(0xff8000, 0.5), [1.0, 128.0 / 255.0, 0.0, 0.5]);
        assert!((font_px(30.0) - 40.0).abs() < 1e-4);
    }

    #[test]
    fn mirrored_pair_meets_at_the_anchor() {
        let c = Canvas::new([540.0, 540.0], 1.0);
        let mut p = Painter::new(c);
        p.mirrored("x", [0.0, 0.0], [100.0, 50.0], 0.5, hex(0xffffff, 1.0));
        let v = p.finish();
        let rects: Vec<_> = v
            .items
            .iter()
            .map(|i| match i {
                Item::Image { rect, uv, .. } => (rect.x, *uv),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(rects, [(440.0, FULL), (540.0, MIRROR)]);
    }

    #[test]
    fn ease_decays_to_zero() {
        let o = ease(1.0, 0.05, 0.05);
        assert!((o - (-1.0f32).exp()).abs() < 1e-5);
        assert_eq!(ease(0.0005, 0.0, 1.0), 0.0);
        assert!(ease(-2.0, 0.01, 0.1) < 0.0);
    }
}
