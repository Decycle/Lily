//! Pure playfield geometry (no GPU). Spec: `docs/research/04-gameplay-mechanics.md` §2,
//! `docs/research/01-chart-format.md` §3–5.3.
//!
//! Playfield space: centre `(0, 0)`, judgement line (rim) at radius 1, y up. Lane 0 starts at
//! 3 o'clock and lanes increase counter-clockwise (UNVERIFIED on screen, see `wacca_chart::model`).
//!
//! Depth follows the chart doc: 0 = on the judgement line, 1 = spawn point at the centre.

use std::f32::consts::TAU;

use wacca_chart::{HoldNode, LANES, MaskDir, MaskEvent, ReverseSection, ScrollSegment};

/// Seconds a note is visible at note speed 1.0.
// UNVERIFIED: community values are 3.2667 s and 3.3333 s (04 §2.2).
pub const APPROACH_SECS: f64 = 3.3;
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
    pub color: [f32; 4],
}

/// Axis-aligned rectangle in pixels, origin top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Perspective radius for a depth (04 §2.2): `r(x) = 3.325x / (13.825 − 10.5x)` with `x = 1 − depth`.
pub fn depth_to_radius(depth: f32) -> f32 {
    let x = 1.0 - depth.clamp(0.0, 1.0);
    3.325 * x / (13.825 - 10.5 * x)
}

/// Scroll distance `D(t)` (01 §5.3). Before the first segment the speed is 1.0.
pub fn scroll_distance(scroll: &[ScrollSegment], t: f64) -> f64 {
    match scroll.partition_point(|s| s.start_sec <= t) {
        0 => t,
        i => {
            let s = &scroll[i - 1];
            s.start_dist + (t - s.start_sec) * s.speed
        }
    }
}

/// Unclamped depth of chart time `t` when the scroll position is `now_dist` (= `D(now)`).
pub fn raw_depth(scroll: &[ScrollSegment], t: f64, now_dist: f64, note_speed: f64) -> f64 {
    (scroll_distance(scroll, t) - now_dist) * note_speed / APPROACH_SECS
}

/// Depth of a note at `note_time` seen at `now`, or `None` outside the visible window `0..=1`.
pub fn note_depth(
    note_time: f64,
    now: f64,
    scroll: &[ScrollSegment],
    note_speed: f64,
) -> Option<f32> {
    let d = raw_depth(scroll, note_time, scroll_distance(scroll, now), note_speed);
    (0.0..=1.0).contains(&d).then_some(d as f32)
}

/// Reverse gimmick (01 §5.3, MP behaviour, UNVERIFIED): returns the visual "now" and, inside a
/// reverse section, the last note time that may be drawn.
pub fn reverse_now(reverses: &[ReverseSection], now: f64) -> (f64, Option<f64>) {
    match reverses.iter().find(|r| r.start < now && now < r.end) {
        Some(r) => {
            let t = r.end + (r.end - now) * (r.note_end - r.end) / (r.end - r.start);
            (t, Some(r.note_end))
        }
        None => (now, None),
    }
}

/// Angle in radians (counter-clockwise from +x) of a (possibly fractional / unwrapped) lane edge.
pub fn lane_angle(lane: f32) -> f32 {
    lane * TAU / LANES as f32
}

pub fn polar(radius: f32, lane: f32) -> [f32; 2] {
    let a = lane_angle(lane);
    [radius * a.cos(), radius * a.sin()]
}

fn push_quad(out: &mut Vec<Vertex>, p: [[f32; 2]; 4], color: [f32; 4]) {
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(Vertex { pos: p[i], color });
    }
}

/// Annular sector covering lanes `lane .. lane + lanes` (no wrap handling needed: angles are
/// continuous, so e.g. lane 58 width 4 covers 58..62 ≡ 58,59,0,1). One quad per lane or less.
pub fn push_sector(
    out: &mut Vec<Vertex>,
    r_in: f32,
    r_out: f32,
    lane: f32,
    lanes: f32,
    color: [f32; 4],
) {
    push_strip(out, (r_in, lane, lanes), (r_out, lane, lanes), color);
}

