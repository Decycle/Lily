//! Pure playfield geometry (no GPU). Spec: `docs/research/04-gameplay-mechanics.md` §2,
//! `docs/research/01-chart-format.md` §3–5.3.
//!
//! Playfield space: centre `(0, 0)`, judgement line (rim) at radius 1, y up. Lane 0 starts at
//! 3 o'clock and lanes increase counter-clockwise (UNVERIFIED on screen, see `wacca_chart::model`).
//!
//! Depth follows the chart doc: 0 = on the judgement line, 1 = spawn point at the centre.

use std::f32::consts::TAU;

use wacca_chart::{Chart, HoldNode, LANES, MaskDir, MaskEvent};

use crate::camera;
use crate::style::{SENSOR_DEPTHS, SENSOR_INNER, SENSOR_OUTER};

/// Seconds from spawn (scale 0) to the judgement line (scale 1) at note speed 1.0: exactly 10/3 s
/// (RenderDoc note matrices, doc 11 §2.4: ds/dt = 0.45/s at speed 1.5). E2's 3.29 s came from
/// fitting the note's outer edge with a 2D curve (doc 09); see [`crate::camera`].
pub const APPROACH_SECS: f64 = 10.0 / 3.0;
/// Mask sweep: one lane per `1000/60/2` ms (01 §4).
pub const MASK_SECS_PER_LANE: f64 = 1.0 / 120.0;
/// Hold bodies are sampled at this time step between nodes (bounded by [`HOLD_MAX_SAMPLES`]).
pub const HOLD_SAMPLE_SECS: f64 = 1.0 / 60.0;
pub const HOLD_MAX_SAMPLES: usize = 64;
/// Window shape (portrait 9:16).
pub const ASPECT_W: f32 = 9.0;
pub const ASPECT_H: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 2],
    /// Sprite atlas coordinates ([`WHITE_UV`] for flat colour).
    pub uv: [f32; 2],
    /// sRGB, straight alpha.
    pub color: [f32; 4],
    /// `[mask, additive]` flags (0 or 1), see [`Paint`].
    pub fx: [f32; 2],
}

/// Atlas rectangle `[u0, v0, u1, v1]`.
pub type UvRect = [f32; 4];

/// Side of the sprite atlas texture; its top-left [`WHITE_BLOCK`] texels are opaque white.
pub const ATLAS_SIZE: u32 = 2048;
pub const WHITE_BLOCK: u32 = 4;
/// Centre of the white block: sampling it gives the plain vertex colour.
pub const WHITE_UV: [f32; 2] = [
    WHITE_BLOCK as f32 / 2.0 / ATLAS_SIZE as f32,
    WHITE_BLOCK as f32 / 2.0 / ATLAS_SIZE as f32,
];

/// How a strip is filled: `color` times the atlas region `uv`.
///
/// `mask`: the texel is a baked mask (`r` = white highlight, `g` = shade of `color`, `a` =
/// coverage) instead of a plain image. `additive`: added to what is below (glows).
/// `along_v`: the texture's v axis runs along the arc (u radial) instead of u.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub color: [f32; 4],
    pub uv: UvRect,
    pub mask: bool,
    pub additive: bool,
    pub along_v: bool,
}

impl Paint {
    pub fn flat(color: [f32; 4]) -> Self {
        let [u, v] = WHITE_UV;
        Self::image(color, [u, v, u, v])
    }

    pub fn image(color: [f32; 4], uv: UvRect) -> Self {
        Self {
            color,
            uv,
            mask: false,
            additive: false,
            along_v: false,
        }
    }

    pub fn mask(color: [f32; 4], uv: UvRect) -> Self {
        Self {
            mask: true,
            ..Self::image(color, uv)
        }
    }

    pub fn additive(self) -> Self {
        Self {
            additive: true,
            ..self
        }
    }

    pub fn along_v(self) -> Self {
        Self {
            along_v: true,
            ..self
        }
    }

    /// Texture coordinate at `s` along the arc and `t` across it (both `0..=1`).
    pub fn uv_at(&self, s: f32, t: f32) -> [f32; 2] {
        let [u0, v0, u1, v1] = self.uv;
        let (su, sv) = if self.along_v { (t, s) } else { (s, t) };
        [u0 + (u1 - u0) * su, v0 + (v1 - v0) * sv]
    }

