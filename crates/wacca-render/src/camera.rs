//! The play view's 3D camera and the note transform (pure). Spec: `docs/research/11-shaders-and-meshes.md`
//! §2.3–2.4, §3.0–3.1 (RenderDoc captures of the real game).
//!
//! World units are cm, UE axes. The camera sits at `(0, CAMERA_Y, 0)` looking down −Y (up +Z,
//! right +X) with a 90° FOV on a square view: `ndc = (X, Z) / w` with view depth
//! `w = CAMERA_Y − Y`; NDC 1 is the edge of the square viewport = [`VIEW_EXTENT`] playfield units
//! (playfield radius 1 = the rim's inner edge). Screen x = world X, screen y = world Z, so lanes run
//! counter-clockwise from 3 o'clock.
//!
//! A note at scale `s` (0 at spawn, 1 on the judgement line, linear in time over
//! [`APPROACH_SECS`](crate::layout::APPROACH_SECS); `s = 1 − depth`) has `LocalToWorld` = uniform
//! scale `s`, rotation by `φ = 6°·lane_start` about Y (local +X → angle φ, +Z → φ + 90°) and
//! translation `Y = APPEAR_DISTANCE·(s − 1)`. Bar lines use the same transform.

use crate::layout::lane_angle;
use crate::style::VIEW_EXTENT;

/// Camera height above the judgement-line plane (cm).
pub const CAMERA_Y: f32 = 190.0;
/// Distance (cm) below the line plane where notes appear (`RhythmGamePosTable` NoteAppearPos).
pub const APPEAR_DISTANCE: f32 = 600.0;
/// Radius (cm, at scale 1, on the line plane) of the hold-body strip (doc 11 §3.4).
pub const HOLD_RADIUS_CM: f32 = 172.5;

/// Note scale at `depth` (0 = on the line, 1 = spawn).
pub fn note_scale(depth: f32) -> f32 {
    1.0 - depth
}

/// World position of local mesh point `p` on a note at scale `s` whose first lane is `lane`.
pub fn note_to_world(p: [f32; 3], s: f32, lane: f32) -> [f32; 3] {
    let (sin, cos) = lane_angle(lane).sin_cos();
    let (x, z) = (p[0] * cos - p[2] * sin, p[0] * sin + p[2] * cos);
    [s * x, APPEAR_DISTANCE * (s - 1.0) + s * p[1], s * z]
}

/// `[X, Z, w]` of a world point: screen-axis position (cm) and view depth (cm) — the clip
/// coordinates `(X, Z, ·, w)` of the 90° camera.
pub fn world_to_view([x, y, z]: [f32; 3]) -> [f32; 3] {
    [x, z, CAMERA_Y - y]
}

/// Playfield position of a `world_to_view` point.
pub fn project([x, y, w]: [f32; 3]) -> [f32; 2] {
    [VIEW_EXTENT * x / w, VIEW_EXTENT * y / w]
}

/// Screen radius (playfield units) of a local point at radius `rho` and height `y` (cm) on a
/// note at scale `s`: `s·rho / (790 − s·(600 + y))` in NDC.
pub fn project_radius(s: f32, rho: f32, y: f32) -> f32 {
    VIEW_EXTENT * s * rho / (CAMERA_Y + APPEAR_DISTANCE - s * (APPEAR_DISTANCE + y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(p: [f32; 3], s: f32, lane: f32) -> [f32; 3] {
        world_to_view(note_to_world(p, s, lane))
    }

    #[test]
    fn transform_matches_the_captured_note_matrices() {
        // 12.0 s capture: s = 0.81994 -> Y = -108.036; lane 45: local +X goes to world -Z.
        let w = note_to_world([100.0, 0.0, 0.0], 0.81994, 45.0);
        assert!(w[0].abs() < 1e-3 && (w[2] + 81.994).abs() < 1e-3);
        assert!((w[1] + 108.036).abs() < 1e-2);
        // d = 790 - s(600 + y_l).
        let [_, _, d] = place([0.0, 10.0, 0.0], 0.5, 0.0);
        assert!((d - (790.0 - 0.5 * 610.0)).abs() < 1e-3);
    }

    #[test]
    fn radius_matches_the_full_transform() {
        let p = [150.0, 7.5, 0.0];
        let [x, y] = project(place(p, 0.7, 23.0));
        assert!((x.hypot(y) - project_radius(0.7, 150.0, 7.5)).abs() < 1e-5);
    }

    /// The E2 fit (outer note edge, `r = R(1-k)x/(1-kx)`, k 0.750, T 2.184 s at speed 1.5, rim R
    /// 291.8 px of the 675 px view) is reproduced by the captured projection of the note strip's
    /// outer edge (UV0 u = 1: r 174.12, y 7.5) within the fit's own error (RMS 1.3 px) once its
    /// time origin is shifted (worst sample < 2 px): the edge crosses the rim ~50 ms before the
    /// note is on the line.
    #[test]
    fn projection_reproduces_the_e2_radius_curve() {
        const RIM_PX: f64 = 291.8;
        let e2 = |tau: f64| {
            let x = 1.0 - (tau - 0.049) / 2.184;
            RIM_PX * 0.25 * x / (1.0 - 0.75 * x)
        };
        let (mut worst, mut sq, mut n) = (0.0f64, 0.0, 0);
        for i in 0..400 {
            let tau = f64::from(i) / 400.0 * 2.2;
            let r_e2 = e2(tau);
            if !(30.0..RIM_PX).contains(&r_e2) {
                continue;
            }
            let s = 1.0 - tau / (crate::layout::APPROACH_SECS / 1.5);
            let r = project_radius(s as f32, 174.12, 7.5);
            let px = f64::from(r) * RIM_PX;
            worst = worst.max((px - r_e2).abs());
            sq += (px - r_e2).powi(2);
            n += 1;
        }
        let rms = (sq / f64::from(n)).sqrt();
        assert!(
            n > 100 && worst < 2.0 && rms < 0.6,
            "n {n} worst {worst} rms {rms}"
        );
    }

    #[test]
    fn lanes_rotate_counter_clockwise() {
        let [x, y] = project(place([100.0, 0.0, 0.0], 1.0, 15.0));
        assert!(x.abs() < 1e-4 && y > 0.0, "lane 15 is at 12 o'clock");
        // Mesh +Z is the next lane (counter-clockwise).
        let [_, y] = project(place([100.0, 0.0, 10.0], 1.0, 0.0));
        assert!(y > 0.0);
    }

    #[test]
    fn spawn_point_collapses_to_the_centre() {
        let [x, y] = project(place([170.0, 10.0, 20.0], 0.0, 7.0));
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6);
        assert_eq!(note_scale(1.0), 0.0);
    }
}
