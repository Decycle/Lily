//! Measuring notes in captured playfield frames (experiment E2, doc 09): where a touch note is along its lane's
//! ray at a known time-to-hit, which angle its arc covers, and fitting the approach model to those samples.
//!
//! Screen geometry: centre `(cx, cy)` in pixels (y down), angles in degrees counter-clockwise from 3 o'clock as on
//! screen, so lane `l` (lane 0 just above 3 o'clock, counter-clockwise; docs/research/04) starts at `theta0 + 6 l`.
//! Model (docs/research/04 §2.2): `x = 1 - (tau + delta) / T` (0 at spawn, 1 on the judgement line, `tau` = note
//! time - frame time), `r = R (1 - k) x / (1 - k x)`; our renderer uses `T = 3.3 s / speed`, `k = 10.5 / 13.825`.

use crate::frame::Rgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub cx: f64,
    pub cy: f64,
}

impl Geometry {
    /// Pixel at radius `r` (px) and screen angle `deg`, if inside the frame.
    pub fn at(&self, f: &Rgb, r: f64, deg: f64) -> Option<[u8; 3]> {
        let a = deg.to_radians();
        let (x, y) = (self.cx + r * a.cos(), self.cy - r * a.sin());
        (x >= 0.0 && y >= 0.0 && (x as usize) < f.width && (y as usize) < f.height)
            .then(|| f.pixel(x as usize, y as usize))
    }
}

/// Touch-note colour (bright pink/magenta: red and blue high, green low). Excludes the rim (bluer than red), hold
/// and chain notes (green high) and white/grey text and glow.
pub fn is_touch_note(p: [u8; 3]) -> bool {
    let [r, g, b] = p.map(i32::from);
    r >= 170 && r - g >= 80 && r + 15 >= b && b >= 100
}

/// Rim colour (the saturated pink-to-blue judgement ring; touch notes match too, so callers look for the rim's
/// long run near the edge).
pub fn is_rim(p: [u8; 3]) -> bool {
    let [r, g, b] = p.map(i32::from);
    r.max(b) >= 170 && g <= 60
}

/// Runs `(r_start, r_end)` (px, inclusive) of matching pixels along the ray at `deg` for radii in `r0..r1`, joining
/// gaps of up to `gap` px.
pub fn ray_runs(
    f: &Rgb,
    g: &Geometry,
    deg: f64,
    r0: f64,
    r1: f64,
    gap: f64,
    is: impl Fn([u8; 3]) -> bool,
) -> Vec<(f64, f64)> {
    let mut runs: Vec<(f64, f64)> = Vec::new();
    let mut r = r0;
    while r <= r1 {
        if g.at(f, r, deg).is_some_and(&is) {
            match runs.last_mut() {
                Some(last) if r - last.1 <= gap + 1.0 => last.1 = r,
                _ => runs.push((r, r)),
            }
        }
        r += 1.0;
    }
    runs
}

/// Angular span `(start, end)` (degrees, start < end) of matching pixels at radius `r` that contains `around`,
/// walking out at most `half` degrees each way in `step` steps.
pub fn arc_span(
    f: &Rgb,
    g: &Geometry,
    r: f64,
    around: f64,
    half: f64,
    step: f64,
    is: impl Fn([u8; 3]) -> bool,
) -> Option<(f64, f64)> {
    let hit = |d: f64| g.at(f, r, d).is_some_and(&is);
    if !hit(around) {
        return None;
    }
    let walk = |dir: f64| {
        let mut d = around;
        while (d - around).abs() < half && hit(d + dir * step) {
            d += dir * step;
        }
        d
    };
    Some((walk(-1.0), walk(1.0)))
}

/// Least-squares circle through points (Kasa fit): `(cx, cy, radius)`.
pub fn fit_circle(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = pts.len() as f64;
    if pts.len() < 3 {
        return None;
    }
    let (mx, my) = pts
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / n, b + y / n));
    let (mut suu, mut svv, mut suv, mut suuu, mut svvv, mut suvv, mut svuu) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for &(x, y) in pts {
        let (u, v) = (x - mx, y - my);
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-9 {
        return None;
    }
    let (bu, bv) = ((suuu + suvv) / 2.0, (svvv + svuu) / 2.0);
    let uc = (bu * svv - bv * suv) / det;
    let vc = (bv * suu - bu * suv) / det;
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    Some((mx + uc, my + vc, r))
}

/// Model radius (px) at time-to-hit `tau`, `None` before spawn.
pub fn model_radius(tau: f64, m: &Approach) -> Option<f64> {
    let x = 1.0 - (tau + m.delta) / m.t;
    (x >= 0.0).then(|| m.r * (1.0 - m.k) * x / (1.0 - m.k * x))
}

