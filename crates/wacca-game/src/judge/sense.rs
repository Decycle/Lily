//! Pure sensor-grid predicates used by the judge: new presses, arc touches, snap and slide motion.

use crate::input::{DEPTHS, SensorState};
use wacca_chart::{Arc, LANES, SlideDir, SnapDir};

const ALL_LANES: u64 = (1 << LANES) - 1;

/// Slide fires when the mean pressed offset rises this many lanes above its minimum.
// UNVERIFIED: Saturn's value, "chosen based on testing with the original game".
const SLIDE_THRESHOLD: f64 = 1.9;
/// Larger jumps are rejected as unrelated input (more than half a circle).
// UNVERIFIED: Saturn.
const SLIDE_MAX: f64 = 30.0;

/// Lane `offset` steps counter-clockwise from the arc's start (mod 60).
pub fn lane_at(arc: Arc, offset: u8) -> u8 {
    ((arc.lane as u16 + offset as u16) % LANES as u16) as u8
}

pub fn arcs_overlap(a: Arc, b: Arc) -> bool {
    a.contains(b.lane) || b.contains(a.lane)
}

/// Any sensor (any depth) inside `arc` is on.
pub fn arc_touched(s: &SensorState, arc: Arc) -> bool {
    (0..arc.width).any(|o| s.lane_touched(lane_at(arc, o)))
}

/// Bit set of lanes in `arc`.
pub fn arc_mask(arc: Arc) -> u64 {
    (0..arc.width).fold(0, |m, o| m | 1 << lane_at(arc, o))
}

/// Lanes with a sensor (any depth) that went off→on between two frames.
pub fn new_presses(prev: &SensorState, cur: &SensorState) -> u64 {
    (0..DEPTHS).fold(0, |m, d| m | (cur.rings[d] & !prev.rings[d])) & ALL_LANES
}

/// Min/max pressed depth over `lanes`, if any is pressed.
fn depth_range(s: &SensorState, lanes: impl Iterator<Item = u8> + Clone) -> Option<(usize, usize)> {
    let depths = (0..DEPTHS).filter(|&d| lanes.clone().any(|l| s.get(l, d)));
    let min = depths.clone().next()?;
    Some((min, depths.last().unwrap_or(min)))
}

/// Radial motion across depth rings inside `arc` between two frames. Checks every single lane
/// and every adjacent pair; `In` (toward the centre, depth 0) fires when the min or max pressed
/// depth decreased, `Out` when it increased.
// UNVERIFIED: Saturn `SnapNote.Snapped`; one ring of movement suffices.
pub fn snapped(prev: &SensorState, cur: &SensorState, arc: Arc, dir: SnapDir) -> bool {
    (0..arc.width).any(|i| {
        (1..=2u8).filter(|k| i + k <= arc.width).any(|k| {
            let lanes = (i..i + k).map(|o| lane_at(arc, o));
            match (depth_range(prev, lanes.clone()), depth_range(cur, lanes)) {
                (Some(p), Some(c)) => match dir {
                    SnapDir::In => c.0 < p.0 || c.1 < p.1,
                    SnapDir::Out => c.0 > p.0 || c.1 > p.1,
                },
                _ => false,
            }
        })
    })
}

/// Sub-arcs tracked for a slide: full-circle slides use four half-circle arcs so a swipe
/// can cross the start edge.
// UNVERIFIED: Saturn `FullCircleSwipeNote` (sub-notes at 0/15/30/45, width 30).
pub fn slide_spans(arc: Arc) -> Vec<Arc> {
    if arc.width < LANES {
        return vec![arc];
    }
    (0..4)
        .map(|i| Arc {
            lane: lane_at(arc, i * 15),
            width: 30,
        })
        .collect()
}

/// Mean offset (lanes from the arc's trailing edge in the slide direction) of all pressed
/// sensors in `arc`, each depth counted separately. Lanes increase counter-clockwise.
pub fn slide_position(s: &SensorState, arc: Arc, dir: SlideDir) -> Option<f64> {
    let (mut sum, mut n) = (0u32, 0u32);
    for off in 0..arc.width {
        let lane = match dir {
            SlideDir::Ccw => lane_at(arc, off),
            SlideDir::Cw => lane_at(arc, arc.width - 1 - off),
        };
        let hits = (0..DEPTHS).filter(|&d| s.get(lane, d)).count() as u32;
        sum += off as u32 * hits;
        n += hits;
    }
    (n > 0).then(|| sum as f64 / n as f64)
}

/// Slide motion from the lowest position seen so far to `pos`.
pub fn slid(min: f64, pos: f64) -> bool {
    let d = pos - min;
    d > SLIDE_THRESHOLD && d < SLIDE_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(cells: &[(u8, usize)]) -> SensorState {
        let mut s = SensorState::default();
        for &(l, d) in cells {
            s.set(l, d, true);
        }
        s
    }

    fn a(lane: u8, width: u8) -> Arc {
        Arc { lane, width }
    }

    #[test]
    fn new_presses_and_masks() {
        let prev = s(&[(10, 0)]);
        let cur = s(&[(10, 0), (10, 1), (11, 1), (59, 3)]);
        assert_eq!(new_presses(&prev, &cur), 1 << 10 | 1 << 11 | 1 << 59);
        assert_eq!(new_presses(&cur, &cur), 0);
        assert_eq!(arc_mask(a(58, 3)), 1 << 58 | 1 << 59 | 1);
        assert_eq!(arc_mask(a(7, 60)), ALL_LANES);
    }

    #[test]
    fn touch_and_overlap() {
        assert!(arc_touched(&s(&[(1, 3)]), a(58, 4)));
        assert!(!arc_touched(&s(&[(2, 3)]), a(58, 4)));
        assert!(arcs_overlap(a(58, 4), a(1, 3)) && !arcs_overlap(a(58, 4), a(2, 3)));
    }

    #[test]
    fn snap_directions() {
        let outer = s(&[(5, 3)]);
        let inner = s(&[(5, 0)]);
        assert!(snapped(&outer, &inner, a(4, 3), SnapDir::In));
        assert!(!snapped(&outer, &inner, a(4, 3), SnapDir::Out));
        assert!(snapped(&inner, &outer, a(4, 3), SnapDir::Out));
        // Movement between adjacent lanes counts; movement outside the arc does not.
        assert!(snapped(&s(&[(4, 2)]), &s(&[(5, 1)]), a(4, 2), SnapDir::In));
        assert!(!snapped(&outer, &inner, a(6, 3), SnapDir::In));
        assert!(!snapped(
            &SensorState::default(),
            &inner,
            a(4, 3),
            SnapDir::In
        ));
    }

    #[test]
    fn slide_positions() {
        let arc = a(58, 6); // 58..=3
        assert_eq!(
            slide_position(&s(&[(58, 0)]), arc, SlideDir::Ccw),
            Some(0.0)
        );
        assert_eq!(
            slide_position(&s(&[(1, 0), (1, 1)]), arc, SlideDir::Ccw),
            Some(3.0)
        );
        assert_eq!(slide_position(&s(&[(1, 0)]), arc, SlideDir::Cw), Some(2.0));
        assert_eq!(slide_position(&s(&[(10, 0)]), arc, SlideDir::Cw), None);
        assert!(slid(0.0, 2.0) && !slid(0.0, 1.9) && !slid(0.0, 30.0));
        assert_eq!(slide_spans(a(7, 60)).len(), 4);
        assert_eq!(slide_spans(a(7, 60))[3], a(52, 30));
    }
}
