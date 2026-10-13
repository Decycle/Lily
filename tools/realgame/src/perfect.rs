//! Perfect input for a real-game chart (experiment E2): our autoplay ([`wacca_game::judge::autoplay_frames`]) as
//! per-cell contacts, each held at least the real game's minimum (E1: one 16.7 ms frame; 34 ms for margin), then
//! written as a tap script in chart time.
//!
//! Autoplay uses ~4 ms contact phases. A contact that is too short is lengthened forwards (a tap starts on the note
//! time) unless another contact starts exactly where it ends (the first phase of a snap or slide, or a hold
//! finger moving on): then it is lengthened backwards so the motion still happens on the note time. Either way it
//! keeps `min_gap` to the neighbouring contacts of the same cell so every press stays a fresh press.

use crate::timeline::Tap;
use wacca_chart::LANES;
use wacca_game::input::{DEPTHS, InputFrame};

/// One sensor held during `[start, end)` (seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub lane: u8,
    pub depth: usize,
    pub start: f64,
    pub end: f64,
}

/// Per-cell on-intervals of an edge list, sorted by start time.
pub fn contacts(frames: &[InputFrame]) -> Vec<Contact> {
    let mut out = Vec::new();
    let mut on: Vec<Option<f64>> = vec![None; LANES as usize * DEPTHS];
    for f in frames {
        for lane in 0..LANES {
            for depth in 0..DEPTHS {
                let slot = &mut on[lane as usize * DEPTHS + depth];
                match (slot.is_some(), f.sensors.get(lane, depth)) {
                    (false, true) => *slot = Some(f.time),
                    (true, false) => {
                        if let Some(start) = slot.take() {
                            out.push(Contact {
                                lane,
                                depth,
                                start,
                                end: f.time,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(end) = frames.last().map(|f| f.time) {
        for (i, s) in on.iter().enumerate() {
            if let Some(start) = *s {
                let (lane, depth) = ((i / DEPTHS) as u8, i % DEPTHS);
                out.push(Contact {
                    lane,
                    depth,
                    start,
                    end,
                });
            }
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// Lengthens contacts shorter than `min_on` (see the module docs), keeping `min_gap` to the same cell's previous
/// and next contacts.
pub fn stretch(contacts: &[Contact], min_on: f64, min_gap: f64) -> Vec<Contact> {
    const SAME: f64 = 1e-6;
    let same_cell = |a: &Contact, b: &Contact| a.lane == b.lane && a.depth == b.depth;
    contacts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if c.end - c.start >= min_on {
                return *c;
            }
            let prev_end = contacts[..i]
                .iter()
                .rev()
                .find(|o| same_cell(o, c))
                .map_or(f64::NEG_INFINITY, |o| o.end);
            let next_start = contacts[i + 1..]
                .iter()
                .find(|o| same_cell(o, c))
                .map_or(f64::INFINITY, |o| o.start);
            let handover = contacts
                .iter()
                .any(|o| !same_cell(o, c) && (o.start - c.end).abs() < SAME);
            let mut out = *c;
            if handover {
                out.start = (c.end - min_on).max(prev_end + min_gap).min(c.start);
            } else {
                out.end = (c.start + min_on).min(next_start - min_gap).max(c.end);
            }
            out
        })
        .collect()
}

fn us(t: f64) -> i64 {
    (t * 1e6).round() as i64
}

/// Contacts as taps (ms): cells with the same interval in one lane share a tap (depth mask), and neighbouring
/// lanes with the same interval and mask are merged into one wider tap.
pub fn to_taps(contacts: &[Contact]) -> Vec<Tap> {
    let mut keyed: Vec<(i64, i64, u8, u8)> = Vec::new();
    for c in contacts {
        let key = (us(c.start), us(c.end), c.lane);
        match keyed.iter_mut().find(|k| (k.0, k.1, k.2) == key) {
            Some(k) => k.3 |= 1 << c.depth,
            None => keyed.push((key.0, key.1, key.2, 1 << c.depth)),
        }
    }
    keyed.sort_by_key(|&(s, e, lane, d)| (s, e, d, lane));
    let mut taps: Vec<Tap> = Vec::new();
    for (s, e, lane, depths) in keyed {
        let tap = Tap {
            at_ms: s as f64 / 1000.0,
            dur_ms: (e - s) as f64 / 1000.0,
            lane,
            width: 1,
            depths,
        };
        match taps.last_mut() {
            Some(t)
                if (t.at_ms, t.dur_ms, t.depths) == (tap.at_ms, tap.dur_ms, depths)
                    && t.lane + t.width == lane =>
            {
                t.width += 1;
            }
            _ => taps.push(tap),
        }
    }
    taps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::expand;
    use wacca_game::input::SensorState;

    fn frame(time: f64, cells: &[(u8, usize)]) -> InputFrame {
        let mut sensors = SensorState::default();
        cells.iter().for_each(|&(l, d)| sensors.set(l, d, true));
        InputFrame { time, sensors }
    }

    fn c(lane: u8, depth: usize, start: f64, end: f64) -> Contact {
        Contact {
            lane,
            depth,
            start,
            end,
        }
    }

    #[test]
    fn contacts_from_frames() {
        let f = [
            frame(1.0, &[(5, 0), (6, 0)]),
            frame(1.004, &[(6, 0)]),
            frame(1.008, &[]),
            frame(2.0, &[(7, 3)]),
        ];
        assert_eq!(
            contacts(&f),
            [c(5, 0, 1.0, 1.004), c(6, 0, 1.0, 1.008), c(7, 3, 2.0, 2.0)]
        );
    }

    #[test]
    fn stretches_taps_forward_and_lead_ins_backward() {
        let got = stretch(
            &[
                // Snap: outer ring just before 1.0 hands over to the inner ring at 1.0.
                c(10, 3, 0.996, 1.0),
                c(10, 0, 1.0, 1.004),
                // Two quick taps on one cell: the first may only grow up to the gap before the second.
                c(20, 0, 2.0, 2.004),
                c(20, 0, 2.04, 2.044),
                // Long contacts stay.
                c(30, 3, 3.0, 4.0),
            ],
            0.034,
            0.017,
        );
        let r = |v: f64| (v * 1e6).round() / 1e6;
        let spans: Vec<(f64, f64)> = got.iter().map(|c| (r(c.start), r(c.end))).collect();
        assert_eq!(
            spans,
            [
                (0.966, 1.0),
                (1.0, 1.034),
                (2.0, 2.023),
                (2.04, 2.074),
                (3.0, 4.0)
            ]
        );
    }

    #[test]
    fn taps_merge_depths_and_lanes_and_round_trip() {
        let cs = [
            c(58, 0, 1.0, 1.034),
            c(59, 0, 1.0, 1.034),
            c(59, 1, 1.0, 1.034),
            c(0, 0, 1.0, 1.034),
            c(3, 2, 1.5, 1.6),
            c(4, 2, 1.5, 1.6),
        ];
        let taps = to_taps(&cs);
        let got: Vec<(f64, f64, u8, u8, u8)> = taps
            .iter()
            .map(|t| (t.at_ms, t.dur_ms, t.lane, t.width, t.depths))
            .collect();
        assert_eq!(
            got,
            [
                (1000.0, 34.0, 0, 1, 1),
                (1000.0, 34.0, 58, 1, 1),
                (1000.0, 34.0, 59, 1, 3),
                (1500.0, 100.0, 3, 2, 4)
            ]
        );
        let mut back = contacts(&expand(&taps));
        back.sort_by(|a, b| {
            (a.start, a.lane, a.depth)
                .partial_cmp(&(b.start, b.lane, b.depth))
                .unwrap()
        });
        assert_eq!(back.len(), cs.len());
        assert!(back.iter().all(|b| cs.iter().any(|c| c.lane == b.lane
            && c.depth == b.depth
            && (c.start - b.start).abs() < 1e-9
            && (c.end - b.end).abs() < 1e-9)));
    }
}
