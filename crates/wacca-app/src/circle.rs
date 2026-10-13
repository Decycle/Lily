//! Shared look of the round-screen menus (results, options): drawing in the original 1080 × 1920
//! portrait canvas, placed on the window like the other menus ([`canvas::Canvas`]), the
//! background disc, slanted panels clipped to the round screen, word-art textures and the GO /
//! side-tab buttons. Positions and colours are measured from the E2 real-game frames.

use wacca_render::layout::{PORTRAIT_RING_CENTER, Rect};
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::canvas::{self, Place};
use crate::ui::{self, Action, Item, View};

/// The round screen in canvas units.
// UNVERIFIED: measured from E2 frames (675 × 1200 captures × 1.6).
pub const CENTER: [f32; 2] = PORTRAIT_RING_CENTER;
pub const RADIUS: f32 = 530.0;

/// Canvas point → menu design units (origin at the circle centre).
fn design([x, y]: [f32; 2]) -> [f32; 2] {
    [x - CENTER[0], y - CENTER[1]]
}

/// Panels lean 30° from vertical: a panel's bottom edge sits this much × its height to the left.
pub const SLANT: f32 = 0.577;
/// Segments of the clip / background circle.
const SEGMENTS: usize = 96;

pub const PINK: Rgba = [1.0, 0.01, 0.68, 1.0];
pub const NAVY: Rgba = [0.10, 0.08, 0.28, 1.0];
pub const INK: Rgba = [0.16, 0.15, 0.20, 1.0];
const BG_TOP: Rgba = [0.27, 0.10, 0.62, 1.0];
const BG_BOTTOM: Rgba = [0.17, 0.05, 0.45, 1.0];

/// Exported word-art texture key under `data/ui`.
pub fn font_tex(name: &str) -> String {
    format!("UI/Textures/FONT/{name}")
}

