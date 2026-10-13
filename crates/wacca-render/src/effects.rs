//! Hit effects: a small deterministic CPU particle system approximating the Cascade systems of
//! the default touch effect (`TouchEffectTable` 312001: `vo_tap000_03/04` Marvelous pop/shoot,
//! `_05/06` Great, `_00` Good, `vo_hold000_*_lp` hold sustain; 06 §3), plus the key beams.
//!
//! Matched to the 120 fps E2 recordings (`runs/e2/*/video.mkv`, doc 09): on a judgement the
//! note's arc lights up as a sector from the line inwards (white at the line, then the
//! judgement's colour ramp `vT_Hit{Marv,Grate,Good}A00`), a narrower "shoot" wedge flies to the
//! centre, small sparks drift inwards and the rim over the arc flashes white; all gone after
//! ~0.25 s. Pressed sensors light their lanes (key beam) and fade over ~0.25 s after release.
//!
//! [`Effects`] is pure: spawn with [`Effects::hit`], advance with [`Effects::update`], draw with
//! [`Effects::draw`]. Randomness comes from a seeded xorshift, so runs are reproducible.

use wacca_chart::{HoldNode, LANES};

use crate::layout::{
    Paint, Vertex, paint_sector, polar, push_oriented_rect, push_sector, push_strip,
};
use crate::sprites::{JudgeSprite, Sprite, SpriteSheet};
use crate::style::Rgba;

/// Life of the pop sector (s); `vo_tap000_03` emitter lifetimes are 0.1–0.35 s.
pub const POP_SECS: f32 = 0.3;
/// Radial length of the pop sector (judgement line = 1).
pub const POP_LEN: f32 = 0.55;
/// White core at the line: radial length.
pub const CORE_LEN: f32 = 0.15;
/// Share of white mixed into the colour ramp.
pub const POP_WHITE: f32 = 0.35;
/// Part of the colour ramp the pop runs through over its life (the recordings stay on the
/// bright end).
pub const POP_RAMP_SPAN: f32 = 0.4;
/// Shoot wedge: life, distance travelled inwards, length, arc fraction.
pub const SHOOT_SECS: f32 = 0.12;
pub const SHOOT_DIST: f32 = 0.75;
pub const SHOOT_LEN: f32 = 0.35;
pub const SHOOT_ARC: f32 = 0.6;
/// Width of the wedge's head relative to its tail.
// UNVERIFIED: wedge taper eyeballed from the 120 fps recordings.
pub const SHOOT_TAPER: f32 = 0.35;
/// White band over the rim: life and radial extent outside the line.
pub const RIM_SECS: f32 = 0.22;
pub const RIM_WIDTH: f32 = 0.133;
/// Sparks: life range (s), inward speed range (radius / s), size (half extent).
pub const SPARK_LIFE: (f32, f32) = (0.2, 0.4);
pub const SPARK_SPEED: (f32, f32) = (0.3, 1.2);
pub const SPARK_SIZE: f32 = 0.012;
/// Hit sparks start between this radius and the line.
pub const SPARK_R_MIN: f32 = 0.6;
/// Key beam: fade after release (s), radial length, peak opacity, rim opacity.
pub const BEAM_FADE_SECS: f32 = 0.25;
pub const BEAM_LEN: f32 = 0.55;
pub const BEAM_ALPHA: f32 = 0.3;
pub const BEAM_RIM_ALPHA: f32 = 0.85;
/// Hold sustain: sparks per second per held hold, glow colour.
pub const HOLD_SPARK_RATE: f32 = 40.0;
pub const HOLD_GLOW: Rgba = [1.0, 0.65, 0.30, 0.6];

