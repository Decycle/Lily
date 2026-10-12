//! Pure sensor-grid predicates used by the judge: presses, arc touches, snap and slide motion.

use crate::input::{DEPTHS, SensorState};
use wacca_chart::{Arc, LANES, SlideDir, SnapDir};

const ALL_LANES: u64 = (1 << LANES) - 1;

/// Slide fires when the mean pressed offset rises this many lanes above its minimum.
// UNVERIFIED: Saturn's value, "chosen based on testing with the original game".
pub const SLIDE_THRESHOLD: f64 = 1.9;
/// Larger jumps are rejected as unrelated input (more than half a circle).
// UNVERIFIED: Saturn.
const SLIDE_MAX: f64 = 30.0;

fn lane_at(arc: Arc, offset: u8) -> u8 {
    ((arc.lane as u16 + offset as u16) % LANES as u16) as u8
}

pub fn arcs_overlap(a: Arc, b: Arc) -> bool {
    a.contains(b.lane) || b.contains(a.lane)
}

/// Any sensor (any depth) inside `arc` is on.
pub fn arc_touched(s: &SensorState, arc: Arc) -> bool {
    (0..arc.width).any(|o| s.lane_touched(lane_at(arc, o)))
}

/// New presses: contiguous runs (wrapping 59→0) of lanes with a sensor that went off→on.
pub fn presses(prev: &SensorState, cur: &SensorState) -> Vec<Arc> {
    let mask = (0..DEPTHS).fold(0, |m, d| m | (cur.rings[d] & !prev.rings[d])) & ALL_LANES;
    if mask == 0 {
        return Vec::new();
    }
    if mask == ALL_LANES {
        return vec![Arc {
            lane: 0,
            width: LANES,
        }];
    }
    let set = |l: u8| mask >> l & 1 == 1;
    // Start scanning just after an unset lane so no run is split by the wrap.
    let gap = (0..LANES).find(|&l| !set(l)).unwrap_or(0);
    let mut runs = Vec::new();
    let mut run: Option<Arc> = None;
    for k in 1..=LANES {
        let lane = (gap + k) % LANES;
        if !set(lane) {
            runs.extend(run.take());
        } else if let Some(r) = run.as_mut() {
            r.width += 1;
        } else {
            run = Some(Arc { lane, width: 1 });
        }
    }
    runs.extend(run);
    runs
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
    fn presses_group_runs_and_wrap() {
        let prev = s(&[(10, 0)]);
        let cur = s(&[(10, 0), (11, 1), (12, 3), (59, 0), (0, 2), (30, 1)]);
        let mut p = presses(&prev, &cur);
        p.sort_by_key(|r| r.lane);
        assert_eq!(p, vec![a(11, 2), a(30, 1), a(59, 2)]);
        assert!(presses(&cur, &cur).is_empty());
        let mut full = SensorState::default();
        full.rings[2] = ALL_LANES;
        assert_eq!(presses(&prev, &full), vec![a(0, 60)]);
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
