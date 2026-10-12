//! The test menu's "TEST OF INPUT MACHINE" screen: a 60 x 4 grid of touch cells (thin white lines on black, a
//! digit in the middle of each cell) whose cells turn light grey while touched.
//!
//! Screen cells are numbered in our lane convention, independent of Waccon's: `lane * 4 + ring`, lane 0 = the
//! 6 degree segment just above 3 o'clock, lanes counter-clockwise, ring 0 = innermost. [`fit`] calibrates the
//! grid's centre and radii from one frame; [`cell_levels`] reads every cell's brightness while avoiding its digit.

use crate::frame::Gray;
use wacca_chart::LANES;
use wacca_game::input::DEPTHS;

pub const SCREEN_CELLS: usize = LANES as usize * DEPTHS;
/// Mean luma above which a cell counts as lit (unlit cells read ~0-20, lit ~190).
pub const LIT_LEVEL: f64 = 100.0;
const LANE_DEG: f64 = 360.0 / LANES as f64;

pub fn screen_cell(lane: u8, ring: usize) -> usize {
    usize::from(lane) * DEPTHS + ring
}

/// `(lane, ring)` of a screen cell.
pub fn screen_lane_ring(cell: usize) -> (u8, usize) {
    ((cell / DEPTHS) as u8, cell % DEPTHS)
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Geometry {
    pub cx: f64,
    pub cy: f64,
    pub r_in: f64,
    pub r_out: f64,
}

impl Geometry {
    /// Starting point for [`fit`], measured on the 675x1200 window (scaled with the width).
    pub fn initial(width: usize, height: usize) -> Self {
        let w = width as f64;
        Self {
            cx: 0.5 * w,
            cy: 0.467 * height as f64,
            r_in: 0.289 * w,
            r_out: 0.471 * w,
        }
    }

    fn ring_width(&self) -> f64 {
        (self.r_out - self.r_in) / DEPTHS as f64
    }

    /// Pixel position at `deg` counter-clockwise from 3 o'clock, radius `r`.
    fn point(&self, deg: f64, r: f64) -> (f64, f64) {
        let a = deg.to_radians();
        (self.cx + r * a.cos(), self.cy - r * a.sin())
    }

    /// Eight points inside a cell on a 3x3 grid around its centre, skipping the centre (the digit).
    pub fn cell_points(&self, lane: u8, ring: usize) -> Vec<(f64, f64)> {
        let mid_deg = (f64::from(lane) + 0.5) * LANE_DEG;
        let mid_r = self.r_in + (ring as f64 + 0.5) * self.ring_width();
        let mut out = Vec::with_capacity(8);
        for da in [-0.32, 0.0, 0.32] {
            for dr in [-0.3, 0.0, 0.3] {
                if da != 0.0 || dr != 0.0 {
                    out.push(self.point(mid_deg + da * LANE_DEG, mid_r + dr * self.ring_width()));
                }
            }
        }
        out
    }
}

fn mean(g: &Gray, pts: &[(f64, f64)]) -> f64 {
    pts.iter().map(|&(x, y)| g.sample(x, y)).sum::<f64>() / pts.len().max(1) as f64
}

/// Search radius (px) around the start centre and radii in [`fit`].
const SEARCH_PX: f64 = 10.0;
const PROFILE_STEP: f64 = 0.5;

/// Mean luma on the circle of radius `r` around `(cx, cy)`, at a quarter and three quarters of every lane (off the
/// spokes and off the digits).
fn ring_mean(g: &Gray, cx: f64, cy: f64, r: f64) -> f64 {
    let geo = Geometry {
        cx,
        cy,
        r_in: 0.0,
        r_out: 0.0,
    };
    let pts: Vec<(f64, f64)> = (0..2 * u32::from(LANES))
        .map(|i| geo.point((f64::from(i) * 0.5 + 0.25) * LANE_DEG, r))
        .collect();
    mean(g, &pts)
}

/// Best `(r_in, r_out, score)` for a radial profile sampled every [`PROFILE_STEP`] from `r0`: the five ring
/// circles should be bright and the four ring middles dark.
fn best_radii(profile: &[f64], r0: f64, start: &Geometry) -> (f64, f64, f64) {
    let at = |r: f64| {
        let x = (r - r0) / PROFILE_STEP;
        let i = x.floor().max(0.0) as usize;
        let f = x - x.floor();
        let v = |i: usize| profile.get(i).copied().unwrap_or(0.0);
        v(i) * (1.0 - f) + v(i + 1) * f
    };
    let n = (2.0 * SEARCH_PX / PROFILE_STEP) as usize;
    let mut best = (start.r_in, start.r_out, f64::NEG_INFINITY);
    for i in 0..=n {
        for o in 0..=n {
            let r_in = start.r_in - SEARCH_PX + i as f64 * PROFILE_STEP;
            let r_out = start.r_out - SEARCH_PX + o as f64 * PROFILE_STEP;
            let w = (r_out - r_in) / DEPTHS as f64;
            let lines: f64 = (0..=DEPTHS).map(|k| at(r_in + k as f64 * w)).sum();
            let gaps: f64 = (0..DEPTHS).map(|k| at(r_in + (k as f64 + 0.5) * w)).sum();
            let score = lines / (DEPTHS + 1) as f64 - gaps / DEPTHS as f64;
            if score > best.2 {
                best = (r_in, r_out, score);
            }
        }
    }
    best
}

/// Calibrates the grid within [`SEARCH_PX`] of `start`: for every candidate centre (1 px grid, then 0.25 px
/// around the best) the radial profile is matched against five evenly spaced bright circles; an off-centre
/// guess smears the circles, so the sharpest match wins. Returns the geometry and its score (line minus gap
/// brightness, ~200 on a clean frame).
pub fn fit(g: &Gray, start: Geometry) -> (Geometry, f64) {
    let r0 = start.r_in - SEARCH_PX - 1.0;
    let len = ((start.r_out + SEARCH_PX + 1.0 - r0) / PROFILE_STEP) as usize + 2;
    let eval = |cx: f64, cy: f64| {
        let profile: Vec<f64> = (0..len)
            .map(|i| ring_mean(g, cx, cy, r0 + i as f64 * PROFILE_STEP))
            .collect();
        let (r_in, r_out, score) = best_radii(&profile, r0, &start);
        (
            Geometry {
                cx,
                cy,
                r_in,
                r_out,
            },
            score,
        )
    };
    let search = |c: (f64, f64), half: i32, step: f64, best: (Geometry, f64)| {
        let mut best = best;
        for dy in -half..=half {
            for dx in -half..=half {
                let cand = eval(c.0 + f64::from(dx) * step, c.1 + f64::from(dy) * step);
                if cand.1 > best.1 {
                    best = cand;
                }
            }
        }
        best
    };
    let coarse = search(
        (start.cx, start.cy),
        SEARCH_PX as i32,
        1.0,
        eval(start.cx, start.cy),
    );
    search((coarse.0.cx, coarse.0.cy), 3, 0.25, coarse)
}

/// Mean luma of every screen cell (index [`screen_cell`]).
pub fn cell_levels(g: &Gray, geo: &Geometry) -> Vec<f64> {
    (0..SCREEN_CELLS)
        .map(|c| {
            let (lane, ring) = screen_lane_ring(c);
            mean(g, &geo.cell_points(lane, ring))
        })
        .collect()
}

pub fn lit_cells(levels: &[f64]) -> Vec<usize> {
    (0..levels.len())
        .filter(|&c| levels[c] > LIT_LEVEL)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 200;
    const H: usize = 300;
    const TRUE_GEO: Geometry = Geometry {
        cx: 101.0,
        cy: 139.0,
        r_in: 55.0,
        r_out: 95.0,
    };

    /// Synthetic input-test screen: grid lines 255, `lit` cells 190, rest 0, a bright "digit" in every cell centre.
    fn synth(lit: &[usize]) -> Gray {
        let mut data = vec![0u8; W * H];
        let geo = TRUE_GEO;
        let rw = geo.ring_width();
        for y in 0..H {
            for x in 0..W {
                let (dx, dy) = (x as f64 - geo.cx, geo.cy - y as f64);
                let r = dx.hypot(dy);
                if r < geo.r_in - 0.5 || r > geo.r_out + 0.5 {
                    continue;
                }
                let deg = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                let ring_pos = (r - geo.r_in) / rw;
                let lane_pos = deg / LANE_DEG;
                let near_line = |pos: f64, scale: f64| (pos - pos.round()).abs() * scale < 0.7;
                let v = if near_line(ring_pos, rw) || near_line(lane_pos, r * LANE_DEG.to_radians())
                {
                    255
                } else {
                    let (lane, ring) =
                        (lane_pos as u8 % LANES, (ring_pos as usize).min(DEPTHS - 1));
                    let centre = (lane_pos.fract() - 0.5).abs() < 0.1
                        && (ring_pos.fract() - 0.5).abs() < 0.12;
                    if centre {
                        255
                    } else if lit.contains(&screen_cell(lane, ring)) {
                        190
                    } else {
                        0
                    }
                };
                data[y * W + x] = v;
            }
        }
        Gray::new(W, H, data).unwrap()
    }

    #[test]
    fn cell_numbering_round_trips() {
        for c in 0..SCREEN_CELLS {
            let (l, r) = screen_lane_ring(c);
            assert_eq!(screen_cell(l, r), c);
        }
        assert_eq!(screen_cell(59, 3), 239);
    }

    #[test]
    fn lane_zero_is_just_above_three_oclock() {
        let geo = TRUE_GEO;
        for (x, y) in geo.cell_points(0, 0) {
            assert!(x > geo.cx && y < geo.cy);
        }
        // Lane 15 is just left of 12 o'clock (counter-clockwise).
        for (x, y) in geo.cell_points(15, 3) {
            assert!(x < geo.cx && y < geo.cy);
        }
    }

    #[test]
    fn fit_recovers_the_grid() {
        let g = synth(&[]);
        let start = Geometry {
            cx: 98.0,
            cy: 142.0,
            r_in: 58.0,
            r_out: 92.0,
        };
        let (geo, score) = fit(&g, start);
        assert!(score > 100.0, "{score}");
        for (got, want) in [
            (geo.cx, TRUE_GEO.cx),
            (geo.cy, TRUE_GEO.cy),
            (geo.r_in, TRUE_GEO.r_in),
            (geo.r_out, TRUE_GEO.r_out),
        ] {
            assert!((got - want).abs() <= 1.0, "{geo:?}");
        }
    }

    #[test]
    fn lit_cells_are_found() {
        let lit = [screen_cell(0, 0), screen_cell(0, 3), screen_cell(31, 2)];
        let g = synth(&lit);
        let levels = cell_levels(&g, &TRUE_GEO);
        assert_eq!(lit_cells(&levels), lit);
        assert!(lit_cells(&cell_levels(&synth(&[]), &TRUE_GEO)).is_empty());
    }

    #[test]
    fn initial_geometry_scales_with_window() {
        let g = Geometry::initial(675, 1200);
        assert!((g.cx - 337.5).abs() < 1e-9 && (g.cy - 560.4).abs() < 1e-9);
        assert!(g.r_in < g.r_out);
    }
}