/// Approach model parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Approach {
    /// Visible time (s) from spawn to the judgement line at the play's note speed.
    pub t: f64,
    /// Perspective constant.
    pub k: f64,
    /// Judgement-line radius (px).
    pub r: f64,
    /// Notes reach the line at `tau = -delta` (s), i.e. `delta` after their chart time.
    pub delta: f64,
}

/// Fit result: model plus RMS radius error (px) and sample count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub model: Approach,
    pub rms_px: f64,
    pub n: usize,
}

/// Fits [`Approach`] to `(tau, r_px)` samples. For each `(k, R)` on a grid the inverse model `x(r)` is linear in
/// `tau`, so `T` and `delta` come from a least-squares line; the `k` with the smallest RMS radius error wins.
///
/// The curve `r(tau)` is a linear fractional function (3 degrees of freedom), so the judgement-line radius `r`
/// cannot be fitted together with `T`, `k` and `delta`; it is given (measured from the rim).
pub fn fit_approach(samples: &[(f64, f64)], ks: &[f64], r: f64) -> Option<Fit> {
    let n = samples.len();
    if n < 3 {
        return None;
    }
    let nf = n as f64;
    let mt = samples.iter().map(|s| s.0).sum::<f64>() / nf;
    let mut best: Option<Fit> = None;
    for &k in ks {
        // x = r_px / (R (1 - k) + k r_px)
        let xs: Vec<f64> = samples
            .iter()
            .map(|&(_, p)| p / (r * (1.0 - k) + k * p))
            .collect();
        let mx = xs.iter().sum::<f64>() / nf;
        let (mut stt, mut stx) = (0.0, 0.0);
        for (s, x) in samples.iter().zip(&xs) {
            stt += (s.0 - mt) * (s.0 - mt);
            stx += (s.0 - mt) * (x - mx);
        }
        if stt == 0.0 || stx >= 0.0 {
            continue;
        }
        let b = stx / stt;
        let t = -1.0 / b;
        let model = Approach {
            t,
            k,
            r,
            delta: (1.0 - (mx - b * mt)) * t,
        };
        let sq: f64 = samples
            .iter()
            .map(|&(tau, p)| model_radius(tau, &model).map_or(p * p, |m| (m - p).powi(2)))
            .sum();
        let rms_px = (sq / nf).sqrt();
        if best.is_none_or(|b| rms_px < b.rms_px) {
            best = Some(Fit { model, rms_px, n });
        }
    }
    best
}

/// Screen angle (degrees) of the centre of a `width`-lane arc starting at `lane`, lane 0 starting at `theta0`.
pub fn lane_centre_deg(theta0: f64, lane: u8, width: u8) -> f64 {
    (theta0 + 6.0 * (f64::from(lane) + f64::from(width) / 2.0)).rem_euclid(360.0)
}

/// A touch note worth measuring in a frame at chart time `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub note: usize,
    /// Note time - frame time (s).
    pub tau: f64,
    /// Screen angle of the arc centre (degrees).
    pub deg: f64,
}

/// Touch notes with `0 < tau <= max_tau` whose centre angle passes `visible` and whose centre lane no other note
/// (any kind; holds over their whole length) uses within `isolation` seconds.
pub fn candidates(
    chart: &wacca_chart::Chart,
    t: f64,
    max_tau: f64,
    isolation: f64,
    theta0: f64,
    visible: impl Fn(f64) -> bool,
) -> Vec<Candidate> {
    use wacca_chart::NoteBody;
    let span = |n: &wacca_chart::Note| match &n.body {
        NoteBody::Hold { nodes } => (n.time, nodes.last().map_or(n.time, |e| e.time)),
        _ => (n.time, n.time),
    };
    let uses = |n: &wacca_chart::Note, lane: u8| match &n.body {
        NoteBody::Hold { nodes } => nodes.iter().any(|h| h.arc.contains(lane)),
        _ => n.arc.contains(lane),
    };
    chart
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.body, NoteBody::Touch) && n.time > t && n.time - t <= max_tau)
        .filter_map(|(i, n)| {
            let deg = lane_centre_deg(theta0, n.arc.lane, n.arc.width);
            let centre = ((n.arc.lane as u16 + n.arc.width as u16 / 2) % 60) as u8;
            let crowded = chart.notes.iter().enumerate().any(|(j, o)| {
                let (a, b) = span(o);
                j != i && a - isolation < n.time && b + isolation > n.time && uses(o, centre)
            });
            (visible(deg) && !crowded).then_some(Candidate {
                note: i,
                tau: n.time - t,
                deg,
            })
        })
        .collect()
}

