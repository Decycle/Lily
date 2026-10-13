//! Perfect input for a chart: every note hit exactly on time.

use super::sense;
use crate::input::{DEPTHS, InputFrame, SensorState};
use crate::rules::FRAME_SECS;
use wacca_chart::{Arc, Chart, HoldNode, NoteBody, SlideDir, SnapDir};

/// Length of each scripted contact phase; short so neighbouring notes get fresh presses.
const STEP: f64 = FRAME_SECS / 4.0;
/// Rings used for hold starts and resting hold fingers (taps and chains use ring 0).
const HOLD_START: usize = 2;
const HOLD_BODY: usize = DEPTHS - 1;
/// Early window of tap-like notes (Touch/Hold Good = 6 frames, plus margin). Delaying a hold
/// finger by up to this stays within `HOLD_DROP_LENIENCY`.
const TAP_EARLY: f64 = 6.5 * FRAME_SECS;

/// Sensors held during `[start, end)`.
#[derive(Clone, Copy, Debug)]
struct Contact {
    start: f64,
    end: f64,
    sensors: SensorState,
}

fn mask(arc: Arc, depths: std::ops::Range<usize>) -> SensorState {
    let mut s = SensorState::default();
    for d in depths {
        s.rings[d] = sense::arc_mask(arc);
    }
    s
}

fn lane(arc: Arc, offset: u8) -> Arc {
    Arc {
        lane: sense::lane_at(arc, offset),
        width: 1,
    }
}

fn contacts(chart: &Chart) -> Vec<Contact> {
    let c = |start, end, sensors| Contact {
        start,
        end,
        sensors,
    };
    let taps: Vec<(f64, Arc)> = chart
        .notes
        .iter()
        .filter(|n| matches!(n.body, NoteBody::Touch | NoteBody::Hold { .. }))
        .map(|n| (n.time, n.arc))
        .collect();
    let mut out = Vec::new();
    for note in &chart.notes {
        let t = note.time;
        let all = 0..DEPTHS;
        match &note.body {
            NoteBody::Touch | NoteBody::Chain => out.push(c(t, t + STEP, mask(note.arc, 0..1))),
            NoteBody::Snap(dir) => {
                let (from, to) = match dir {
                    SnapDir::In => (DEPTHS - 1, 0),
                    SnapDir::Out => (0, DEPTHS - 1),
                };
                out.push(c(t - STEP, t, mask(note.arc, from..from + 1)));
                out.push(c(t, t + STEP, mask(note.arc, to..to + 1)));
            }
            NoteBody::Slide(dir) => {
                // Move from the trailing edge up to 8 lanes forward (needs width >= 3); the long
                // travel keeps the mean moving past the threshold despite resting hold fingers.
                let last = note.arc.width.saturating_sub(1);
                let dist = last.min(8);
                let (from, to) = match dir {
                    SlideDir::Ccw => (0, dist),
                    SlideDir::Cw => (last, last - dist),
                };
                out.push(c(t - STEP, t, mask(lane(note.arc, from), all.clone())));
                out.push(c(t, t + STEP, mask(lane(note.arc, to), all)));
            }
            NoteBody::Hold { nodes } => {
                // Tap the start on its own ring, then rest one sensor on the rim so taps,
                // chains and other hold starts in the same lanes still see fresh presses.
                out.push(c(t, t + STEP, mask(note.arc, HOLD_START..HOLD_START + 1)));
                let end = nodes.last().map_or(t, |n| n.time);
                let release = (end - STEP).max(t + STEP);
                let moves = hold_finger(note.arc, nodes, &taps);
                for (k, &(start, finger)) in moves.iter().enumerate() {
                    let next = moves.get(k + 1).map_or(release, |m| m.0.min(release));
                    if next > start {
                        out.push(c(start, next, mask(finger, HOLD_BODY..HOLD_BODY + 1)));
                    }
                }
            }
        }
    }
    out
}

/// Where the resting hold finger is from each time on. It follows the judged path (straight
/// between control points, sampled every `STEP`) and only moves when the path leaves it, since
/// entering new sensors counts as a fresh press; a move into the arc of a tap note whose window
/// is open waits for that note's time (well within the hold's release allowance).
fn hold_finger(start: Arc, nodes: &[HoldNode], taps: &[(f64, Arc)]) -> Vec<(f64, Arc)> {
    let mut finger = lane(start, start.width / 2);
    let (Some(first), Some(last)) = (nodes.first(), nodes.last()) else {
        return Vec::new();
    };
    let mut moves = vec![(first.time, finger)];
    let steps = ((last.time - first.time) / STEP).ceil().max(0.0) as usize;
    for k in 1..=steps {
        let time = (first.time + k as f64 * STEP).min(last.time);
        let Some(arc) = super::hold_path_at(nodes, time) else {
            break;
        };
        if arc.contains(finger.lane) {
            continue;
        }
        finger = lane(arc, arc.width / 2);
        // Arriving together with the earliest such note hits it on time and uses up its lanes.
        let at = taps
            .iter()
            .filter(|(t, a)| a.contains(finger.lane) && time >= t - TAP_EARLY && time < *t)
            .map(|&(t, _)| t)
            .reduce(f64::min)
            .unwrap_or(time);
        let prev = moves.last().map_or(at, |m| m.0);
        moves.push((at.max(prev), finger));
    }
    moves
}

/// Sensor snapshots that hit every note of `chart` on time, ending with an empty frame one
/// second after the last contact so every note is resolved.
pub fn autoplay_frames(chart: &Chart) -> Vec<InputFrame> {
    let mut contacts = contacts(chart);
    contacts.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut times: Vec<f64> = contacts.iter().flat_map(|c| [c.start, c.end]).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let last = times.last().copied().unwrap_or(0.0);
    times.push(last + 1.0);
    times
        .into_iter()
        .map(|time| {
            let mut sensors = SensorState::default();
            for c in contacts.iter().take_while(|c| c.start <= time) {
                if time < c.end {
                    for d in 0..DEPTHS {
                        sensors.rings[d] |= c.sensors.rings[d];
                    }
                }
            }
            InputFrame { time, sensors }
        })
        .collect()
}
