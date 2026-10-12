//! Experiment E1 analysis (doc 07 §7): which cells the input-test screen lit and when, against what was injected.
//!
//! All times are ms since the run's epoch. Injected cells are mapped to screen cells with the `cells.rs`
//! hypothesis (Waccon cell -> lane/depth); a tap's mapping is correct when exactly those screen cells light up.

use crate::cells::{CELLS, cell_lane_depth};
use crate::inject::{Kind, Record, cells_from_hex};
use crate::stats::{Stats, summarize};
use crate::testscreen::{screen_cell, screen_lane_ring};
use serde::Serialize;

/// Frames this much before a press write still count for it (clock alignment slack; shows up as negative latency).
pub const EARLY_MS: f64 = 30.0;
/// Frames up to this long after the release write still count for a press (the screen lags ~0.1 s, more under
/// load). Windows of consecutive presses may overlap; cells are attributed by when they turn on.
pub const LAG_MS: f64 = 400.0;

/// One captured frame: its time and lit screen cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub t_ms: f64,
    pub lit: Vec<usize>,
}

/// Cells turned on by one edge write, until all of them are released.
#[derive(Clone, Debug, PartialEq)]
pub struct Press {
    pub t_script_ms: f64,
    pub on_ms: f64,
    pub off_ms: Option<f64>,
    /// DLL acknowledgement latency of the press write.
    pub ack_ms: Option<f64>,
    /// Expected screen cells, sorted.
    pub expected: Vec<usize>,
}