/// The run whose outer edge is nearest `expected` (px), if within `tolerance`; returns its outer edge.
pub fn nearest_run(runs: &[(f64, f64)], expected: f64, tolerance: f64) -> Option<f64> {
    runs.iter()
        .map(|r| r.1)
        .filter(|r| (r - expected).abs() <= tolerance)
        .min_by(|a, b| (a - expected).abs().total_cmp(&(b - expected).abs()))
}

/// The time shift from `deltas` under which most `(tau, runs)` samples have a run edge within 3 px + 4 % of the
/// radius `model` predicts (its own `delta` replaced); returns `(delta, matches)`. Used per song segment to check
/// that the game's visual clock keeps a constant offset to the audio.
pub fn best_delta(
    samples: &[(f64, &[(f64, f64)])],
    model: &Approach,
    deltas: &[f64],
) -> Option<(f64, usize)> {
    let counts = deltas
        .iter()
        .map(|&delta| {
            let m = Approach { delta, ..*model };
            let n = samples
                .iter()
                .filter(|(tau, runs)| {
                    model_radius(*tau, &m)
                        .filter(|&e| e >= 20.0)
                        .is_some_and(|e| runs.iter().any(|r| (r.1 - e).abs() <= 3.0 + 0.04 * e))
                })
                .count();
            (delta, n)
        })
        .collect::<Vec<_>>();
    // The tolerance makes a plateau of equally good shifts: take its middle.
    let best = counts.iter().map(|c| c.1).max().filter(|&n| n > 0)?;
    let top: Vec<f64> = counts.iter().filter(|c| c.1 == best).map(|c| c.0).collect();
    Some((top.iter().sum::<f64>() / top.len() as f64, best))
}

/// Indices `i` where `values[i + 1]` differs from `values[i]` by more than `limit`.
pub fn jumps(values: &[f64], limit: f64) -> Vec<usize> {
    values
        .windows(2)
        .enumerate()
        .filter(|(_, w)| (w[1] - w[0]).abs() > limit)
        .map(|(i, _)| i)
        .collect()
}

