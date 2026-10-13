//! Scripted input: a list of taps (JSON) expanded into the `InputFrame` edge list both the injector and our
//! judge engine consume, so the real game and `wacca-game` see the same sensor sequence.
//!
//! File format (`*.taps.json`): `{"taps":[{"at_ms":2000,"dur_ms":50,"lane":0,"width":1,"depths":15}, ...]}`.
//! `at_ms` is song time (chart time, ms) of the press, `dur_ms` how long the cells stay on, `lane`/`width` the
//! first lane and lane count (counter-clockwise, wraps 59 -> 0), `depths` a bit mask (bit 0 = innermost ring).

use serde::{Deserialize, Serialize};
use wacca_chart::LANES;
use wacca_game::input::{DEPTHS, InputFrame, SensorState};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tap {
    pub at_ms: f64,
    pub dur_ms: f64,
    pub lane: u8,
    pub width: u8,
    pub depths: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub taps: Vec<Tap>,
}

impl Tap {
    fn active_at(&self, t_ms: f64) -> bool {
        t_ms >= self.at_ms && t_ms < self.at_ms + self.dur_ms
    }

    fn apply(&self, s: &mut SensorState) {
        for i in 0..self.width.min(LANES) {
            let lane = (self.lane % LANES + i) % LANES;
            for depth in 0..DEPTHS {
                if self.depths >> depth & 1 == 1 {
                    s.set(lane, depth, true);
                }
            }
        }
    }
}

pub fn parse(json: &str) -> serde_json::Result<Script> {
    serde_json::from_str(json)
}

/// One frame per edge (press or release), in time order, starting with the all-released state at t = 0 or the
/// earliest edge. Consecutive identical states are merged.
pub fn expand(taps: &[Tap]) -> Vec<InputFrame> {
    let mut edges: Vec<f64> = taps
        .iter()
        .filter(|t| t.dur_ms > 0.0)
        .flat_map(|t| [t.at_ms, t.at_ms + t.dur_ms])
        .collect();
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    let mut out: Vec<InputFrame> = Vec::with_capacity(edges.len());
    for t in edges {
        let mut s = SensorState::default();
        taps.iter()
            .filter(|tap| tap.active_at(t))
            .for_each(|tap| tap.apply(&mut s));
        if out.last().is_none_or(|f| f.sensors != s) {
            out.push(InputFrame {
                time: t / 1000.0,
                sensors: s,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_expand_overlapping_taps() {
        let s = parse(
            r#"{"taps":[
                {"at_ms":1000,"dur_ms":50,"lane":59,"width":2,"depths":15},
                {"at_ms":1020,"dur_ms":50,"lane":30,"width":1,"depths":1}
            ]}"#,
        )
        .unwrap();
        let f = expand(&s.taps);
        let times: Vec<f64> = f.iter().map(|f| f.time).collect();
        assert_eq!(times, [1.0, 1.02, 1.05, 1.07]);
        assert!(
            f[0].sensors.get(59, 3) && f[0].sensors.get(0, 0) && !f[0].sensors.lane_touched(30)
        );
        assert!(f[1].sensors.get(30, 0) && !f[1].sensors.get(30, 1) && f[1].sensors.get(59, 0));
        assert!(!f[2].sensors.lane_touched(59) && f[2].sensors.get(30, 0));
        assert!(f[3].sensors.is_empty());
    }

    #[test]
    fn e1_script_parses() {
        let s = parse(include_str!("../scripts/e1-lanes.taps.json")).unwrap();
        assert_eq!(s.taps.len(), 21);
        assert_eq!(expand(&s.taps).len(), 42);
    }

    #[test]
    fn back_to_back_taps_on_same_cells_merge() {
        let taps = [
            Tap {
                at_ms: 0.0,
                dur_ms: 10.0,
                lane: 5,
                width: 1,
                depths: 1,
            },
            Tap {
                at_ms: 10.0,
                dur_ms: 10.0,
                lane: 5,
                width: 1,
                depths: 1,
            },
        ];
        let f = expand(&taps);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].time, f[1].time), (0.0, 0.02));
    }
}