/// Presses from an `inject.jsonl` log whose `t_*_ms` are relative to `clock_start_ms`.
pub fn presses(records: &[Record], clock_start_ms: f64) -> Vec<Press> {
    let mut out: Vec<Press> = Vec::new();
    let mut open: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut prev = [0u8; CELLS];
    for r in records.iter().filter(|r| r.kind != Kind::Renew) {
        let Some(cur) = cells_from_hex(&r.cells) else {
            log::warn!("bad cells in record seq {}", r.seq);
            continue;
        };
        let t = clock_start_ms + r.t_write_ms;
        open.retain(|(i, cells)| {
            let released = cells.iter().all(|&c| cur[c] == 0);
            if released {
                out[*i].off_ms = Some(t);
            }
            !released
        });
        let new: Vec<usize> = (0..CELLS)
            .filter(|&c| cur[c] != 0 && prev[c] == 0)
            .collect();
        if !new.is_empty() {
            let mut expected: Vec<usize> = new
                .iter()
                .map(|&c| {
                    let (lane, depth) = cell_lane_depth(c);
                    screen_cell(lane, depth)
                })
                .collect();
            expected.sort_unstable();
            open.push((out.len(), new));
            out.push(Press {
                t_script_ms: r.t_script_ms,
                on_ms: t,
                off_ms: None,
                ack_ms: r.ack_latency_ms(),
                expected,
            });
        }
        prev = cur;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TapResult {
    pub t_script_ms: f64,
    /// `(lane, ring)` pairs.
    pub expected: Vec<(u8, usize)>,
    /// Every cell that turned on while the tap was visible.
    pub observed: Vec<(u8, usize)>,
    pub detected: bool,
    pub mapping_ok: bool,
    /// First frame showing any expected cell - press write.
    pub on_latency_ms: Option<f64>,
    /// First frame without them - release write.
    pub off_latency_ms: Option<f64>,
    /// How long the cells stayed visible (first unlit frame - first lit frame).
    pub visible_ms: Option<f64>,
    pub held_ms: Option<f64>,
    pub ack_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Analysis {
    pub taps: Vec<TapResult>,
    /// Frames showing a lit cell that no press explains.
    pub stray_frames: usize,
    pub detected: usize,
    pub mapping_ok: usize,
    pub on_latency: Option<Stats>,
    pub off_latency: Option<Stats>,
    pub ack: Option<Stats>,
}

fn lane_rings(cells: &[usize]) -> Vec<(u8, usize)> {
    cells.iter().map(|&c| screen_lane_ring(c)).collect()
}

/// `frames` must be in time order.
pub fn analyze(presses: &[Press], frames: &[Seen]) -> Analysis {
    let window = |p: &Press| p.on_ms - EARLY_MS..p.off_ms.unwrap_or(p.on_ms) + LAG_MS;
    let mut taps = Vec::new();
    for p in presses {
        let w = window(p);
        let in_window: Vec<usize> = (0..frames.len())
            .filter(|&k| w.contains(&frames[k].t_ms))
            .collect();
        let shows = |k: usize| frames[k].lit.iter().any(|c| p.expected.contains(c));
        let first = in_window.iter().copied().find(|&k| shows(k));
        let gone = first.and_then(|f| in_window.iter().copied().find(|&k| k > f && !shows(k)));
        // Cells that turn on during the visible span (lingering cells of an earlier tap were already on).
        let mut observed: Vec<usize> = Vec::new();
        if let Some(f) = first {
            for k in in_window
                .iter()
                .copied()
                .filter(|&k| k >= f && gone.is_none_or(|g| k < g))
            {
                let before: &[usize] = k.checked_sub(1).map_or(&[], |b| &frames[b].lit);
                observed.extend(frames[k].lit.iter().filter(|c| !before.contains(c)));
            }
        }
        observed.sort_unstable();
        observed.dedup();
        let t = |k: Option<usize>| k.map(|k| frames[k].t_ms);
        taps.push(TapResult {
            t_script_ms: p.t_script_ms,
            expected: lane_rings(&p.expected),
            observed: lane_rings(&observed),
            detected: first.is_some(),
            mapping_ok: first.is_some() && observed == p.expected,
            on_latency_ms: t(first).map(|f| f - p.on_ms),
            off_latency_ms: t(gone).zip(p.off_ms).map(|(g, off)| g - off),
            visible_ms: t(gone).zip(t(first)).map(|(g, f)| g - f),
            held_ms: p.off_ms.map(|off| off - p.on_ms),
            ack_ms: p.ack_ms,
        });
    }
    let explained = |f: &Seen| {
        f.lit.iter().all(|c| {
            presses
                .iter()
                .any(|p| window(p).contains(&f.t_ms) && p.expected.contains(c))
        })
    };
    let stray_frames = frames
        .iter()
        .filter(|f| !f.lit.is_empty() && !explained(f))
        .count();
    let collect = |f: fn(&TapResult) -> Option<f64>| {
        summarize(&taps.iter().filter_map(f).collect::<Vec<_>>())
    };
    Analysis {
        detected: taps.iter().filter(|t| t.detected).count(),
        mapping_ok: taps.iter().filter(|t| t.mapping_ok).count(),
        on_latency: collect(|t| t.on_latency_ms),
        off_latency: collect(|t| t.off_latency_ms),
        ack: collect(|t| t.ack_ms),
        stray_frames,
        taps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::cell_index;
    use crate::inject::cells_hex;

    fn rec(kind: Kind, t: f64, on: &[usize]) -> Record {
        let mut cells = [0u8; CELLS];
        on.iter().for_each(|&c| cells[c] = 1);
        Record {
            kind,
            seq: 0,
            t_script_ms: t,
            t_sched_ms: t,
            t_write_ms: t,
            t_ack_ms: Some(t + 1.0),
            opbtn: 0,
            cells: cells_hex(&cells),
        }
    }

    fn lane_cells(lane: u8) -> Vec<usize> {
        (0..4).map(|d| cell_index(lane, d)).collect()
    }

    #[test]
    fn presses_from_log() {
        let lane0 = lane_cells(0);
        let log = [
            rec(Kind::Edge, 100.0, &lane0),
            rec(Kind::Renew, 150.0, &lane0),
            rec(Kind::Edge, 300.0, &[]),
            rec(Kind::Edge, 500.0, &[cell_index(20, 0)]),
            rec(Kind::Clear, 550.0, &[]),
        ];
        let p = presses(&log, 1000.0);
        assert_eq!(p.len(), 2);
        assert_eq!(
            (p[0].on_ms, p[0].off_ms, p[0].ack_ms),
            (1100.0, Some(1300.0), Some(1.0))
        );
        assert_eq!(p[0].expected, [0, 1, 2, 3]);
        assert_eq!(p[1].expected, [screen_cell(20, 0)]);
        assert_eq!(p[1].off_ms, Some(1550.0));
    }

    fn seen(t_ms: f64, lit: &[usize]) -> Seen {
        Seen {
            t_ms,
            lit: lit.to_vec(),
        }
    }

    #[test]
    fn detection_latency_and_mapping() {
        let p = presses(
            &[
                rec(Kind::Edge, 100.0, &lane_cells(0)),
                rec(Kind::Edge, 300.0, &[]),
                rec(Kind::Edge, 1000.0, &[cell_index(20, 0)]),
                rec(Kind::Edge, 1017.0, &[]),
                rec(Kind::Edge, 2000.0, &[cell_index(30, 3)]),
                rec(Kind::Edge, 2005.0, &[]),
            ],
            0.0,
        );
        let frames = [
            seen(90.0, &[]),
            seen(140.0, &[0, 1, 2, 3]),
            seen(300.0, &[0, 1, 2, 3]),
            seen(340.0, &[]),
            // Wrong ring shown for lane 20; a stray cell elsewhere later.
            seen(1050.0, &[screen_cell(20, 0), screen_cell(20, 3)]),
            seen(1060.0, &[screen_cell(20, 0)]),
            seen(1080.0, &[]),
            seen(1500.0, &[screen_cell(7, 1)]),
            seen(2100.0, &[]),
        ];
        let a = analyze(&p, &frames);
        let t0 = &a.taps[0];
        assert!(t0.detected && t0.mapping_ok);
        assert_eq!(
            (
                t0.on_latency_ms,
                t0.off_latency_ms,
                t0.visible_ms,
                t0.held_ms
            ),
            (Some(40.0), Some(40.0), Some(200.0), Some(200.0))
        );
        let t1 = &a.taps[1];
        assert!(t1.detected && !t1.mapping_ok);
        assert_eq!(t1.observed, [(20, 0), (20, 3)]);
        assert_eq!(t1.on_latency_ms, Some(50.0));
        // The 5 ms tap never shows.
        assert!(!a.taps[2].detected && a.taps[2].on_latency_ms.is_none());
        assert_eq!((a.detected, a.mapping_ok), (2, 1));
        // Frames at 1050 (lane 20 ring 3 not expected) and 1500 (lane 7).
        assert_eq!(a.stray_frames, 2);
        assert_eq!(a.on_latency.map(|s| s.n), Some(2));
        assert_eq!(a.ack.map(|s| (s.n, s.mean)), Some((3, 1.0)));
    }

    #[test]
    fn overlapping_windows_attribute_cells_by_onset() {
        // Two taps 150 ms apart; the screen lags 120 ms, so tap A is still lit when tap B's window opens.
        let p = presses(
            &[
                rec(Kind::Edge, 0.0, &[cell_index(3, 0)]),
                rec(Kind::Edge, 100.0, &[]),
                rec(Kind::Edge, 150.0, &[cell_index(4, 0)]),
                rec(Kind::Edge, 250.0, &[]),
            ],
            0.0,
        );
        let (a, b) = (screen_cell(3, 0), screen_cell(4, 0));
        let frames = [
            seen(120.0, &[a]),
            seen(200.0, &[a]),
            seen(220.0, &[]),
            seen(270.0, &[b]),
            seen(370.0, &[]),
        ];
        let r = analyze(&p, &frames);
        assert_eq!((r.detected, r.mapping_ok, r.stray_frames), (2, 2, 0));
        assert_eq!(r.taps[1].on_latency_ms, Some(120.0));
        assert_eq!(r.taps[0].off_latency_ms, Some(120.0));
    }
}
