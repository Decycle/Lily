//! The Mirror play option as a pure chart transform (`docs/research/04-gameplay-mechanics.md` §9
//! option list): left ↔ right across the vertical axis of the ring.
//!
//! With lane 0 at 3 o'clock and lanes increasing counter-clockwise, the arc `[l, l + w)` reflects
//! to `[30 − l − w, 30 − l)`. Slides swap CW ↔ CCW, mask sweeps swap Plus ↔ Minus, snaps are
//! unchanged.
// UNVERIFIED: the reflection axis and formula are derived (doc 04), not checked in the real game
// (only the lane convention they rest on is measured, `docs/research/09-e2-results.md` §0).

use crate::model::{Arc, Chart, HoldNode, LANES, MaskDir, NoteBody, SlideDir};

/// Half a turn; the mirror axis runs through the lane boundaries 15 (12 o'clock) and 45.
const AXIS: i32 = LANES as i32 / 2;

/// `chart` with every lane reflected. Mirroring twice gives the original chart back.
pub fn mirrored(chart: &Chart) -> Chart {
    let mut c = chart.clone();
    for n in &mut c.notes {
        n.arc = mirror_arc(n.arc);
        match &mut n.body {
            NoteBody::Slide(d) => {
                *d = match d {
                    SlideDir::Cw => SlideDir::Ccw,
                    SlideDir::Ccw => SlideDir::Cw,
                }
            }
            NoteBody::Hold { nodes } => mirror_hold(nodes),
            NoteBody::Touch | NoteBody::Snap(_) | NoteBody::Chain => {}
        }
    }
    for m in &mut c.masks {
        m.arc = mirror_arc(m.arc);
        m.dir = match m.dir {
            MaskDir::Plus => MaskDir::Minus,
            MaskDir::Minus => MaskDir::Plus,
            MaskDir::Center => MaskDir::Center,
        };
    }
    c
}

/// `[l, l + w)` → `[(30 − l − w) mod 60, …)` with the same width.
pub fn mirror_arc(a: Arc) -> Arc {
    let lane = (AXIS - i32::from(a.lane) - i32::from(a.width)).rem_euclid(i32::from(LANES));
    Arc {
        lane: lane as u8,
        width: a.width,
    }
}

/// Reflect the nodes' arcs and their unwrapped path (`u → 30 − u − w`, so the hold sweeps the
/// mirrored way), shifted by whole turns so the first node's unwrapped lane is its lane again.
fn mirror_hold(nodes: &mut [HoldNode]) {
    let reflect = |n: &HoldNode| AXIS as f32 - n.lane_unwrapped - f32::from(n.arc.width);
    let Some(first) = nodes.first() else { return };
    let turns = f32::from(mirror_arc(first.arc).lane) - reflect(first);
    for n in nodes.iter_mut() {
        n.lane_unwrapped = reflect(n) + turns;
        n.arc = mirror_arc(n.arc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BeatPos, Bonus, ChartHeader, MaskEvent, Note, SnapDir};

    fn arc(lane: u8, width: u8) -> Arc {
        Arc { lane, width }
    }

    fn note(lane: u8, width: u8, body: NoteBody) -> Note {
        Note {
            pos: BeatPos::default(),
            time: 0.0,
            arc: arc(lane, width),
            bonus: Bonus::None,
            body,
            file_index: 0,
        }
    }

    fn node(lane: u8, width: u8, lane_unwrapped: f32) -> HoldNode {
        HoldNode {
            pos: BeatPos::default(),
            time: 0.0,
            arc: arc(lane, width),
            lane_unwrapped,
        }
    }

    fn chart(notes: Vec<Note>, masks: Vec<MaskEvent>) -> Chart {
        Chart {
            header: ChartHeader::default(),
            notes,
            masks,
            tempo: Vec::new(),
            scroll: Vec::new(),
            reverses: Vec::new(),
            tutorial_tags: Vec::new(),
            end: BeatPos::default(),
            end_time: 0.0,
        }
    }

    #[test]
    fn arcs_reflect_across_the_vertical_axis() {
        // 3 o'clock (lanes 0..2) ↔ 9 o'clock (28..30); 12 o'clock (14..16) stays put.
        assert_eq!(mirror_arc(arc(0, 2)), arc(28, 2));
        assert_eq!(mirror_arc(arc(14, 2)), arc(14, 2));
        assert_eq!(mirror_arc(arc(44, 2)), arc(44, 2));
        assert_eq!(mirror_arc(arc(58, 4)), arc(28, 4));
        assert_eq!(mirror_arc(arc(10, 60)).width, 60);
        for lane in 0..LANES {
            for width in 1..=LANES {
                assert_eq!(mirror_arc(mirror_arc(arc(lane, width))), arc(lane, width));
            }
        }
    }

    #[test]
    fn slides_and_masks_swap_direction_snaps_do_not() {
        let mask = |dir| MaskEvent {
            pos: BeatPos::default(),
            time: 0.0,
            arc: arc(0, 15),
            add: true,
            dir,
        };
        let c = chart(
            vec![
                note(0, 4, NoteBody::Slide(SlideDir::Cw)),
                note(5, 4, NoteBody::Slide(SlideDir::Ccw)),
                note(5, 4, NoteBody::Snap(SnapDir::In)),
            ],
            vec![
                mask(MaskDir::Plus),
                mask(MaskDir::Minus),
                mask(MaskDir::Center),
            ],
        );
        let m = mirrored(&c);
        let bodies: Vec<_> = m.notes.iter().map(|n| n.body.clone()).collect();
        assert_eq!(
            bodies,
            [
                NoteBody::Slide(SlideDir::Ccw),
                NoteBody::Slide(SlideDir::Cw),
                NoteBody::Snap(SnapDir::In)
            ]
        );
        assert_eq!(m.notes[0].arc, arc(26, 4));
        let dirs: Vec<_> = m.masks.iter().map(|k| k.dir).collect();
        assert_eq!(dirs, [MaskDir::Minus, MaskDir::Plus, MaskDir::Center]);
        assert_eq!(m.masks[0].arc, arc(15, 15));
    }

    #[test]
    fn holds_sweep_the_mirrored_way_and_double_mirror_is_identity() {
        // Start at 58, wrap counter-clockwise past 0 to lane 4 while widening 2 → 4.
        let hold = NoteBody::Hold {
            nodes: vec![node(58, 2, 58.0), node(2, 2, 62.0), node(4, 4, 64.0)],
        };
        let c = chart(vec![note(58, 2, hold), note(7, 3, NoteBody::Touch)], vec![]);
        let m = mirrored(&c);
        let NoteBody::Hold { nodes } = &m.notes[0].body else {
            panic!("hold expected")
        };
        let lanes: Vec<_> = nodes.iter().map(|n| (n.arc, n.lane_unwrapped)).collect();
        // Mirrored start [58, 60) → [30, 32), then clockwise (decreasing) instead of CCW.
        assert_eq!(
            lanes,
            [(arc(30, 2), 30.0), (arc(26, 2), 26.0), (arc(22, 4), 22.0)]
        );
        assert_eq!(m.notes[0].arc, nodes[0].arc);
        assert_eq!(mirrored(&m), c);
    }
}