/// Fraction of `t` between `start` and `start + len`, eased out (fast then settling), 0..=1.
pub fn ease(t: f32, start: f32, len: f32) -> f32 {
    let p = ((t - start) / len).clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

/// The screen circle as a polygon (clockwise on screen).
pub fn disc() -> Vec<[f32; 2]> {
    (0..SEGMENTS)
        .map(|i| {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            [CENTER[0] + RADIUS * a.cos(), CENTER[1] + RADIUS * a.sin()]
        })
        .collect()
}

/// Sutherland–Hodgman: the part of `poly` inside the convex polygon `clip` (same winding).
/// Colours are interpolated along clipped edges.
pub fn clip(poly: &[([f32; 2], Rgba)], clip: &[[f32; 2]]) -> Vec<([f32; 2], Rgba)> {
    let mut out = poly.to_vec();
    for (i, &a) in clip.iter().enumerate() {
        let b = clip[(i + 1) % clip.len()];
        let side = |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let input = std::mem::take(&mut out);
        for (j, &(p, pc)) in input.iter().enumerate() {
            let (q, qc) = input[(j + 1) % input.len()];
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push((p, pc));
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                let mix = |x: f32, y: f32| x + (y - x) * t;
                out.push((
                    [mix(p[0], q[0]), mix(p[1], q[1])],
                    std::array::from_fn(|k| mix(pc[k], qc[k])),
                ));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Parallelogram leaning like the game's panels: top edge `x..x + w` at `y`, height `h`.
pub fn slab(x: f32, y: f32, w: f32, h: f32) -> [[f32; 2]; 4] {
    let d = h * SLANT;
    [[x, y], [x + w, y], [x + w - d, y + h], [x - d, y + h]]
}

/// Draws in canvas units into a [`View`], optionally shifted and faded (slide-in animations).
#[derive(Clone, Copy, Debug)]
pub struct Canvas {
    /// The menus' mapping ([`canvas::Canvas`], origin at the circle centre).
    menu: canvas::Canvas,
    pub offset: [f32; 2],
    pub alpha: f32,
}

impl Canvas {
    pub fn new(menu: canvas::Canvas) -> Self {
        Self {
            menu,
            offset: [0.0, 0.0],
            alpha: 1.0,
        }
    }

    /// Same canvas, shifted by `d` canvas units and faded to `alpha`.
    pub fn moved(self, d: [f32; 2], alpha: f32) -> Self {
        Self {
            offset: [self.offset[0] + d[0], self.offset[1] + d[1]],
            alpha: self.alpha * alpha,
            ..self
        }
    }

    fn shift(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [x + self.offset[0], y + self.offset[1]]
    }

    /// Mapping of an element anchored at canvas point `at`, chosen by its resting place (not
    /// by where a slide-in has moved it).
    fn place(&self, at: [f32; 2]) -> Place {
        self.menu.place(design(at))
    }

    /// Canvas point → window pixels, mapped like an element anchored there.
    pub fn p(&self, p: [f32; 2]) -> [f32; 2] {
        self.place(p).pt(design(self.shift(p)))
    }

    /// Shifted canvas point on the circle's mapping (shapes clipped to the circle).
    fn on_circle(&self, p: [f32; 2]) -> [f32; 2] {
        self.menu.pt(design(p))
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let [px, py] = self.p([x, y]);
        let s = self.scale([x, y]);
        Rect {
            x: px,
            y: py,
            w: w * s,
            h: h * s,
        }
    }

    /// Pixels per canvas unit for an element anchored at `at`.
    fn scale(&self, at: [f32; 2]) -> f32 {
        self.place(at).scale
    }

    fn fade(&self, mut c: Rgba) -> Rgba {
        c[3] *= self.alpha;
        c
    }

    /// Filled convex polygon clipped to the round screen; `colors` per point or one flat colour.
    pub fn panel(&self, v: &mut View, points: &[[f32; 2]], colors: &[Rgba]) {
        let Some(&flat) = colors.first() else { return };
        let poly: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, &p)| (self.shift(p), colors.get(i).copied().unwrap_or(flat)))
            .collect();
        let clipped = clip(&poly, &disc());
        if clipped.len() >= 3 {
            v.poly(
                clipped.iter().map(|&(p, _)| self.on_circle(p)).collect(),
                clipped.iter().map(|&(_, c)| self.fade(c)).collect(),
            );
        }
    }

    /// One line of text; `at` = (anchor x per `align`, top) in canvas units, size `px`.
    pub fn text(&self, v: &mut View, s: &str, at: [f32; 2], px: f32, color: Rgba, align: Align) {
        v.text_in(
            s,
            self.p(at),
            px * self.scale(at),
            self.fade(color),
            align,
            None,
        );
    }

    /// Centred text cut to `max` canvas units wide.
    pub fn text_fit(&self, v: &mut View, s: &str, at: [f32; 2], px: f32, color: Rgba, max: f32) {
        self.fit(v, s, at, px, color, max, Align::Center);
    }

    /// Text cut to `max` canvas units wide, anchored per `align`.
    #[allow(clippy::too_many_arguments)]
    pub fn fit(
        &self,
        v: &mut View,
        s: &str,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        max: f32,
        align: Align,
    ) {
        let sc = self.scale(at);
        v.text_in(
            s,
            self.p(at),
            px * sc,
            self.fade(color),
            align,
            Some(max * sc),
        );
    }

    /// One line in the game's numeral face.
    pub fn numerals(
        &self,
        v: &mut View,
        s: &str,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
    ) {
        v.numerals(s, self.p(at), px * self.scale(at), self.fade(color), align);
    }

    /// Characters on a fixed `pitch` (the game's wide-spaced labels), first cell from `x`.
    pub fn spaced(&self, v: &mut View, s: &str, at: [f32; 2], px: f32, pitch: f32, color: Rgba) {
        self.cells(v, s, at, pitch, |v, ch, at| {
            self.text(v, ch, at, px, color, Align::Center)
        });
    }

    /// [`Canvas::spaced`] in the numeral face (counters, scores).
    pub fn counter(&self, v: &mut View, s: &str, at: [f32; 2], px: f32, pitch: f32, color: Rgba) {
        self.cells(v, s, at, pitch, |v, ch, at| {
            self.numerals(v, ch, at, px, color, Align::Center)
        });
    }

    fn cells(
        &self,
        v: &mut View,
        s: &str,
        [x, y]: [f32; 2],
        pitch: f32,
        mut draw: impl FnMut(&mut View, &str, [f32; 2]),
    ) {
        for (i, ch) in s.chars().enumerate() {
            let cx = x + (i as f32 + 0.5) * pitch;
            draw(v, &ch.to_string(), [cx, y]);
        }
    }

    /// Texture `key` repeated on tiles `[x, y, w, h]`, `[x + w, y, w, h]`, … (both ways), cut to
    /// the convex polygon `poly` and the round screen. `flip` mirrors each tile left-right.
    pub fn tiled(
        &self,
        v: &mut View,
        key: String,
        poly: &[[f32; 2]],
        tile: [f32; 4],
        flip: bool,
        alpha: f32,
    ) {
        let poly: Vec<_> = poly.iter().map(|&p| (self.shift(p), ui::WHITE)).collect();
        let shape = clip(&poly, &disc());
        let [tx, ty] = self.shift([tile[0], tile[1]]);
        let [tw, th] = [tile[2], tile[3]];
        let xs = shape.iter().map(|(p, _)| p[0]);
        let lo = xs.clone().fold(f32::MAX, f32::min);
        let hi = xs.fold(f32::MIN, f32::max);
        for i in ((lo - tx) / tw).floor() as i32..((hi - tx) / tw).ceil() as i32 {
            let x = tx + i as f32 * tw;
            let cell = [[x, ty], [x + tw, ty], [x + tw, ty + th], [x, ty + th]];
            let part = clip(&shape, &cell);
            if part.len() < 3 {
                continue;
            }
            let uv = |[px, py]: [f32; 2]| {
                let u = (px - x) / tw;
                [if flip { 1.0 - u } else { u }, (py - ty) / th]
            };
            v.items.push(Item::ImagePoly {
                key: key.clone(),
                points: part.iter().map(|&(p, _)| self.on_circle(p)).collect(),
                uvs: part.iter().map(|&(p, _)| uv(p)).collect(),
                tint: [1.0, 1.0, 1.0, self.alpha * alpha],
            });
        }
    }

    /// Texture `key` on the quad `corners` (canvas units; texture corners top-left, top-right,
    /// bottom-right, bottom-left), clipped to the convex `within` and the round screen.
    pub fn image_quad(
        &self,
        v: &mut View,
        key: String,
        corners: [[f32; 2]; 4],
        within: &[[f32; 2]],
        tint: Rgba,
    ) {
        // The clipper interpolates its colour payload: carry the texture coordinates in it.
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let quad: Vec<_> = corners
            .iter()
            .zip(uvs)
            .map(|(&p, [u, w])| (self.shift(p), [u, w, 0.0, 0.0]))
            .collect();
        let part = clip(&clip(&quad, within), &disc());
        if part.len() >= 3 {
            v.items.push(Item::ImagePoly {
                key,
                points: part.iter().map(|&(p, _)| self.on_circle(p)).collect(),
                uvs: part.iter().map(|&(_, c)| [c[0], c[1]]).collect(),
                tint: self.fade(tint),
            });
        }
    }

    /// Texture `key` centred on `c`, `size` canvas units.
    pub fn image(&self, v: &mut View, key: String, c: [f32; 2], [w, h]: [f32; 2], alpha: f32) {
        let r = self.rect(c[0] - w / 2.0, c[1] - h / 2.0, w, h);
        v.image(key, r, [1.0, 1.0, 1.0, self.alpha * alpha]);
    }

    pub fn button(&self, v: &mut View, r: [f32; 4], action: Action) {
        v.button(self.rect(r[0], r[1], r[2], r[3]), action);
    }
}

/// The purple field of the round screen with the two pink diagonal stripes.
pub fn background(v: &mut View, c: &Canvas) {
    let pts = disc();
    let colors: Vec<_> = pts
        .iter()
        .map(|p| {
            let t = ((p[1] - CENTER[1]) / RADIUS + 1.0) / 2.0;
            std::array::from_fn(|k| BG_TOP[k] + (BG_BOTTOM[k] - BG_TOP[k]) * t)
        })
        .collect();
    c.panel(v, &pts, &colors);
    for x in [595.0, 1345.0] {
        c.panel(v, &slab(x, 380.0, 12.0, 1100.0), &[PINK]);
    }
}

/// A slanted tab button left or right of GO (RETRY, BACK).
pub fn side_tab(v: &mut View, c: &Canvas, label: &str, x: f32, color: Rgba, action: Action) {
    let (y, w, h) = (1300.0, 130.0, 56.0);
    c.panel(v, &slab(x, y, w, h), &[color]);
    let mid = x + w / 2.0 - h * SLANT / 2.0;
    c.text(v, label, [mid, y + 12.0], 30.0, ui::WHITE, Align::Center);
    c.button(
        v,
        [x - h * SLANT, y - 10.0, w + h * SLANT, h + 20.0],
        action,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Rgba = [1.0; 4];

    #[test]
    fn clip_keeps_inside_and_cuts_outside() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let inner = [[2.0, 2.0], [4.0, 2.0], [4.0, 4.0]].map(|p| (p, W));
        assert_eq!(clip(&inner, &square), inner.to_vec());
        let wide = [[-5.0, 2.0], [15.0, 2.0], [15.0, 4.0], [-5.0, 4.0]].map(|p| (p, W));
        let cut = clip(&wide, &square);
        assert!(cut.iter().all(|(p, _)| (0.0..=10.0).contains(&p[0])));
        assert_eq!(cut.len(), 4);
        let outside = [[20.0, 20.0], [30.0, 20.0], [30.0, 30.0]].map(|p| (p, W));
        assert!(clip(&outside, &square).is_empty());
    }

    #[test]
    fn clip_interpolates_colours() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let black = [0.0, 0.0, 0.0, 1.0];
        let bar = [
            ([0.0, 0.0], black),
            ([20.0, 0.0], W),
            ([20.0, 1.0], W),
            ([0.0, 1.0], black),
        ];
        let cut = clip(&bar, &square);
        let at_edge = cut.iter().find(|(p, _)| p[0] == 10.0).unwrap().1;
        assert_eq!(at_edge, [0.5, 0.5, 0.5, 1.0]);
    }

    /// The portrait frame at `scale` pixels per unit, top-left at the window origin.
    fn frame(scale: f32) -> Canvas {
        Canvas::new(canvas::Canvas::new(CENTER.map(|c| c * scale), scale))
    }

    #[test]
    fn off_circle_elements_move_beside_the_circle_and_slide_with_it() {
        let c = Canvas::new(canvas::Canvas::fit(1920.0, 1200.0));
        let ring = wacca_render::layout::ring(1920.0, 1200.0);
        // On the circle: concentric with the play ring.
        let [x, y] = c.p(CENTER);
        assert!((x - ring.center[0]).abs() < 1e-3 && (y - ring.center[1]).abs() < 1e-3);
        // A label above the circle lands left of it, one below right of it.
        assert!(c.p([540.0, 100.0])[0] < ring.center[0] - ring.radius);
        assert!(c.p([540.0, 1800.0])[0] > ring.center[0] + ring.radius);
        // A slide-in moving an on-circle panel past the circle's square does not move it there.
        let header = c.moved([0.0, -300.0], 1.0).rect(330.0, 420.0, 10.0, 10.0);
        assert!((header.x - c.p([330.0, 120.0])[0]).abs() > 1.0);
        assert!((header.x - c.rect(330.0, 420.0, 10.0, 10.0).x).abs() < 1e-3);
    }

    #[test]
    fn panels_are_clipped_to_the_screen_circle() {
        let mut v = View::default();
        frame(1.0).panel(&mut v, &slab(0.0, 880.0, 2000.0, 40.0), &[W]);
        let crate::ui::Item::Poly { points, .. } = &v.items[0] else {
            panic!("{:?}", v.items)
        };
        for p in points {
            let d = (p[0] - CENTER[0]).hypot(p[1] - CENTER[1]);
            assert!(d <= RADIUS + 0.01, "{p:?}");
        }
    }

    #[test]
    fn slab_leans_left_and_ease_settles() {
        let s = slab(100.0, 0.0, 50.0, 10.0);
        assert!(s[3][0] < s[0][0] && s[2][1] == 10.0);
        assert_eq!(ease(0.0, 1.0, 2.0), 0.0);
        assert_eq!(ease(5.0, 1.0, 2.0), 1.0);
        assert!(ease(2.0, 1.0, 2.0) > 0.5);
    }

    #[test]
    fn moved_canvas_shifts_and_fades() {
        let c = frame(0.5).moved([100.0, 0.0], 0.5);
        assert_eq!(c.p([440.0, 0.0]), [270.0, 0.0]);
        let mut v = View::default();
        c.image(&mut v, font_tex("x"), [540.0, 960.0], [100.0, 50.0], 1.0);
        let crate::ui::Item::Image {
            key, tint, rect, ..
        } = &v.items[0]
        else {
            panic!()
        };
        assert_eq!((key.as_str(), tint[3]), ("UI/Textures/FONT/x", 0.5));
        assert_eq!(rect.w, 50.0);
    }

    #[test]
    fn tiled_texture_repeats_and_maps_uvs() {
        let mut v = View::default();
        // 300 wide around the centre, tiles 200 wide from x = 400: [400, 600) and [600, 800).
        let poly = [
            [450.0, 880.0],
            [750.0, 880.0],
            [750.0, 920.0],
            [450.0, 920.0],
        ];
        frame(1.0).tiled(
            &mut v,
            "k".into(),
            &poly,
            [400.0, 880.0, 200.0, 40.0],
            true,
            1.0,
        );
        assert_eq!(v.items.len(), 2);
        for item in &v.items {
            let Item::ImagePoly { points, uvs, .. } = item else {
                panic!("{item:?}")
            };
            assert_eq!(points.len(), uvs.len());
            for (p, uv) in points.iter().zip(uvs) {
                let x = if p[0] < 600.0 || (p[0] == 600.0 && uv[0] == 0.0) {
                    400.0
                } else {
                    600.0
                };
                // flipped: u = 1 - (x - tile x) / w
                assert!(
                    (uv[0] - (1.0 - (p[0] - x) / 200.0)).abs() < 1e-4,
                    "{p:?} {uv:?}"
                );
                assert!((uv[1] - (p[1] - 880.0) / 40.0).abs() < 1e-4);
            }
        }
    }
}
