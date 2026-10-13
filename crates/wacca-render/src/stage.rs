//! The play view's 3D elements other than the note bodies (pure, no GPU): the guide-lane cone
//! (which also draws the judgement ring), the hold ribbons, the slide chevrons and snap arrows.
//! Spec: `docs/research/11-shaders-and-meshes.md` §2.5, §3.2–3.4 (RenderDoc captures of the
//! real game). Drawn by `stage_gpu` with the ported pixel shaders in `stage.wgsl`.
//!
//! Every vertex is a [`MeshVertex`] in view space (`[X, Z, w]`, see [`crate::camera`]).

use wacca_chart::{Chart, HoldNode, LANES, NoteBody, SlideDir, SnapDir};

use crate::camera::{APPEAR_DISTANCE, HOLD_RADIUS_CM, note_scale, note_to_world, world_to_view};
use crate::layout::{last_drawable_time, mask_state, raw_depth};
use crate::notes3d::{MeshKind, MeshVertex, NoteMesh, NoteMeshes};
use crate::scene::{LiveState, SceneOptions};

/// Guide-lane cone (`vMX_2_GuideLane_000`, one dynamic mesh per lane, frame 12365): rim radius
/// and height of its open end, height of its apex (the note spawn point).
pub const LANE_RIM_RADIUS: f32 = 218.75;
pub const LANE_RIM_Y: f32 = 150.0;
pub const LANE_APEX_Y: f32 = -APPEAR_DISTANCE;

/// Hold ribbon (`vMX_2_Hold_001`, frame 12365): one row of [`HOLD_COLUMNS`] vertices per
/// [`HOLD_ROW_SECS`] of chart time, inset [`HOLD_INSET_DEG`] from the lane edges on both sides.
// UNVERIFIED: rows per chart time (1/60 s) or per scroll distance (4.5 cm) — equal at speed 1.5.
pub const HOLD_ROW_SECS: f64 = 1.0 / 60.0;
pub const HOLD_COLUMNS: usize = 16;
pub const HOLD_INSET_DEG: f32 = 4.0;
/// UV0.u at the last column (the strip covers half the texture tile).
pub const HOLD_U_SPAN: f32 = 0.5;

/// Chevron material, as [`MeshVertex::material`] `kind | width << 8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Chevron {
    /// `vMX_2_SlideL_002` (clockwise slides).
    SlideL,
    /// `vMX_2_SlideR_002`.
    SlideR,
    /// `vMX_2_SnapIn_001` (red, inward).
    SnapIn,
    /// `vMX_2_SnapOut_001`.
    SnapOut,
}

impl Chevron {
    fn of(body: &NoteBody) -> Option<(MeshKind, Self)> {
        match body {
            NoteBody::Slide(SlideDir::Cw) => Some((MeshKind::Slide, Self::SlideL)),
            NoteBody::Slide(SlideDir::Ccw) => Some((MeshKind::Slide, Self::SlideR)),
            NoteBody::Snap(SnapDir::In) => Some((MeshKind::Snap, Self::SnapIn)),
            NoteBody::Snap(SnapDir::Out) => Some((MeshKind::Snap, Self::SnapOut)),
            _ => None,
        }
    }
}

/// One frame of the 3D stage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    /// Guide-lane triangles of the lanes that are on (masked lanes are not drawn).
    pub lanes: Vec<MeshVertex>,
    /// Hold ribbons (one triangle list for all holds).
    pub holds: Vec<MeshVertex>,
    /// Slide chevrons and snap arrows (mesh sections at and above the note width).
    pub chevrons: Vec<MeshVertex>,
    /// A background movie plays (guide-lane fade parameters, `RhythmGameAppearanceTable`).
    pub movie: bool,
    /// `View.GameTime` (s): scrolls the guide-lane noise.
    pub time: f32,
    /// `MPC_BpmSignal` second scalar: the judgement ring's glow pulse.
    pub pulse: f32,
}

impl Stage {
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty() && self.holds.is_empty() && self.chevrons.is_empty()
    }

    /// Empties the stage, keeping the buffers.
    pub fn clear(&mut self) {
        self.lanes.clear();
        self.holds.clear();
        self.chevrons.clear();
        self.movie = false;
        self.time = 0.0;
        self.pulse = 0.0;
    }
}