/// Strip between two arcs `(radius, start lane, width in lanes)`, subdivided per lane.
pub fn push_strip(out: &mut Vec<Vertex>, a: (f32, f32, f32), b: (f32, f32, f32), color: [f32; 4]) {
    let steps = a.2.max(b.2).ceil().max(1.0) as usize;
    let at = |(r, lane, w): (f32, f32, f32), i: usize| polar(r, lane + w * i as f32 / steps as f32);
    for i in 0..steps {
        push_quad(out, [at(a, i), at(b, i), at(b, i + 1), at(a, i + 1)], color);
    }
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
        color,
    );
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
    color: [f32; 4],
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
            push_strip(out, row(u0), row(u1), color);
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

/// Square viewport for the playfield: as wide as the letterbox and centred in it.
pub fn playfield_viewport(w: f32, h: f32) -> Rect {
    let b = letterbox(w, h);
    Rect {
        x: b.x,
        y: b.y + (b.h - b.w) / 2.0,
        w: b.w,
        h: b.w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{Arc, BeatPos};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
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
        assert!(close(depth_to_radius(0.0), 1.0));
        assert!(close(depth_to_radius(1.0), 0.0));
        let rs: Vec<f32> = (0..=100)
            .map(|i| depth_to_radius(i as f32 / 100.0))
            .collect();
        assert!(rs.windows(2).all(|w| w[1] < w[0]));
        // Perspective: halfway in depth is much closer to the centre than halfway in radius.
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
        let v = APPROACH_SECS;
        assert_eq!(note_depth(10.0, 10.0, &[], 1.0), Some(0.0));
        assert!(close(
            note_depth(10.0 + v / 2.0, 10.0, &[], 1.0).unwrap(),
            0.5
        ));
        assert_eq!(note_depth(10.0 + v + 0.01, 10.0, &[], 1.0), None);
        assert_eq!(note_depth(9.99, 10.0, &[], 1.0), None);
        // Speed 2 halves the window.
        assert!(close(
            note_depth(10.0 + v / 4.0, 10.0, &[], 2.0).unwrap(),
            0.5
        ));
    }

    #[test]
    fn depth_during_stop_is_frozen() {
        // hs = 1 until 5 s, stop until 7 s, then 1 again.
        let scroll = [seg(0.0, 0.0, 1.0), seg(5.0, 5.0, 0.0), seg(7.0, 5.0, 1.0)];
        let a = note_depth(8.0, 5.5, &scroll, 1.0).unwrap();
        let b = note_depth(8.0, 6.5, &scroll, 1.0).unwrap();
        assert_eq!(a, b);
        assert!(close(a, (1.0 / APPROACH_SECS) as f32));
    }

    #[test]
    fn negative_hi_speed_recedes() {
        // hs = -1 from 5 s: D decreases, so a note at 10 s moves *away* while time passes.
        let scroll = [seg(0.0, 0.0, 1.0), seg(5.0, 5.0, -1.0)];
        let d6 = note_depth(5.5, 6.0, &scroll, 1.0).unwrap();
        let d7 = note_depth(5.5, 7.0, &scroll, 1.0).unwrap();
        assert!(d7 > d6);
    }

    #[test]
    fn reverse_maps_time_backwards() {
        let r = [ReverseSection {
            start: 10.0,
            end: 12.0,
            note_end: 14.0,
        }];
        assert_eq!(reverse_now(&r, 9.0), (9.0, None));
        assert_eq!(reverse_now(&r, 11.0), (13.0, Some(14.0)));
        let (t, _) = reverse_now(&r, 11.9);
        assert!((t - 12.1).abs() < 1e-9);
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
        push_hold_body(&mut v, &nodes, |t| t / 2.0, [1.0; 4]);
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
        push_hold_body(&mut v, &nodes, |t| t - 1.0, [1.0; 4]);
        let r: Vec<f32> = v.iter().map(|p| p.pos[0].hypot(p.pos[1])).collect();
        assert!(r.iter().all(|r| (-1e-4..=1.0 + 1e-4).contains(r)));
        assert!(r.iter().any(|r| close(*r, 1.0)) && r.iter().any(|r| close(*r, 0.0)));
        // Entirely past the line: nothing.
        let mut v = Vec::new();
        push_hold_body(&mut v, &nodes, |t| t - 10.0, [1.0; 4]);
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
        // Playfield square sits in the middle of the letterbox.
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
}
