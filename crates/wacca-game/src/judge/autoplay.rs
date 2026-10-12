//! Perfect input for a chart: every note hit exactly on time.

use crate::input::{DEPTHS, InputFrame, SensorState};
use crate::rules::FRAME_SECS;
use wacca_chart::{Arc, Chart, LANES, NoteBody, SlideDir, SnapDir};

/// Length of each scripted contact phase; short so neighbouring notes get fresh presses.
const STEP: f64 = FRAME_SECS / 4.0;

/// Sensors held during `[start, end)`.
#[derive(Clone, Copy, Debug)]
struct Contact {
    start: f64,
    end: f64,
    sensors: SensorState,
}

fn mask(arc: Arc, depths: std::ops::Range<usize>) -> SensorState {
    let mut s = SensorState::default();
    for o in 0..arc.width {
        let lane = ((arc.lane as u16 + o as u16) % LANES as u16) as u8;
        for d in depths.clone() {
            s.set(lane, d, true);
        }
    }
    s
}

fn lane(arc: Arc, offset: u8) -> Arc {
    Arc {
        lane: ((arc.lane as u16 + offset as u16) % LANES as u16) as u8,
        width: 1,
    }
}

fn contacts(chart: &Chart) -> Vec<Contact> {
    let c = |start, end, sensors| Contact {
        start,
        end,
        sensors,
    };
    let mut out = Vec::new();
    for note in &chart.notes {
        let t = note.time;
        let all = 0..DEPTHS;
        match &note.body {
            NoteBody::Touch | NoteBody::Chain => out.push(c(t, t + STEP, mask(note.arc, all))),
            NoteBody::Snap(dir) => {
                let (from, to) = match dir {
                    SnapDir::In => (DEPTHS - 1, 0),
                    SnapDir::Out => (0, DEPTHS - 1),
                };
                out.push(c(t - STEP, t, mask(note.arc, from..from + 1)));
                out.push(c(t, t + STEP, mask(note.arc, to..to + 1)));
            }
            NoteBody::Slide(dir) => {
                // Move from the trailing edge up to 3 lanes forward (needs width >= 3).
                let last = note.arc.width.saturating_sub(1);
                let dist = last.min(3);
                let (from, to) = match dir {
                    SlideDir::Ccw => (0, dist),
                    SlideDir::Cw => (last, last - dist),
                };
                out.push(c(t - STEP, t, mask(lane(note.arc, from), all.clone())));
                out.push(c(t, t + STEP, mask(lane(note.arc, to), all)));
            }
            NoteBody::Hold { nodes } => {
                let end = nodes.last().map_or(t, |n| n.time);
                // Release just before the end so a note on the end gets a fresh press.
                let release = (end - STEP).max(t + STEP);
                for (k, node) in nodes.iter().enumerate() {
                    let next = nodes.get(k + 1).map_or(release, |n| n.time.min(release));
                    if next > node.time {
                        out.push(c(node.time, next, mask(node.arc, all.clone())));
                    }
                }
            }
        }
    }
    out
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
