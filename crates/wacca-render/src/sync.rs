//! Sync connector: the cyan line along the ring between simultaneous notes (pure, no GPU).
//! Spec: `docs/research/11-shaders-and-meshes.md` §3.9.
//!
//! Mesh `vMsh_GuideSync00_NN_s`: a flat band (y = 0, r 168.6–171.9) of N per-lane sections, each
//! spanning its lane −0.19°…6.19° with UV0 u = 0 → 1 across the band (inner → outer), v = 0 → 1
//! along the lane. Material `vMX_2_Sync_000`: opaque (no blend mode set), unlit, emissive
//! `tex.rgb · tex.b · (1, 3, 3)` of `vT_SyncNote00_00_m` (sRGB). Placed like a note (scale s of
//! the notes' time, rotation by its first lane) and drawn with the opaque notes.
//!
//! Which notes it joins (no RenderDoc frame has one; KING reference frames, e.g. two R notes at
//! lanes 32+13 / 45+13 and two touches at 28+12 / 50+12): neighbours of a simultaneous group around
//! the ring, along the shorter way — the line crosses the gap between them and runs on under the
//! last lane of one note and the first lane of the other (seen through their end caps, hidden by
//! the opaque bodies). UNVERIFIED: groups of three or more (every gap but the widest is joined),
//! and whether the line spans exactly one lane under each note.

use wacca_chart::{Arc, LANES};

use crate::notes3d::{MaterialId, MeshKind, MeshVertex, NoteMeshes, push_mesh};

/// One connector: `width` lanes counter-clockwise from `lane` (a `vMsh_GuideSync00` width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub lane: u8,
    pub width: u8,
}

/// The connectors of one group of simultaneous notes (`arcs`, any order).
pub fn spans(arcs: &[Arc]) -> Vec<Span> {
    let lanes = i32::from(LANES);
    let mut sorted: Vec<(i32, i32)> = arcs
        .iter()
        .map(|a| {
            (
                i32::from(a.lane) % lanes,
                i32::from(a.width.clamp(1, LANES)),
            )
        })
        .collect();
    if sorted.len() < 2 {
        return Vec::new();
    }
    sorted.sort_unstable();
    // Gap from each note's end to the next note's start (counter-clockwise, wrapping once).
    let gaps: Vec<(i32, i32)> = (0..sorted.len())
        .map(|i| {
            let (lane, width) = sorted[i];
            let next = sorted.get(i + 1).map_or(sorted[0].0 + lanes, |n| n.0);
            (lane + width, next - (lane + width))
        })
        .collect();
    // The widest gap is the way round the back: left open.
    let widest = gaps
        .iter()
        .enumerate()
        .max_by_key(|(i, g)| (g.1, std::cmp::Reverse(*i)))
        .map(|(i, _)| i);
    gaps.iter()
        .enumerate()
        .filter(|&(i, &(_, gap))| Some(i) != widest && gap >= 0)
        .filter_map(|(_, &(end, gap))| {
            let width = u8::try_from(gap + 2).ok().filter(|&w| w <= LANES)?;
            let lane = u8::try_from((end - 1).rem_euclid(lanes)).ok()?;
            Some(Span { lane, width })
        })
        .collect()
}

/// Appends the connector triangles of `heads` (time, depth, arc of every visible simultaneous
/// note head, sorted by time) to `out`, as opaque [`MaterialId::Sync`] note geometry.
pub fn push_connectors(out: &mut Vec<MeshVertex>, meshes: &NoteMeshes, heads: &[(f64, f32, Arc)]) {
    let mut aura = Vec::new();
    for group in heads.chunk_by(|a, b| a.0 == b.0) {
        let arcs: Vec<Arc> = group.iter().map(|h| h.2).collect();
        for span in spans(&arcs) {
            if let Some(mesh) = meshes.get(MeshKind::GuideSync, span.width) {
                let target = (&mut *out, &mut aura);
                let lane = f32::from(span.lane);
                push_mesh(
                    target,
                    mesh,
                    group[0].1,
                    lane,
                    span.width,
                    MaterialId::Sync as u32,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes3d::NoteMesh;

    fn arc(lane: u8, width: u8) -> Arc {
        Arc { lane, width }
    }

    #[test]
    fn pairs_join_the_shorter_way() {
        // KING m45: touches 28+12 and 50+12 → the 10-lane gap below (40..49), not the 26 above.
        assert_eq!(
            spans(&[arc(50, 12), arc(28, 12)]),
            [Span {
                lane: 39,
                width: 12
            }]
        );
        // KING m27: R notes 32+13 and 45+13 touch: the line still shows between the end caps.
        assert_eq!(
            spans(&[arc(32, 13), arc(45, 13)]),
            [Span { lane: 44, width: 2 }]
        );
        // Across lane 0: 55+3 and 2+2 → gap 58..1 (4 lanes) from lane 57.
        assert_eq!(
            spans(&[arc(55, 3), arc(2, 2)]),
            [Span { lane: 57, width: 6 }]
        );
    }

    #[test]
    fn groups_leave_the_widest_gap_open() {
        let s = spans(&[arc(0, 5), arc(10, 5), arc(25, 5)]);
        assert_eq!(
            s,
            [
                Span { lane: 4, width: 7 },
                Span {
                    lane: 14,
                    width: 12
                }
            ]
        );
    }

    #[test]
    fn single_and_overlapping_notes_have_no_line() {
        assert!(spans(&[arc(3, 4)]).is_empty());
        assert!(spans(&[arc(0, 10), arc(5, 10)]).is_empty(), "overlap");
        assert!(spans(&[arc(0, 60), arc(10, 4)]).is_empty());
    }

    #[test]
    fn connectors_are_placed_per_group() {
        let tri = NoteMesh {
            colors: Vec::new(),
            corners: vec![([170.0, 0.0, 1.0], [0.5, 0.5], 0); 3],
        };
        let mut meshes = NoteMeshes::default();
        meshes.insert(MeshKind::GuideSync, 12, tri.clone());
        meshes.insert(MeshKind::GuideSync, 2, tri);
        let heads = [
            (1.0, 0.25, arc(28, 12)),
            (1.0, 0.25, arc(50, 12)),
            (2.0, 0.5, arc(32, 13)),
            (2.0, 0.5, arc(45, 13)),
        ];
        let mut out = Vec::new();
        push_connectors(&mut out, &meshes, &heads);
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|v| v.material == MaterialId::Sync as u32));
        assert!(out[0].pos[2] < out[3].pos[2], "each group at its own depth");
    }
}