    /// The same paint restricted to the arc fraction `s0..s1` of its region.
    pub fn sub_arc(&self, s0: f32, s1: f32) -> Self {
        let ([a0, b0], [a1, b1]) = (self.uv_at(s0, 0.0), self.uv_at(s1, 1.0));
        Self {
            uv: [a0, b0, a1, b1],
            ..*self
        }
    }

    fn vertex(&self, pos: [f32; 2], uv: [f32; 2]) -> Vertex {
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        Vertex {
            pos,
            uv,
            color: self.color,
            fx: [flag(self.mask), flag(self.additive)],
        }
    }
}

/// Axis-aligned rectangle in pixels, origin top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Screen radius of the hold-body circle (`camera::HOLD_RADIUS_CM` on the note plane) at `depth`
/// through the game's camera ([`crate::camera`]): 0 at spawn, ≈ 1.05 on the line (the note band,
/// 0.97–1.10, straddles the rim's inner edge at 1).
pub fn depth_to_radius(depth: f32) -> f32 {
    let s = camera::note_scale(depth.clamp(0.0, 1.0));
    camera::project_radius(s, camera::HOLD_RADIUS_CM, 0.0)
}

/// Unclamped depth of chart time `t` when the scroll position is `now_dist` (= `chart.dist_at(view now)`).
pub fn raw_depth(chart: &Chart, t: f64, now_dist: f64, note_speed: f64) -> f64 {
    (chart.dist_at(t) - now_dist) * note_speed / APPROACH_SECS
}

/// Depth of a note at `note_time` seen at chart time `now` (reverse-remapped via
/// [`Chart::view_time`]), or `None` outside the visible window `0..=1`.
pub fn note_depth(chart: &Chart, note_time: f64, now: f64, note_speed: f64) -> Option<f32> {
    let d = raw_depth(
        chart,
        note_time,
        chart.dist_at(chart.view_time(now)),
        note_speed,
    );
    (0.0..=1.0).contains(&d).then_some(d as f32)
}

/// Inside a reverse section only notes up to its `note_end` are drawn (01 §5.3, UNVERIFIED).
pub fn last_drawable_time(chart: &Chart, now: f64) -> Option<f64> {
    chart
        .reverses
        .iter()
        .find(|r| r.view_time(now).is_some())
        .map(|r| r.note_end)
}

/// Angle in radians (counter-clockwise from +x) of a (possibly fractional / unwrapped) lane edge.
pub fn lane_angle(lane: f32) -> f32 {
    lane * TAU / LANES as f32
}

pub fn polar(radius: f32, lane: f32) -> [f32; 2] {
    let a = lane_angle(lane);
    [radius * a.cos(), radius * a.sin()]
}

/// Quad `p` (corners in order) with texture coordinates `(s along, t across)` per corner.
fn push_quad(out: &mut Vec<Vertex>, p: [[f32; 2]; 4], st: [[f32; 2]; 4], paint: &Paint) {
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(paint.vertex(p[i], paint.uv_at(st[i][0], st[i][1])));
    }
}

/// Flat annular sector covering lanes `lane .. lane + lanes` (no wrap handling needed: angles
/// are continuous, so e.g. lane 58 width 4 covers 58..62 ≡ 58,59,0,1). One quad per lane or less.
pub fn push_sector(
    out: &mut Vec<Vertex>,
    r_in: f32,
    r_out: f32,
    lane: f32,
    lanes: f32,
    color: [f32; 4],
) {
    let paint = Paint::flat(color);
    push_strip(out, (r_in, lane, lanes), (r_out, lane, lanes), &paint);
}

/// Painted sector from radius `r_a` (texture `t = 0`) to `r_b` (`t = 1`); the texture's arc
/// axis runs from `lane` to `lane + lanes`.
pub fn paint_sector(
    out: &mut Vec<Vertex>,
    r_a: f32,
    r_b: f32,
    lane: f32,
    lanes: f32,
    paint: &Paint,
) {
    push_strip(out, (r_a, lane, lanes), (r_b, lane, lanes), paint);
}