/// Colour ramp over the effect's life (t = 0 → 1), sampled from the bottom of the 1-pixel-wide
/// ramp textures to their top.
fn ramp(kind: JudgeSprite) -> [Rgba; 4] {
    let c = |r: u8, g: u8, b: u8| [r, g, b].map(|v| f32::from(v) / 255.0);
    let rgb = match kind {
        // vT_HitMarvA00_00_m
        JudgeSprite::Marvelous => [
            c(255, 172, 133),
            c(252, 118, 229),
            c(233, 95, 252),
            c(218, 87, 242),
        ],
        // vT_HitGrateA00_00_m
        JudgeSprite::Great => [
            c(186, 255, 159),
            c(108, 236, 79),
            c(12, 179, 36),
            c(0, 146, 65),
        ],
        // vT_HitGoodA00_00_m
        JudgeSprite::Good | JudgeSprite::Miss => [
            c(113, 237, 255),
            c(0, 209, 237),
            c(0, 132, 180),
            c(0, 96, 146),
        ],
    };
    rgb.map(|[r, g, b]| [r, g, b, 1.0])
}

/// Colour of `kind`'s ramp at life fraction `t`.
pub fn ramp_at(kind: JudgeSprite, t: f32) -> Rgba {
    let r = ramp(kind);
    let x = t.clamp(0.0, 1.0) * 3.0;
    let i = (x as usize).min(2);
    let f = x - i as f32;
    [0, 1, 2, 3].map(|k| r[i][k] + (r[i + 1][k] - r[i][k]) * f)
}

/// Shoot wedge colour (purple / cyan in the recordings).
fn shoot_color(kind: JudgeSprite) -> Rgba {
    match kind {
        JudgeSprite::Marvelous => [0.80, 0.45, 1.0, 1.0],
        JudgeSprite::Great => [0.30, 0.95, 0.90, 0.7],
        JudgeSprite::Good | JudgeSprite::Miss => [0.30, 0.70, 1.0, 0.6],
    }
}

/// Number of sparks and their colours per judgement (Marvelous: white/yellow/pink triangles,
/// `vT_HitMarvPt01`; Great/Good: a few cyan-white dots).
fn sparks(kind: JudgeSprite) -> (u32, Sprite, [Rgba; 3]) {
    match kind {
        JudgeSprite::Marvelous => (
            24,
            Sprite::Spark,
            [
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 0.95, 0.5, 1.0],
                [1.0, 0.55, 0.85, 1.0],
            ],
        ),
        JudgeSprite::Great => (6, Sprite::Glow, [[0.6, 1.0, 0.95, 0.9]; 3]),
        JudgeSprite::Good | JudgeSprite::Miss => (4, Sprite::Glow, [[0.5, 0.85, 1.0, 0.8]; 3]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    /// Arc-wide sector from the line inwards in `kind`'s colour ramp.
    Pop(JudgeSprite),
    /// Wedge flying inwards.
    Shoot(Rgba),
    /// White band just outside the line.
    Rim,
    /// Point sprite moving radially.
    Spark { sprite: Sprite, color: Rgba },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Particle {
    shape: Shape,
    /// Start lane (sectors) or lane position (sparks), unwrapped.
    lane: f32,
    /// Arc width in lanes (sectors).
    span: f32,
    /// Radius and radial velocity (sparks).
    r: f32,
    vr: f32,
    age: f32,
    life: f32,
}

impl Particle {
    fn t(&self) -> f32 {
        (self.age / self.life).clamp(0.0, 1.0)
    }
}

/// Live hit effects, key beams and hold sustain glows.
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    particles: Vec<Particle>,
    /// Key-beam intensity per lane (1 = pressed, fading to 0).
    beams: [f32; LANES as usize],
    /// Arcs `(start lane, width)` of the holds being held.
    holds: Vec<(f32, f32)>,
    /// Fractional hold sparks carried to the next update.
    hold_carry: f32,
    seed: u32,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            beams: [0.0; LANES as usize],
            holds: Vec::new(),
            hold_carry: 0.0,
            seed: 0x9e37_79b9,
        }
    }
}