fn vertex(world: [f32; 3], uv: [f32; 2], material: u32) -> MeshVertex {
    MeshVertex {
        pos: world_to_view(world),
        uv,
        material,
    }
}

fn polar(radius: f32, deg: f32, y: f32) -> [f32; 3] {
    let (sin, cos) = deg.to_radians().sin_cos();
    [radius * cos, y, radius * sin]
}

/// Guide-lane triangle of `lane` exactly as the game builds it: vertices P2, P1, P0 (the
/// captured index order 2, 1, 0) with P0/P2 on the rim at the lane's edges and P1 the apex,
/// UV0 (0.5, 0), (0, 1), (0, 0). The game's second triangle (3, 1, 2) is degenerate (P3 = P1).
pub fn lane_triangle(lane: u8) -> [MeshVertex; 3] {
    let a0 = 6.0 * f32::from(lane);
    [
        vertex(polar(LANE_RIM_RADIUS, a0 + 6.0, LANE_RIM_Y), [0.5, 0.0], 0),
        vertex([0.0, LANE_APEX_Y, 0.0], [0.0, 1.0], 0),
        vertex(polar(LANE_RIM_RADIUS, a0, LANE_RIM_Y), [0.0, 0.0], 0),
    ]
}

/// Guide-lane `v` of the judgement ring's peak: the guide-lane shader's ring term
/// `sin(2π(v/2 + JudgeLine_Pos))` is largest at `v/2 + 0.1473 = 0.25`.
pub const RING_PEAK_V: f32 = 2.0 * (0.25 - 0.1473);

/// World point on lane `lane`'s centre line of the cone at guide-lane `v`.
pub fn lane_point(lane: f32, v: f32) -> [f32; 3] {
    // The rim chord's midpoint, moved towards the apex.
    let chord = LANE_RIM_RADIUS * 3f32.to_radians().cos();
    let y = LANE_RIM_Y + (LANE_APEX_Y - LANE_RIM_Y) * v;
    polar(chord * (1.0 - v), 6.0 * lane + 3.0, y)
}