/// Strip between two arcs `(radius, start lane, width in lanes)`, subdivided per lane; the
/// texture runs along the arcs (`s`) and from `a` to `b` (`t`).
pub fn push_strip(out: &mut Vec<Vertex>, a: (f32, f32, f32), b: (f32, f32, f32), paint: &Paint) {
    let steps = a.2.max(b.2).ceil().max(1.0) as usize;
    let at = |(r, lane, w): (f32, f32, f32), i: usize| polar(r, lane + w * i as f32 / steps as f32);
    let s = |i: usize| i as f32 / steps as f32;
    for i in 0..steps {
        push_quad(
            out,
            [at(a, i), at(b, i), at(b, i + 1), at(a, i + 1)],
            [[s(i), 0.0], [s(i), 1.0], [s(i + 1), 1.0], [s(i + 1), 0.0]],
            paint,
        );
    }
}

/// Axis-aligned sprite centred at `c` with half extents `half` (playfield units, y up); the
/// texture's top edge is at the top.
pub fn push_rect(out: &mut Vec<Vertex>, c: [f32; 2], half: [f32; 2], paint: &Paint) {
    let (x0, x1, y0, y1) = (
        c[0] - half[0],
        c[0] + half[0],
        c[1] - half[1],
        c[1] + half[1],
    );
    push_quad(
        out,
        [[x0, y1], [x0, y0], [x1, y0], [x1, y1]],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        paint,
    );
}

/// Sprite centred at `c` and turned so that the texture's +v axis points along `dir` (any
/// non-zero length): `half = [along, across]` half extents, u runs across.
pub fn push_oriented_rect(
    out: &mut Vec<Vertex>,
    c: [f32; 2],
    half: [f32; 2],
    dir: [f32; 2],
    paint: &Paint,
) {
    let len = dir[0].hypot(dir[1]).max(1e-9);
    let d = [dir[0] / len, dir[1] / len];
    let t = [d[0] * half[0], d[1] * half[0]];
    let n = [-d[1] * half[1], d[0] * half[1]];
    let p = |a: f32, b: f32| [c[0] + t[0] * a + n[0] * b, c[1] + t[1] * a + n[1] * b];
    push_quad(
        out,
        [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        &Paint {
            along_v: false,
            ..*paint
        },
    );
}

/// Thin radial line along a lane edge from `r0` to `r1`.
pub fn push_radial_line(
    out: &mut Vec<Vertex>,
    lane: f32,
    r0: f32,
    r1: f32,
    half_width: f32,
    color: [f32; 4],
) {
    let a = lane_angle(lane);
    let (dir, n) = (
        [a.cos(), a.sin()],
        [-a.sin() * half_width, a.cos() * half_width],
    );
    let p = |r: f32, s: f32| [dir[0] * r + n[0] * s, dir[1] * r + n[1] * s];
    push_quad(
        out,
        [p(r0, -1.0), p(r1, -1.0), p(r1, 1.0), p(r0, 1.0)],
        [[0.0; 2]; 4],
        &Paint::flat(color),
    );
}

/// Pieces of a span `0..span` cut into repeating tiles of length `len` shifted by `phase`
/// (fraction of a tile): `(offset, length, s0, s1)` with `s0..s1` the covered part of the tile.
pub fn tiles(span: f32, len: f32, phase: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut out = Vec::new();
    if span <= 0.0 || len <= 0.0 {
        return out;
    }
    let mut start = -phase.rem_euclid(1.0) * len;
    while start < span {
        let (a, b) = (start.max(0.0), (start + len).min(span));
        // Skip slivers left by rounding.
        if b - a > 1e-4 {
            out.push((a, b - a, (a - start) / len, (b - start) / len));
        }
        start += len;
    }
    out
}

/// Three-slice split of an arc of `span` lanes: fixed-size end caps of `cap` lanes (shrunk to a
/// third of short arcs) mapped to the texture's outer `cap_s` fraction, the middle stretched.
/// Returns `(offset, length, s0, s1)` per piece.
pub fn three_slice(span: f32, cap: f32, cap_s: f32) -> [(f32, f32, f32, f32); 3] {
    let c = cap.min(span / 3.0);
    [
        (0.0, c, 0.0, cap_s),
        (c, span - 2.0 * c, cap_s, 1.0 - cap_s),
        (span - c, c, 1.0 - cap_s, 1.0),
    ]
}

/// Sub-interval `[u0, u1]` of `0..=1` where the linear depth from `d0` to `d1` lies in `0..=1`.
fn clip_visible(d0: f64, d1: f64) -> Option<(f64, f64)> {
    if d0 == d1 {
        return (0.0..=1.0).contains(&d0).then_some((0.0, 1.0));
    }
    let (ua, ub) = ((0.0 - d0) / (d1 - d0), (1.0 - d0) / (d1 - d0));
    let (lo, hi) = (ua.min(ub).max(0.0), ua.max(ub).min(1.0));
    (lo < hi).then_some((lo, hi))
}

/// Hold body between consecutive nodes, interpolating unwrapped lane and width linearly in time
/// (01 §3, UNVERIFIED: the game may interpolate differently). `depth_of(t)` is the unclamped depth.
/// Node pairs entirely on one side of the visible window are culled.
pub fn push_hold_body(
    out: &mut Vec<Vertex>,
    nodes: &[HoldNode],
    depth_of: impl Fn(f64) -> f64,
    paint: &Paint,
) {
    for pair in nodes.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (da, db) = (depth_of(a.time), depth_of(b.time));
        if (da < 0.0 && db < 0.0) || (da > 1.0 && db > 1.0) || a.time >= b.time {
            continue;
        }
        let n = (((b.time - a.time) / HOLD_SAMPLE_SECS).ceil() as usize).clamp(1, HOLD_MAX_SAMPLES);
        // Sample `s` in 0..=1 along the node pair -> (time, lane, width).
        let at = |s: f64| {
            let lerp = |x: f32, y: f32| x + (y - x) * s as f32;
            let w = lerp(a.arc.width as f32, b.arc.width as f32);
            (
                a.time + (b.time - a.time) * s,
                lerp(a.lane_unwrapped, b.lane_unwrapped),
                w,
            )
        };
        let mut prev = (0.0, da);
        for k in 1..=n {
            let s1 = k as f64 / n as f64;
            let d1 = if k == n { db } else { depth_of(at(s1).0) };
            let (s0, d0) = prev;
            prev = (s1, d1);
            let Some((u0, u1)) = clip_visible(d0, d1) else {
                continue;
            };
            let row = |u: f64| {
                let (_, lane, w) = at(s0 + (s1 - s0) * u);
                (depth_to_radius((d0 + (d1 - d0) * u) as f32), lane, w)
            };
            push_strip(out, row(u0), row(u1), paint);
        }
    }
}