impl Effects {
    /// Uniform in `lo..hi` (xorshift32).
    fn rand(&mut self, lo: f32, hi: f32) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        lo + (hi - lo) * (x >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn len(&self) -> usize {
        self.particles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// A judgement over the arc `lane .. lane + width`. Misses show nothing.
    // UNVERIFIED: vo_miss000_00 was never seen in a recording (E2 had no misses).
    pub fn hit(&mut self, lane: u8, width: u8, kind: JudgeSprite) {
        if kind == JudgeSprite::Miss || width == 0 {
            return;
        }
        let (lane, span) = (f32::from(lane), f32::from(width));
        let sector = |shape, life| Particle {
            shape,
            lane,
            span,
            r: 1.0,
            vr: 0.0,
            age: 0.0,
            life,
        };
        self.particles.push(sector(Shape::Pop(kind), POP_SECS));
        self.particles.push(sector(Shape::Rim, RIM_SECS));
        let inset = span * (1.0 - SHOOT_ARC) / 2.0;
        self.particles.push(Particle {
            lane: lane + inset,
            span: span * SHOOT_ARC,
            ..sector(Shape::Shoot(shoot_color(kind)), SHOOT_SECS)
        });
        let (n, sprite, colors) = sparks(kind);
        for i in 0..n {
            self.spark(lane, span, sprite, colors[i as usize % 3], SPARK_R_MIN);
        }
    }

    fn spark(&mut self, lane: f32, span: f32, sprite: Sprite, color: Rgba, r_min: f32) {
        let p = Particle {
            shape: Shape::Spark { sprite, color },
            lane: lane + self.rand(0.0, span),
            span: 0.0,
            r: self.rand(r_min, 1.0),
            vr: -self.rand(SPARK_SPEED.0, SPARK_SPEED.1),
            age: 0.0,
            life: self.rand(SPARK_LIFE.0, SPARK_LIFE.1),
        };
        self.particles.push(p);
    }

    /// Sensors pressed now (`lanes` bit per lane, any depth): their key beams light up.
    pub fn touch(&mut self, lanes: u64) {
        for (l, b) in self.beams.iter_mut().enumerate() {
            if lanes >> l & 1 == 1 {
                *b = 1.0;
            }
        }
    }

    /// Arcs `(start lane, width)` of the holds currently held (replaces the previous set).
    pub fn set_holds(&mut self, arcs: Vec<(f32, f32)>) {
        self.holds = arcs;
    }

    /// Advance by `dt` seconds: age and move particles, drop expired ones, fade key beams and
    /// emit hold sparks.
    pub fn update(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        for p in &mut self.particles {
            p.age += dt;
            p.r += p.vr * dt;
        }
        self.particles.retain(|p| p.age < p.life);
        for b in &mut self.beams {
            *b = (*b - dt / BEAM_FADE_SECS).max(0.0);
        }
        self.hold_carry += HOLD_SPARK_RATE * dt * self.holds.len() as f32;
        let holds = self.holds.clone();
        for &(lane, span) in holds.iter().cycle().take(self.hold_carry as usize) {
            self.spark(lane, span, Sprite::Spark, [1.0, 0.8, 0.3, 1.0], 0.95);
        }
        self.hold_carry = if holds.is_empty() {
            0.0
        } else {
            self.hold_carry.fract()
        };
    }

    /// Append the effects' triangles (additive) to `out`.
    pub fn draw(&self, out: &mut Vec<Vertex>, sheet: &SpriteSheet) {
        // A single column of the guide-lane gradient: bright at the line, fading inwards.
        let fade = sheet.get(Sprite::GuideLane).map(|[u0, v0, u1, v1]| {
            let u = (u0 + u1) / 2.0;
            [u, v0, u, v1]
        });
        let fill = |c: Rgba| match fade {
            Some(uv) => Paint::image(c, uv).additive(),
            None => Paint::flat(c).additive(),
        };
        for (lane, &b) in self.beams.iter().enumerate().filter(|(_, b)| **b > 0.0) {
            let paint = fill([1.0, 1.0, 1.0, BEAM_ALPHA * b]);
            paint_sector(out, 1.0, 1.0 - BEAM_LEN, lane as f32, 1.0, &paint);
            // The rim over a pressed sensor turns white.
            let rim = [1.0, 1.0, 1.0, BEAM_RIM_ALPHA * b];
            push_sector(out, 1.0, 1.0 + RIM_WIDTH, lane as f32, 1.0, rim);
        }
        for &(lane, span) in &self.holds {
            paint_sector(out, 1.0, 1.0 - CORE_LEN * 2.0, lane, span, &fill(HOLD_GLOW));
            paint_sector(out, 1.0, 1.0 + RIM_WIDTH, lane, span, &fill(HOLD_GLOW));
        }
        for p in &self.particles {
            let t = p.t();
            match p.shape {
                Shape::Pop(kind) => {
                    // The recordings read paler than the raw ramp: blend towards white.
                    let ramp = ramp_at(kind, t * POP_RAMP_SPAN);
                    let w = POP_WHITE + (1.0 - POP_WHITE) * (1.0 - t) * 0.5;
                    let mut c = [0, 1, 2, 3].map(|k| ramp[k] + (1.0 - ramp[k]) * w);
                    c[3] = 1.0 - t;
                    let len = POP_LEN * (0.8 + 0.2 * (1.0 - (1.0 - t).powi(3)));
                    paint_sector(out, 1.0, 1.0 - len, p.lane, p.span, &fill(c));
                    let core = fill([1.0, 1.0, 1.0, (1.0 - t).powi(2)]);
                    paint_sector(out, 1.0, 1.0 - CORE_LEN, p.lane, p.span, &core);
                }
                Shape::Shoot(mut c) => {
                    c[3] *= 1.0 - t;
                    let head = 1.0 - SHOOT_DIST * (1.0 - (1.0 - t).powi(2));
                    let tail = (head + SHOOT_LEN).min(1.0);
                    // A wedge: full width at the tail, narrowing towards the head.
                    let w = p.span * SHOOT_TAPER;
                    let head_arc = (head, p.lane + (p.span - w) / 2.0, w);
                    push_strip(out, (tail, p.lane, p.span), head_arc, &fill(c));
                }
                Shape::Rim => {
                    let c = [1.0, 1.0, 1.0, 1.0 - t * t];
                    push_sector(out, 1.0, 1.0 + RIM_WIDTH, p.lane, p.span, c);
                }
                Shape::Spark { sprite, mut color } => {
                    color[3] *= 1.0 - t;
                    let paint = match sheet.get(sprite) {
                        Some(uv) => Paint::image(color, uv),
                        None => Paint::flat(color),
                    }
                    .additive();
                    let s = SPARK_SIZE * p.r.max(0.2);
                    // Pointing along its motion (inwards or outwards).
                    let dir = polar(if p.vr < 0.0 { -1.0 } else { 1.0 }, p.lane);
                    push_oriented_rect(out, polar(p.r, p.lane), [s, s], dir, &paint);
                }
            }
        }
    }
}

/// Arc `(start lane unwrapped, width)` of a hold body at chart time `t` (linear between nodes,
/// as `layout::push_hold_body`); clamped to the first/last node.
pub fn hold_arc_at(nodes: &[HoldNode], t: f64) -> Option<(f32, f32)> {
    let first = nodes.first()?;
    let at = |n: &HoldNode| (n.lane_unwrapped, f32::from(n.arc.width));
    let Some(i) = nodes.iter().position(|n| n.time > t) else {
        return nodes.last().map(at);
    };
    if i == 0 {
        return Some(at(first));
    }
    let (a, b) = (&nodes[i - 1], &nodes[i]);
    let f = ((t - a.time) / (b.time - a.time).max(1e-9)) as f32;
    let ((la, wa), (lb, wb)) = (at(a), at(b));
    Some((la + (lb - la) * f, wa + (wb - wa) * f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{Arc, BeatPos};

    #[test]
    fn hits_spawn_age_and_expire() {
        let mut fx = Effects::default();
        fx.hit(10, 4, JudgeSprite::Miss);
        assert!(fx.is_empty(), "misses show nothing");
        fx.hit(10, 4, JudgeSprite::Marvelous);
        let (n, ..) = sparks(JudgeSprite::Marvelous);
        assert_eq!(fx.len(), 3 + n as usize);
        fx.update(SHOOT_SECS + 0.01);
        assert!(
            !fx.particles
                .iter()
                .any(|p| matches!(p.shape, Shape::Shoot(_)))
        );
        assert!(
            fx.particles
                .iter()
                .any(|p| matches!(p.shape, Shape::Pop(_)))
        );
        // Sparks move inwards.
        assert!(
            fx.particles
                .iter()
                .filter(|p| matches!(p.shape, Shape::Spark { .. }))
                .all(|p| p.r < 1.0)
        );
        fx.update(SPARK_LIFE.1);
        assert!(fx.is_empty());
    }

    #[test]
    fn deterministic_and_inside_the_arc() {
        let run = || {
            let mut fx = Effects::default();
            fx.hit(58, 4, JudgeSprite::Great);
            fx.update(0.05);
            fx
        };
        assert_eq!(run(), run());
        let fx = run();
        for p in &fx.particles {
            assert!((58.0..=62.0).contains(&p.lane), "{p:?}");
        }
        // Negative dt does not rewind.
        let mut back = run();
        back.update(-1.0);
        assert_eq!(back, fx);
    }

    #[test]
    fn beams_fade_after_release() {
        let mut fx = Effects::default();
        fx.touch(1 << 3 | 1 << 59);
        fx.update(BEAM_FADE_SECS / 2.0);
        assert!((fx.beams[3] - 0.5).abs() < 1e-5 && fx.beams[59] > 0.0 && fx.beams[4] == 0.0);
        fx.touch(1 << 3);
        fx.update(BEAM_FADE_SECS / 2.0);
        assert!(fx.beams[3] > fx.beams[59]);
        assert_eq!(fx.beams[59], 0.0);
        let mut v = Vec::new();
        fx.draw(&mut v, &SpriteSheet::default());
        assert_eq!(v.len(), 2 * 6, "one lane: beam and rim quads");
    }

    #[test]
    fn holds_emit_sparks_at_their_rate() {
        let mut fx = Effects::default();
        fx.set_holds(vec![(5.0, 3.0), (30.0, 2.0)]);
        fx.update(0.1);
        assert_eq!(fx.len(), (HOLD_SPARK_RATE * 0.1 * 2.0) as usize);
        fx.set_holds(vec![]);
        fx.update(SPARK_LIFE.1);
        assert!(fx.is_empty());
    }

    #[test]
    fn draws_every_shape() {
        let mut fx = Effects::default();
        fx.hit(0, 6, JudgeSprite::Good);
        let mut v = Vec::new();
        fx.draw(&mut v, &SpriteSheet::default());
        // Pop + core + rim + shoot (6 lanes, or fewer for the shoot) + sparks.
        assert!(v.len() >= (3 * 6 + 4) * 6);
        assert!(v.iter().all(|p| p.fx[1] == 1.0 || p.color[..3] == [1.0; 3]));
    }

    #[test]
    fn ramps_run_from_the_bright_end() {
        assert_eq!(
            ramp_at(JudgeSprite::Great, 0.0),
            ramp(JudgeSprite::Great)[0]
        );
        assert_eq!(
            ramp_at(JudgeSprite::Great, 1.0),
            ramp(JudgeSprite::Great)[3]
        );
        let mid = ramp_at(JudgeSprite::Marvelous, 0.5);
        assert!(mid[2] > 0.9, "magenta in the middle: {mid:?}");
    }

    #[test]
    fn hold_arc_interpolates() {
        let n = |time, lane: f32| HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc {
                lane: lane as u8 % 60,
                width: 4,
            },
            lane_unwrapped: lane,
        };
        let nodes = [n(1.0, 58.0), n(2.0, 62.0)];
        assert_eq!(hold_arc_at(&nodes, 1.5), Some((60.0, 4.0)));
        assert_eq!(hold_arc_at(&nodes, 0.0), Some((58.0, 4.0)));
        assert_eq!(hold_arc_at(&nodes, 3.0), Some((62.0, 4.0)));
        assert_eq!(hold_arc_at(&[], 1.0), None);
    }
}