/// Lane range `(start, width)` (unwrapped lanes) of a hold at chart time `t`: linear between
/// the surrounding control points (the generated joints in between are ignored).
pub fn hold_arc(nodes: &[HoldNode], t: f64) -> Option<(f32, f32)> {
    let mut control = nodes.iter().filter(|n| n.control);
    let mut a = control.next()?;
    for b in control {
        if t <= b.time {
            let span = b.time - a.time;
            let f = if span > 0.0 {
                ((t - a.time) / span).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            let (wa, wb) = (f32::from(a.arc.width), f32::from(b.arc.width));
            let lane = a.lane_unwrapped + (b.lane_unwrapped - a.lane_unwrapped) * f;
            return Some((lane, wa + (wb - wa) * f));
        }
        a = b;
    }
    Some((a.lane_unwrapped, f32::from(a.arc.width)))
}

/// One ribbon row: [`HOLD_COLUMNS`] world points on the circle `r = 172.5·s` at
/// `Y = 600(s − 1)` across the hold's lanes (inset), and its UV0.v.
fn hold_row(s: f32, (lane, width): (f32, f32), v: f32) -> [([f32; 3], [f32; 2]); HOLD_COLUMNS] {
    let a0 = 6.0 * lane + HOLD_INSET_DEG;
    let a1 = 6.0 * (lane + width) - HOLD_INSET_DEG;
    let y = APPEAR_DISTANCE * (s - 1.0);
    std::array::from_fn(|j| {
        let f = j as f32 / (HOLD_COLUMNS - 1) as f32;
        (
            polar(HOLD_RADIUS_CM * s, a0 + (a1 - a0) * f, y),
            [HOLD_U_SPAN * f, v],
        )
    })
}

/// Appends the ribbon of a hold with `nodes`; `scale_at(t)` is the note scale `s` of chart time
/// `t` (1 on the line, 0 at spawn). Rows run from the head every [`HOLD_ROW_SECS`] to the tail;
/// UV0.v is the fraction of the hold's duration. Rows beyond the line are dropped (the first
/// visible row is put on the line) and the first row beyond the spawn point is put on it and
/// ends the ribbon (captured: the last row sits at Y = −600).
// UNVERIFIED: v as the duration fraction (captured: v = row/170 on a 170-frame hold) and the
// clamping at the line (the capture shows a hold before it reached the line).
pub fn push_hold_ribbon(
    out: &mut Vec<MeshVertex>,
    nodes: &[HoldNode],
    scale_at: impl Fn(f64) -> f64,
) {
    let (Some(first), Some(last)) = (nodes.first(), nodes.last()) else {
        return;
    };
    let duration = last.time - first.time;
    if duration <= 0.0 {
        return;
    }
    let rows = (duration / HOLD_ROW_SECS).ceil() as usize;
    let mut prev: Option<[([f32; 3], [f32; 2]); HOLD_COLUMNS]> = None;
    let mut pending: Option<(f64, f64)> = None;
    for k in 0..=rows {
        let t = (first.time + k as f64 * HOLD_ROW_SECS).min(last.time);
        let s = scale_at(t);
        if s >= 1.0 {
            pending = Some((t, 1.0));
            continue;
        }
        let rows_here = pending.take().into_iter().chain([(t, s.max(0.0))]);
        let mut done = false;
        for (t, s) in rows_here {
            let Some(arc) = hold_arc(nodes, t) else {
                return;
            };
            let row = hold_row(s as f32, arc, ((t - first.time) / duration) as f32);
            if let Some(p) = &prev {
                push_ribbon_quads(out, p, &row);
            }
            prev = Some(row);
            done = s <= 0.0;
        }
        if done {
            break;
        }
    }
}

/// Quads between two rows with the captured index pattern (2, 1, 0), (3, 1, 2) on vertices
/// `2j` = row `a` column `j`, `2j + 1` = row `b` column `j`.
fn push_ribbon_quads(
    out: &mut Vec<MeshVertex>,
    a: &[([f32; 3], [f32; 2]); HOLD_COLUMNS],
    b: &[([f32; 3], [f32; 2]); HOLD_COLUMNS],
) {
    let v = |(p, uv): ([f32; 3], [f32; 2])| vertex(p, uv, 0);
    for j in 0..HOLD_COLUMNS - 1 {
        out.extend([a[j + 1], b[j], a[j], b[j + 1], b[j], a[j + 1]].map(v));
    }
}

/// Appends the chevron/arrow sections (material slot at or above `width`) of a slide or snap
/// note mesh, placed like the note body.
pub fn push_chevrons(
    out: &mut Vec<MeshVertex>,
    mesh: &NoteMesh,
    s: f32,
    lane: f32,
    width: u8,
    kind: Chevron,
) {
    let material = kind as u32 | u32::from(width) << 8;
    out.extend(
        mesh.corners
            .iter()
            .filter(|c| c.2 >= u32::from(width))
            .map(|&(p, uv, _)| vertex(note_to_world(p, s, lane), uv, material)),
    );
}

/// The stage at chart time `now` (into `stage`, buffers reused). Chevrons need the note
/// meshes (without them the 2D notes keep their own arrows).
pub fn build_stage_into(
    stage: &mut Stage,
    chart: &Chart,
    now: f64,
    opts: &SceneOptions,
    live: &LiveState,
    meshes: Option<&NoteMeshes>,
) {
    stage.clear();
    stage.movie = opts.movie;
    stage.time = now as f32;
    // UNVERIFIED: the MPC_BpmSignal pulse taken as the beat phase.
    stage.pulse = (chart.sec_to_tick(now) / BEAT_TICKS).rem_euclid(1.0) as f32;

    let mask = mask_state(&chart.masks, now);
    for lane in (0..LANES).filter(|&l| mask[usize::from(l)]) {
        stage.lanes.extend(lane_triangle(lane));
    }

    let last_time = last_drawable_time(chart, now);
    let now_dist = chart.dist_at(chart.view_time(now));
    let depth = |t: f64| raw_depth(chart, t, now_dist, opts.note_speed);
    let judged = |i: usize| live.judged.get(i).copied().unwrap_or(false);
    for (i, note) in chart.notes.iter().enumerate() {
        if judged(i) || last_time.is_some_and(|last| note.time > last) {
            continue;
        }
        match &note.body {
            NoteBody::Hold { nodes } => {
                push_hold_ribbon(&mut stage.holds, nodes, |t| 1.0 - depth(t));
            }
            body => {
                let (Some(meshes), Some((kind, chevron))) = (meshes, Chevron::of(body)) else {
                    continue;
                };
                let d = depth(note.time);
                let width = note.arc.width.clamp(1, LANES);
                if let Some(mesh) = (0.0..=1.0)
                    .contains(&d)
                    .then(|| meshes.get(kind, width))
                    .flatten()
                {
                    let lane = f32::from(note.arc.lane);
                    push_chevrons(
                        &mut stage.chevrons,
                        mesh,
                        note_scale(d as f32),
                        lane,
                        width,
                        chevron,
                    );
                }
            }
        }
    }
}

/// Ticks per beat (quarter note) of the chart's 1920-tick measure.
const BEAT_TICKS: f64 = 480.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::project;
    use crate::style::VIEW_EXTENT;
    use wacca_chart::{Arc, BeatPos};

    /// Pixels of the real 675 px view per NDC unit.
    const PX: f32 = 337.5;

    /// Window pixel offset from the view centre (x right, y up) of a view-space vertex.
    fn px(v: &MeshVertex) -> [f32; 2] {
        project(v.pos).map(|c| c / VIEW_EXTENT * PX)
    }

    fn near(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < tol
    }

    fn node(time: f64, lane: f32, width: u8, control: bool) -> HoldNode {
        HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc {
                lane: lane.rem_euclid(60.0) as u8,
                width,
            },
            lane_unwrapped: lane,
            control,
        }
    }

    #[test]
    fn lane_triangle_is_the_captured_cone_section() {
        // Frame 12365, lane 45 (270°..276°): rim corners at r 218.75, Y 150 and the apex.
        let t = lane_triangle(45);
        let w = |v: &MeshVertex| v.pos[2];
        assert!((w(&t[1]) - 790.0).abs() < 1e-3 && t[1].pos[0] == 0.0 && t[1].pos[1] == 0.0);
        assert!((w(&t[0]) - 40.0).abs() < 1e-3 && (w(&t[2]) - 40.0).abs() < 1e-3);
        let a = |v: &MeshVertex| v.pos[1].atan2(v.pos[0]).to_degrees().rem_euclid(360.0);
        assert!((a(&t[2]) - 270.0).abs() < 1e-3 && (a(&t[0]) - 276.0).abs() < 1e-3);
        assert!((t[0].pos[0].hypot(t[0].pos[1]) - 218.75).abs() < 1e-3);
        assert_eq!(
            [t[0].uv, t[1].uv, t[2].uv],
            [[0.5, 0.0], [0.0, 1.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn judgement_ring_peak_is_where_the_capture_puts_it() {
        // doc 11 §3.3: v 0.2054 -> Y -4.05, r 173.8 at the lane edges, w 194.05, 302 px of
        // 337.5 (E2 measured the band at 292–315 px). Mid-lane the flat triangle is cos 3° nearer.
        assert!((RING_PEAK_V - 0.2054).abs() < 1e-4);
        let p = lane_point(0.0, RING_PEAK_V);
        assert!((p[1] + 4.05).abs() < 0.01, "{p:?}");
        let chord = 3f32.to_radians().cos();
        let r = p[0].hypot(p[2]);
        assert!((r - 173.82 * chord).abs() < 0.05, "{r}");
        let v = vertex(p, [0.0; 2], 0);
        let edge_px = 337.5 * 173.82 / 194.05;
        assert!((px(&v)[0].hypot(px(&v)[1]) - edge_px * chord).abs() < 0.5);
    }

    /// Frame 12365 (`cap3/d12365b`, hold lanes 40..50 -> 0..15 over 170 frames): the captured
    /// rows' world positions, reproduced within 0.5 px on screen.
    #[test]
    fn hold_ribbon_matches_the_captured_rows() {
        // Captured: row 0 Y -195.7676 (s 0.67372), rows every Δs 0.0075035, 91 rows, the last at
        // Y -600; angles 244..296 at row 0, 256..311 at row 17, 304..371 at row 85.
        let s0 = 0.673_72;
        let ds = 0.007_503_5;
        let dur = 2.834_6;
        let nodes = [
            node(0.0, 40.0, 10, true),
            node(1.0, 50.0, 12, false),
            node(dur, 60.0, 15, true),
        ];
        let mut out = Vec::new();
        let scale = |t: f64| s0 - t / HOLD_ROW_SECS * ds;
        push_hold_ribbon(&mut out, &nodes, scale);
        let quads = out.len() / 6;
        assert_eq!(quads, 90 * (HOLD_COLUMNS - 1), "90 row pairs of 15 quads");
        // Row k column j is corner 2 of quad j of row pair k (and corner 1 of the last pair).
        let corner = |k: usize, j: usize| {
            if k < 90 {
                out[(k * 15 + j.min(14)) * 6 + if j == 15 { 0 } else { 2 }]
            } else {
                out[(89 * 15 + j.min(14)) * 6 + if j == 15 { 3 } else { 1 }]
            }
        };
        let captured = |s: f32, deg: f32| {
            let y = 600.0 * (s - 1.0);
            px(&vertex(polar(172.5 * s, deg, y), [0.0; 2], 0))
        };
        for (k, a0, a1) in [(0, 244.0, 296.0), (17, 256.0, 311.0), (85, 304.0, 371.0)] {
            let s = s0 as f32 - k as f32 * ds as f32;
            assert!(
                near(px(&corner(k, 0)), captured(s, a0), 0.5),
                "row {k} start"
            );
            assert!(
                near(px(&corner(k, 15)), captured(s, a1), 0.5),
                "row {k} end"
            );
        }
        let last = corner(90, 0);
        assert!(
            (last.pos[2] - 790.0).abs() < 1e-3,
            "tail clamped to the spawn point"
        );
        assert!((corner(85, 0).uv[1] - 0.5).abs() < 2e-3 && corner(0, 15).uv == [0.5, 0.0]);
    }

    #[test]
    fn hold_ribbon_starts_on_the_line_once_the_head_passed() {
        let nodes = [node(0.0, 0.0, 4, true), node(1.0, 0.0, 4, true)];
        let mut out = Vec::new();
        // Head 0.2 s past the line, 1 s visible window.
        push_hold_ribbon(&mut out, &nodes, |t| 1.2 - t);
        let max_w = out.iter().map(|v| v.pos[2]).fold(0.0, f32::max);
        let min_w = out.iter().map(|v| v.pos[2]).fold(f32::MAX, f32::min);
        assert!(
            (min_w - 190.0).abs() < 1e-3,
            "first row on the line: {min_w}"
        );
        assert!(max_w < 790.0 - 100.0, "tail still visible");
        out.clear();
        push_hold_ribbon(&mut out, &nodes, |t| 2.0 - t);
        assert!(out.is_empty(), "hold fully past");
    }

    #[test]
    fn hold_arc_uses_control_points_only() {
        let nodes = [
            node(0.0, 40.0, 10, true),
            node(0.5, 40.0, 11, false),
            node(2.0, 60.0, 14, true),
            node(3.0, 61.0, 14, true),
        ];
        assert_eq!(hold_arc(&nodes, 0.5), Some((45.0, 11.0)));
        assert_eq!(hold_arc(&nodes, 2.5), Some((60.5, 14.0)));
        assert_eq!(hold_arc(&nodes, 9.0), Some((61.0, 14.0)));
    }

    #[test]
    fn chevrons_are_the_sections_above_the_width() {
        let mesh = NoteMesh {
            corners: vec![
                ([170.0, 0.0, 0.0], [0.0, 0.0], 0),
                ([150.0, 1.0, 0.0], [0.5, 0.2], 3),
                ([130.0, 1.0, 0.0], [1.0, 0.4], 4),
            ],
        };
        let mut out = Vec::new();
        push_chevrons(&mut out, &mesh, 1.0, 15.0, 3, Chevron::SlideR);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].material, 1 | 3 << 8);
        // Lane 15 = 90°: local +X goes to screen up.
        assert!(out[0].pos[0].abs() < 1e-3 && (out[0].pos[1] - 150.0).abs() < 1e-3);
    }
}