/// Evenly spaced grid `from..=to`.
pub fn grid(from: f64, to: f64, step: f64) -> Vec<f64> {
    let n = ((to - from) / step).round() as usize;
    (0..=n).map(|i| from + step * i as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Black frame with pink pixels where `paint(x, y)` is true.
    fn frame(w: usize, h: usize, paint: impl Fn(f64, f64) -> bool) -> Rgb {
        let mut data = vec![0; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                if paint(x as f64 + 0.5, y as f64 + 0.5) {
                    data[(y * w + x) * 3..][..3].copy_from_slice(&[240, 60, 220]);
                }
            }
        }
        Rgb::new(w, h, data).unwrap()
    }

    #[test]
    fn colours() {
        assert!(is_touch_note([231, 115, 231]) && is_touch_note([214, 38, 189]));
        assert!(!is_touch_note([198, 23, 255]) && !is_touch_note([44, 37, 103]));
        assert!(!is_touch_note([255, 220, 60]) && !is_touch_note([230, 230, 230]));
        assert!(is_rim([209, 16, 255]) && is_rim([255, 42, 148]) && !is_rim([231, 115, 231]));
    }

    #[test]
    fn finds_an_arc_band_on_its_ray() {
        let g = Geometry {
            cx: 100.0,
            cy: 100.0,
        };
        // Band between radius 40 and 48, angles 250..290 degrees (below the centre).
        let f = frame(200, 200, |x, y| {
            let (dx, dy) = (x - 100.0, 100.0 - y);
            let r = dx.hypot(dy);
            let a = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            (40.0..=48.0).contains(&r) && (250.0..=290.0).contains(&a)
        });
        let runs = ray_runs(&f, &g, 270.0, 5.0, 90.0, 2.0, is_touch_note);
        assert_eq!(runs.len(), 1);
        assert!((runs[0].0 - 40.0).abs() <= 1.0 && (runs[0].1 - 48.0).abs() <= 1.0);
        assert!(ray_runs(&f, &g, 90.0, 5.0, 90.0, 2.0, is_touch_note).is_empty());
        let (a0, a1) = arc_span(&f, &g, 44.0, 270.0, 40.0, 0.25, is_touch_note).unwrap();
        assert!(
            (a0 - 250.0).abs() < 1.5 && (a1 - 290.0).abs() < 1.5,
            "{a0} {a1}"
        );
        assert!(arc_span(&f, &g, 44.0, 90.0, 40.0, 0.25, is_touch_note).is_none());
    }

    #[test]
    fn candidate_notes_are_isolated_touches() {
        let chart = wacca_chart::parse(
            b"#OFFSET 0.0\n#BODY\n   0    0    2 120.000000\n   0    0    3    4\n\
              1    0    1    1    0   40    4    1\n\
              1  480    1    1    1   30    4    1\n\
              1  960    1   16    2   31    2    1\n\
              2    0    1    1    3   10    4    1\n\
              9    0    1   14    4    0   60    1\n",
        )
        .unwrap();
        // Notes at 2.0 (lane 40), 2.5 (lane 30, a chain at 3.0 on lane 31-32 crowds it), 4.0 (lane 10).
        let all = candidates(&chart, 1.0, 5.0, 0.6, 0.0, |_| true);
        let picked: Vec<(usize, f64)> = all.iter().map(|c| (c.note, c.tau)).collect();
        assert_eq!(picked, [(0, 1.0), (3, 3.0)]);
        assert_eq!(all[0].deg, 252.0);
        let lower = candidates(&chart, 1.0, 5.0, 0.6, 0.0, |d| d > 180.0);
        assert_eq!(lower.len(), 1);
        assert!(candidates(&chart, 1.0, 0.5, 0.6, 0.0, |_| true).is_empty());
        assert_eq!(lane_centre_deg(-3.0, 59, 2), 357.0);
        assert_eq!(lane_centre_deg(3.0, 59, 2), 3.0);
        assert_eq!(
            nearest_run(&[(10.0, 20.0), (50.0, 60.0)], 55.0, 10.0),
            Some(60.0)
        );
        assert_eq!(nearest_run(&[(10.0, 20.0)], 55.0, 10.0), None);
    }

    #[test]
    fn segment_delta_and_jumps() {
        let m = Approach {
            t: 2.2,
            k: 0.75,
            r: 290.0,
            delta: 0.0,
        };
        let shifted = Approach { delta: -0.09, ..m };
        let runs: Vec<(f64, Vec<(f64, f64)>)> = (1..150)
            .map(|i| 0.01 * f64::from(i))
            .filter_map(|tau| model_radius(tau, &shifted).map(|r| (tau, vec![(r - 10.0, r)])))
            .collect();
        let samples: Vec<(f64, &[(f64, f64)])> =
            runs.iter().map(|(t, r)| (*t, r.as_slice())).collect();
        let (d, n) = best_delta(&samples, &m, &grid(-0.3, 0.3, 0.01)).unwrap();
        assert!((d + 0.09).abs() < 0.006 && n > 100, "{d} {n}");
        assert_eq!(best_delta(&[], &m, &[0.0]), None);
        assert_eq!(jumps(&[-0.085, -0.085, 0.03, 0.03], 0.03), [1]);
    }

    #[test]
    fn circle_fit() {
        let pts: Vec<(f64, f64)> = (0..36)
            .map(|i| {
                let a = f64::from(i) * 10f64.to_radians();
                (337.5 + 300.0 * a.cos(), 563.0 + 300.0 * a.sin())
            })
            .collect();
        let (cx, cy, r) = fit_circle(&pts).unwrap();
        assert!((cx - 337.5).abs() < 1e-6 && (cy - 563.0).abs() < 1e-6 && (r - 300.0).abs() < 1e-6);
        assert!(fit_circle(&pts[..2]).is_none());
    }

    #[test]
    fn recovers_approach_parameters() {
        let truth = Approach {
            t: 2.2,
            k: 10.5 / 13.825,
            r: 290.0,
            delta: 0.03,
        };
        let samples: Vec<(f64, f64)> = (0..200)
            .map(|i| 0.01 * f64::from(i))
            .filter_map(|tau| model_radius(tau, &truth).map(|r| (tau, r)))
            .filter(|&(_, r)| r > 10.0)
            .collect();
        let fit = fit_approach(&samples, &grid(0.70, 0.80, 0.001), 290.0).unwrap();
        let m = fit.model;
        assert!((m.t - 2.2).abs() < 0.01, "{fit:?}");
        assert!((m.k - truth.k).abs() < 0.002, "{fit:?}");
        assert!(
            (m.delta - 0.03).abs() < 0.003 && fit.rms_px < 0.5,
            "{fit:?}"
        );
        assert_eq!(model_radius(5.0, &truth), None);
        assert_eq!(grid(0.0, 1.0, 0.25), [0.0, 0.25, 0.5, 0.75, 1.0]);
    }
}
