//! The original menus' coordinate space and a small drawing helper on top of [`View`].
//!
//! The game lays its menus out in a 1080 × 1080 UMG canvas centred on the round screen (x right,
//! y down, origin = circle centre; positions in `docs/research/06-visuals.md` widget dumps). Our
//! window is landscape, so that square is scaled to the window height and centred — the same
//! square the playfield uses ([`playfield_viewport`]) — and everything outside the circle is
//! masked black like on the cabinet. The original 1080 × 1920 portrait frame only adds black
//! bars above and below the circle, so nothing visible is lost.

use std::f32::consts::PI;

use wacca_render::layout::{Rect, playfield_viewport};
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::ui::{Action, Item, View};

/// Side of the design canvas.
pub const DESIGN: f32 = 1080.0;
/// Radius of the visible circle (measured on real frames: 529 of 540).
pub const CIRCLE_R: f32 = 529.0;
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

/// Maps design coordinates to window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub cx: f32,
    pub cy: f32,
    pub scale: f32,
}

impl Canvas {
    /// The design square scaled into the largest centred square of a `w × h` window.
    pub fn fit(w: f32, h: f32) -> Self {
        let r = playfield_viewport(w, h);
        Self {
            cx: r.x + r.w / 2.0,
            cy: r.y + r.h / 2.0,
            scale: r.w / DESIGN,
        }
    }

    pub fn pt(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [self.cx + x * self.scale, self.cy + y * self.scale]
    }

    /// Rectangle of `size` whose point `align` (0..1 per axis, UMG "Alignment") is at `at`.
    pub fn rect(&self, at: [f32; 2], size: [f32; 2], align: [f32; 2]) -> Rect {
        let [x, y] = self.pt([at[0] - size[0] * align[0], at[1] - size[1] * align[1]]);
        Rect {
            x,
            y,
            w: size[0] * self.scale,
            h: size[1] * self.scale,
        }
    }
}

/// Builds a [`View`] in design coordinates.
#[derive(Debug)]
pub struct Painter {
    pub c: Canvas,
    pub v: View,
}

/// Centre alignment.
pub const MID: [f32; 2] = [0.5, 0.5];

impl Painter {
    pub fn new(c: Canvas) -> Self {
        Self {
            c,
            v: View::default(),
        }
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
        self.v.items.push(Item::Image {
            key: key.to_string(),
            rect: self.c.rect(at, size, align),
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
        self.v.fill(rect, color);
    }

    pub fn jacket(&mut self, key: &str, at: [f32; 2], side: f32) {
        self.v.items.push(Item::Jacket {
            rect: self.c.rect(at, [side, side], MID),
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
        let [x, y] = self.c.pt([at[0], at[1] - px * 0.62]);
        let k = self.c.scale;
        self.v
            .text_in(s, [x, y], px * k, color, align, max_w.map(|w| w * k));
    }

    /// One line centred on `at`, turned by `degrees` (clockwise).
    pub fn text_turned(&mut self, s: &str, at: [f32; 2], size: f32, color: Rgba, degrees: f32) {
        let px = font_px(size);
        let a = degrees.to_radians();
        // Rotate the line's top-centre about `at`; the renderer turns the line about it.
        let up = 0.62 * px;
        let top = self.c.pt([at[0] + up * a.sin(), at[1] - up * a.cos()]);
        self.v.items.push(Item::TurnedText {
            text: s.to_string(),
            at: top,
            px: px * self.c.scale,
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
        self.v.button(rect, action);
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

    pub fn finish(self) -> View {
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

    #[test]
    fn canvas_maps_the_design_square_to_the_window_height() {
        let c = Canvas::fit(1920.0, 1080.0);
        assert_eq!(c.pt([0.0, 0.0]), [960.0, 540.0]);
        assert_eq!(c.pt([-540.0, -540.0]), [420.0, 0.0]);
        let r = c.rect([100.0, 0.0], [200.0, 100.0], [1.0, 0.5]);
        assert_eq!((r.x, r.y, r.w, r.h), (860.0, 490.0, 200.0, 100.0));
        let half = Canvas::fit(960.0, 540.0);
        assert_eq!(half.scale, 0.5);
    }

    #[test]
    fn colours_and_fonts() {
        assert_eq!(hex(0xff8000, 0.5), [1.0, 128.0 / 255.0, 0.0, 0.5]);
        assert!((font_px(30.0) - 40.0).abs() < 1e-4);
    }

    #[test]
    fn mirrored_pair_meets_at_the_anchor() {
        let mut p = Painter::new(Canvas::fit(1080.0, 1080.0));
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