/// Lane on/off state at time `t` (01 §4). Lanes start off, or all on if the chart has no
/// MaskAdd events (UNVERIFIED). Each event sweeps one lane per [`MASK_SECS_PER_LANE`];
/// `Center` sweeps from the middle outward for both add and remove (MM behaviour, UNVERIFIED).
/// `masks` must be sorted by time.
pub fn mask_state(masks: &[MaskEvent], t: f64) -> [bool; LANES as usize] {
    let initial = !masks.iter().any(|m| m.add);
    let mut on = [initial; LANES as usize];
    let mut changed_at = [f64::NEG_INFINITY; LANES as usize];
    for m in masks.iter().take_while(|m| m.time <= t) {
        let w = m.arc.width as usize;
        for i in 0..w {
            let order = match m.dir {
                MaskDir::Plus => i,
                MaskDir::Minus => w - 1 - i,
                MaskDir::Center => (2 * i).abs_diff(w - 1) / 2,
            };
            let at = m.time + order as f64 * MASK_SECS_PER_LANE;
            let lane = (m.arc.lane as usize + i) % LANES as usize;
            if at <= t && at >= changed_at[lane] {
                on[lane] = m.add;
                changed_at[lane] = at;
            }
        }
    }
    on
}

/// Largest 9:16 rectangle centred in a `w × h` window.
pub fn letterbox(w: f32, h: f32) -> Rect {
    let (bw, bh) = if w * ASPECT_H > h * ASPECT_W {
        (h * ASPECT_W / ASPECT_H, h)
    } else {
        (w, w * ASPECT_H / ASPECT_W)
    };
    Rect {
        x: (w - bw) / 2.0,
        y: (h - bh) / 2.0,
        w: bw,
        h: bh,
    }
}

/// Square viewport for the playfield: the largest square centred in the `w × h` window (full
/// height on a landscape screen). Touch input maps through the same rectangle.
pub fn playfield_viewport(w: f32, h: f32) -> Rect {
    let side = w.min(h);
    Rect {
        x: (w - side) / 2.0,
        y: (h - side) / 2.0,
        w: side,
        h: side,
    }
}

/// Inner and outer radius (playfield units) of sensor depth ring `depth` (0 = innermost).
pub fn sensor_ring(depth: usize) -> (f32, f32) {
    let step = (SENSOR_OUTER - SENSOR_INNER) / SENSOR_DEPTHS as f32;
    let inner = SENSOR_INNER + step * depth as f32;
    (inner, inner + step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{Arc, BeatPos, ReverseSection, ScrollSegment};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// Chart with the given scroll segments and nothing else.
    fn chart(scroll: Vec<ScrollSegment>) -> Chart {
        Chart {
            header: Default::default(),
            notes: vec![],
            masks: vec![],
            tempo: vec![],
            scroll,
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 10.0,
        }
    }

    fn seg(start_sec: f64, start_dist: f64, speed: f64) -> ScrollSegment {
        ScrollSegment {
            start_sec,
            start_dist,
            speed,
        }
    }

    #[test]
    fn radius_curve_endpoints_and_monotonic() {
        // 172.5 cm at 190 cm from the camera, NDC 1 = VIEW_EXTENT.
        let e = crate::style::VIEW_EXTENT;
        assert!(close(depth_to_radius(0.0), e * 172.5 / 190.0));
        assert!(close(depth_to_radius(1.0), 0.0));
        let rs: Vec<f32> = (0..=100)
            .map(|i| depth_to_radius(i as f32 / 100.0))
            .collect();
        assert!(rs.windows(2).all(|w| w[1] < w[0]));
        // Perspective: halfway in time is much closer to the centre than halfway in radius
        // (s 0.5: 86.25 cm at 490 cm).
        assert!(close(depth_to_radius(0.5), e * 86.25 / 490.0));
        assert!(depth_to_radius(0.5) < 0.25);
    }

    #[test]
    fn lane_angles_ccw_from_three_oclock() {
        let p0 = polar(1.0, 0.0);
        assert!(close(p0[0], 1.0) && close(p0[1], 0.0));
        let top = polar(1.0, 15.0);
        assert!(close(top[0], 0.0) && close(top[1], 1.0));
        let left = polar(1.0, 30.0);
        assert!(close(left[0], -1.0));
        assert!(
            polar(1.0, 1.0)[1] > 0.0,
            "lane 1 is counter-clockwise (above) of lane 0"
        );
    }

    #[test]
    fn depth_window_constant_speed() {
        let (v, c) = (APPROACH_SECS, chart(vec![]));
        assert_eq!(note_depth(&c, 10.0, 10.0, 1.0), Some(0.0));
        assert!(close(
            note_depth(&c, 10.0 + v / 2.0, 10.0, 1.0).unwrap(),
            0.5
        ));
        assert_eq!(note_depth(&c, 10.0 + v + 0.01, 10.0, 1.0), None);
        assert_eq!(note_depth(&c, 9.99, 10.0, 1.0), None);
        // Speed 2 halves the window.
        assert!(close(
            note_depth(&c, 10.0 + v / 4.0, 10.0, 2.0).unwrap(),
            0.5
        ));
    }

    #[test]
    fn depth_during_stop_is_frozen() {
        // hs = 1 until 5 s, stop until 7 s, then 1 again.
        let c = chart(vec![
            seg(0.0, 0.0, 1.0),
            seg(5.0, 5.0, 0.0),
            seg(7.0, 5.0, 1.0),
        ]);
        let a = note_depth(&c, 8.0, 5.5, 1.0).unwrap();
        let b = note_depth(&c, 8.0, 6.5, 1.0).unwrap();
        assert_eq!(a, b);
        assert!(close(a, (1.0 / APPROACH_SECS) as f32));
    }

    #[test]
    fn negative_hi_speed_recedes() {
        // hs = -1 from 5 s: D decreases, so a note moves *away* while time passes.
        let c = chart(vec![seg(0.0, 0.0, 1.0), seg(5.0, 5.0, -1.0)]);
        let d6 = note_depth(&c, 5.5, 6.0, 1.0).unwrap();
        let d7 = note_depth(&c, 5.5, 7.0, 1.0).unwrap();
        assert!(d7 > d6);
    }

    #[test]
    fn reverse_remaps_depth_and_limits_notes() {
        let mut c = chart(vec![]);
        c.reverses = vec![ReverseSection {
            start: 10.0,
            end: 12.0,
            note_end: 14.0,
        }];
        // At 11 s the view time is 13 s: a note at 13 s sits on the line.
        assert_eq!(note_depth(&c, 13.0, 11.0, 1.0), Some(0.0));
        assert_eq!(last_drawable_time(&c, 11.0), Some(14.0));
        assert_eq!(last_drawable_time(&c, 9.0), None);
    }

    #[test]
    fn sector_wraps_across_lane_zero() {
        let mut v = Vec::new();
        push_sector(&mut v, 0.5, 1.0, 58.0, 4.0, [1.0; 4]);
        assert_eq!(v.len(), 4 * 6);
        let angles: Vec<i32> = v
            .iter()
            .map(|p| (p.pos[1].atan2(p.pos[0]).to_degrees().round() as i32).rem_euclid(360))
            .collect();
        for a in [348, 354, 0, 6, 12] {
            assert!(angles.contains(&a), "missing edge at {a} deg");
        }
        assert!(angles.iter().all(|a| *a >= 348 || *a <= 12));
    }

    fn node(time: f64, lane: u8, width: u8, lane_unwrapped: f32) -> HoldNode {
        HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width },
            lane_unwrapped,
        }
    }

    #[test]
    fn hold_strip_continuous_across_wrap() {
        // 58 -> 61 (= lane 1), width 2: every quad edge stays near lanes 58..63, never jumps around.
        let nodes = [node(0.0, 58, 2, 58.0), node(1.0, 1, 2, 61.0)];
        let mut v = Vec::new();
        push_hold_body(&mut v, &nodes, |t| t / 2.0, &Paint::flat([1.0; 4]));
        assert!(!v.is_empty());
        for tri in v.chunks(3) {
            let a: Vec<f32> = tri.iter().map(|p| p.pos[1].atan2(p.pos[0])).collect();
            for i in 0..3 {
                let mut d = (a[i] - a[(i + 1) % 3]).abs();
                d = d.min(TAU - d);
                assert!(d < lane_angle(2.5), "triangle spans {d} rad");
            }
        }
    }

    #[test]
    fn hold_strip_clipped_to_window() {
        let nodes = [node(0.0, 0, 4, 0.0), node(4.0, 0, 4, 0.0)];
        let mut v = Vec::new();
        // depth = t - 1: visible part is t in 1..2.
        push_hold_body(&mut v, &nodes, |t| t - 1.0, &Paint::flat([1.0; 4]));
        let r: Vec<f32> = v.iter().map(|p| p.pos[0].hypot(p.pos[1])).collect();
        let line = depth_to_radius(0.0);
        assert!(r.iter().all(|r| (-1e-4..=line + 1e-4).contains(r)));
        assert!(r.iter().any(|r| close(*r, line)) && r.iter().any(|r| close(*r, 0.0)));
        // Entirely past the line: nothing.
        let mut v = Vec::new();
        push_hold_body(&mut v, &nodes, |t| t - 10.0, &Paint::flat([1.0; 4]));
        assert!(v.is_empty());
    }

    fn mask(time: f64, lane: u8, width: u8, add: bool, dir: MaskDir) -> MaskEvent {
        MaskEvent {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width },
            add,
            dir,
        }
    }

    #[test]
    fn mask_initial_state() {
        assert!(mask_state(&[], 0.0).iter().all(|&on| on));
        let m = [mask(1.0, 0, 60, true, MaskDir::Plus)];
        assert!(mask_state(&m, 0.0).iter().all(|&on| !on));
    }

    #[test]
    fn mask_sweep_timing() {
        let lane = MASK_SECS_PER_LANE;
        let plus = [mask(1.0, 58, 4, true, MaskDir::Plus)];
        let s = mask_state(&plus, 1.0 + lane * 1.5);
        assert!(s[58] && s[59] && !s[0] && !s[1]);
        assert!(mask_state(&plus, 1.0 + lane * 3.0)[1]);

        let minus = [mask(1.0, 10, 4, true, MaskDir::Minus)];
        let s = mask_state(&minus, 1.0 + lane * 0.5);
        assert!(s[13] && !s[12] && !s[10]);

        let center = [mask(1.0, 10, 5, true, MaskDir::Center)];
        let s = mask_state(&center, 1.0 + lane * 1.0);
        assert_eq!(&s[9..16], &[false, false, true, true, true, false, false]);

        // Width 2 centre appears instantly.
        let s = mask_state(&[mask(1.0, 0, 2, true, MaskDir::Center)], 1.0);
        assert!(s[0] && s[1]);
    }

    #[test]
    fn mask_remove_after_add() {
        let m = [
            mask(0.0, 0, 60, true, MaskDir::Plus),
            mask(2.0, 0, 10, false, MaskDir::Plus),
        ];
        let s = mask_state(&m, 3.0);
        assert!(s[..10].iter().all(|&on| !on) && s[10..].iter().all(|&on| on));
    }

    #[test]
    fn letterbox_math() {
        // Landscape: full height, centred horizontally.
        let r = letterbox(1920.0, 1200.0);
        assert_eq!(
            r,
            Rect {
                x: (1920.0 - 675.0) / 2.0,
                y: 0.0,
                w: 675.0,
                h: 1200.0
            }
        );
        // Very tall: full width, centred vertically.
        let r = letterbox(900.0, 2000.0);
        assert_eq!(
            r,
            Rect {
                x: 0.0,
                y: 200.0,
                w: 900.0,
                h: 1600.0
            }
        );
        // Playfield square: full width in portrait, full height in landscape.
        let p = playfield_viewport(1920.0, 1200.0);
        assert_eq!(
            p,
            Rect {
                x: 360.0,
                y: 0.0,
                w: 1200.0,
                h: 1200.0
            }
        );
        let p = playfield_viewport(900.0, 1600.0);
        assert_eq!(
            p,
            Rect {
                x: 0.0,
                y: 350.0,
                w: 900.0,
                h: 900.0
            }
        );
    }

    #[test]
    fn sensor_rings_split_the_band_evenly() {
        assert_eq!(sensor_ring(0).0, SENSOR_INNER);
        assert!((sensor_ring(SENSOR_DEPTHS - 1).1 - SENSOR_OUTER).abs() < 1e-6);
        assert!((sensor_ring(1).0 - sensor_ring(0).1).abs() < 1e-6);
    }

    #[test]
    fn tiles_cover_the_span_with_phase() {
        let t = tiles(5.0, 2.0, 0.0);
        assert_eq!(
            t,
            vec![
                (0.0, 2.0, 0.0, 1.0),
                (2.0, 2.0, 0.0, 1.0),
                (4.0, 1.0, 0.0, 0.5)
            ]
        );
        // Shifted by a quarter tile: the first piece is the tail of a tile.
        let t = tiles(4.0, 2.0, 0.25);
        assert_eq!(t[0], (0.0, 1.5, 0.25, 1.0));
        assert!((t.iter().map(|p| p.1).sum::<f32>() - 4.0).abs() < 1e-6);
        // Phase wraps.
        assert_eq!(tiles(4.0, 2.0, 1.25), t);
        assert_eq!(tiles(4.0, 2.0, -0.75), t);
        assert!(tiles(0.0, 2.0, 0.0).is_empty());
    }

    #[test]
    fn three_slice_keeps_caps_fixed() {
        let [a, b, c] = three_slice(10.0, 0.5, 0.1);
        assert_eq!(a, (0.0, 0.5, 0.0, 0.1));
        assert_eq!(b, (0.5, 9.0, 0.1, 0.9));
        assert_eq!(c, (9.5, 0.5, 0.9, 1.0));
        // Short arcs: caps shrink to a third each.
        let [a, b, _] = three_slice(0.9, 0.5, 0.1);
        assert!((a.1 - 0.3).abs() < 1e-6 && (b.1 - 0.3).abs() < 1e-6);
    }

    #[test]
    fn paint_maps_arc_and_radius_to_uv() {
        let p = Paint::image([1.0; 4], [0.2, 0.4, 0.6, 0.8]);
        assert_eq!(p.uv_at(0.0, 0.0), [0.2, 0.4]);
        assert_eq!(p.uv_at(1.0, 1.0), [0.6, 0.8]);
        assert_eq!(p.along_v().uv_at(1.0, 0.0), [0.2, 0.8]);
        let sub = p.sub_arc(0.5, 1.0);
        assert!((sub.uv[0] - 0.4).abs() < 1e-6 && sub.uv[1] == 0.4 && sub.uv[2] == 0.6);
        let sub = p.along_v().sub_arc(0.0, 0.5);
        assert!(sub.uv[0] == 0.2 && sub.uv[2] == 0.6 && (sub.uv[3] - 0.6).abs() < 1e-6);
        // A flipped region (reversed v) stays flipped.
        let flipped = Paint::image([1.0; 4], [0.0, 1.0, 1.0, 0.0]).along_v();
        assert_eq!(flipped.sub_arc(0.0, 0.25).uv, [0.0, 1.0, 1.0, 0.75]);
        let flat = Paint::flat([1.0; 4]);
        assert_eq!(flat.uv_at(0.3, 0.7), WHITE_UV);
    }

    #[test]
    fn painted_sector_uv_runs_along_arc_and_radius() {
        let mut v = Vec::new();
        let p = Paint::mask([1.0; 4], [0.0, 0.0, 1.0, 1.0]).additive();
        paint_sector(&mut v, 1.0, 0.5, 0.0, 2.0, &p);
        assert_eq!(v.len(), 2 * 6);
        for q in &v {
            let r = q.pos[0].hypot(q.pos[1]);
            // t = 0 at r_a = 1.0, t = 1 at r_b = 0.5.
            assert!((q.uv[1] - (1.0 - r) / 0.5).abs() < 1e-5);
            let lane = q.pos[1].atan2(q.pos[0]) / lane_angle(1.0);
            assert!((q.uv[0] - lane / 2.0).abs() < 1e-4);
            assert_eq!(q.fx, [1.0, 1.0]);
        }
    }

    #[test]
    fn rect_is_axis_aligned_with_texture_upright() {
        let mut v = Vec::new();
        push_rect(
            &mut v,
            [0.0, -0.5],
            [0.2, 0.1],
            &Paint::image([1.0; 4], [0.0, 0.0, 1.0, 1.0]),
        );
        assert_eq!(v.len(), 6);
        for q in &v {
            // v = 0 at the top (y = -0.4), 1 at the bottom (y = -0.6).
            assert!((q.uv[1] - (-0.4 - q.pos[1]) / 0.2).abs() < 1e-6);
            assert!((q.uv[0] - (q.pos[0] + 0.2) / 0.4).abs() < 1e-6);
        }
    }

    #[test]
    fn oriented_rect_points_v_along_the_direction() {
        let mut v = Vec::new();
        let paint = Paint::image([1.0; 4], [0.0, 0.0, 1.0, 1.0]).along_v();
        // Pointing down (−y): v grows downwards, u across (x).
        push_oriented_rect(&mut v, [1.0, 1.0], [0.3, 0.1], [0.0, -2.0], &paint);
        assert_eq!(v.len(), 6);
        for q in &v {
            assert!(
                (q.uv[1] - (1.0 - q.pos[1] + 0.3) / 0.6).abs() < 1e-6,
                "{q:?}"
            );
            assert!((q.pos[0] - 1.0).abs() <= 0.1 + 1e-6);
        }
        let front = v.iter().find(|q| q.uv[1] == 1.0).unwrap();
        assert!(close(front.pos[1], 0.7));
    }
}
