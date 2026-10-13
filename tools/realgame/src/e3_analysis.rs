//! E3 analysis (doc 10): debug-panel changes ([`crate::panel`]) → one real judgement per test note → window edges
//! and probe tables, pooled over runs.
//!
//! Time base: panel changes are stamped in audio chart time. A note judged on a press is shown a fixed `lag` after
//! the scripted press; `lag` is the most common distance from a change to the nearest note ([`estimate_lag`]).
//! Window edges are given relative to each run's touch Marvelous centre (its timing check, [`touch_centre`]): the
//! script's absolute offset to the game's judge clock (input lead, A/V skew, frame phase) is not known to better
//! than a frame, but widths and edges relative to the touch window are.

use crate::e3::{Class, Probe, Test, fit_interval, rank};
use crate::panel::Event;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;
use wacca_game::rules::{FRAME_SECS, Judgement};

/// Slack before a test note's start and after its end in which its panel change is looked for (s).
pub const BEFORE: f64 = 0.25;
pub const AFTER: f64 = 0.30;

/// Most common offset from a panel change to the nearest of `times`, refined by the median within ±25 ms.
pub fn estimate_lag(events: &[f64], times: &[f64]) -> Option<f64> {
    let residual = |t: f64| {
        times
            .iter()
            .map(|&n| t - n)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
    };
    let rs: Vec<f64> = events
        .iter()
        .filter_map(|&t| residual(t))
        .filter(|r| r.abs() <= 0.4)
        .collect();
    let bin = |r: f64| ((r + 0.4) / 0.005).floor() as usize;
    let mut hist = vec![0usize; 161];
    rs.iter().for_each(|&r| hist[bin(r).min(160)] += 1);
    let (best, _) = hist.iter().enumerate().max_by_key(|&(i, n)| (*n, usize::MAX - i))?;
    let centre = best as f64 * 0.005 - 0.4 + 0.0025;
    let mut near: Vec<f64> = rs.into_iter().filter(|r| (r - centre).abs() <= 0.025).collect();
    near.sort_by(f64::total_cmp);
    near.get(near.len() / 2).copied()
}

/// What the real game showed for one test note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(flatten)]
    pub test: Test,
    /// The single judgement shown for the note; `None` if there was no change or more than one.
    pub real: Option<Judgement>,
    pub fast: bool,
    pub late: bool,
    /// When the judgement was shown, relative to the note time, `lag` removed (s).
    pub shown: Option<f64>,
    /// All judgement counter changes in the note's window.
    pub seen: Vec<Judgement>,
}

pub fn resolve(test: &Test, events: &[Event], lag: f64) -> Outcome {
    let (from, to) = (test.time + lag - BEFORE, test.end + lag + AFTER);
    let hits: Vec<&Event> = events.iter().filter(|e| e.t >= from && e.t <= to).collect();
    let seen: Vec<Judgement> = hits.iter().flat_map(|e| e.judgements()).collect();
    let single = (hits.len() == 1 && seen.len() == 1).then(|| hits[0]);
    Outcome {
        test: test.clone(),
        real: single.map(|_| seen[0]),
        fast: single.is_some_and(Event::fast),
        late: single.is_some_and(Event::late),
        shown: single.map(|e| e.t - lag - test.time),
        seen,
    }
}

fn shift_ms(t: &Test) -> Option<f64> {
    match t.probe {
        Probe::Shift { ms } => Some(ms),
        _ => None,
    }
}

/// `(shift ms, real rank)` of a run's resolved shift tests of `class`.
fn sweep(outcomes: &[Outcome], class: Class) -> Vec<(f64, usize)> {
    outcomes
        .iter()
        .filter(|o| o.test.class == class)
        .filter_map(|o| Some((shift_ms(&o.test)?, rank(o.real?))))
        .collect()
}

/// Centre (ms of script shift) of the touch Marvelous interval; needs at least `min` resolved touch shifts.
pub fn touch_centre(outcomes: &[Outcome], min: usize) -> Option<f64> {
    let pts = sweep(outcomes, Class::Touch);
    if pts.len() < min {
        return None;
    }
    let ok: Vec<(f64, bool)> = pts.iter().map(|&(x, r)| (x, r == 0)).collect();
    fit_interval(&ok).map(|(lo, hi, _)| (lo + hi) / 2.0)
}

/// One run's outcomes and its touch centre (ms).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    pub run: String,
    pub lag: f64,
    pub centre_ms: Option<f64>,
    pub outcomes: Vec<Outcome>,
}

const TIERS: [&str; 3] = ["Marvelous", "≥Great", "≥Good"];

fn frames(ms: f64) -> f64 {
    ms / (FRAME_SECS * 1000.0)
}

fn name(j: Option<Judgement>) -> &'static str {
    match j {
        Some(Judgement::Marvelous) => "M",
        Some(Judgement::Great) => "Gr",
        Some(Judgement::Good) => "Gd",
        Some(Judgement::Miss) => "Miss",
        None => "?",
    }
}

/// Fitted edges per class and tier, in frames relative to the run's touch centre: `(lo, hi, misfits, n)`.
pub fn edges(runs: &[RunResult]) -> BTreeMap<(Class, usize), (f64, f64, usize, usize)> {
    let mut out = BTreeMap::new();
    for class in [Class::Touch, Class::Hold, Class::Chain, Class::Slide, Class::Snap] {
        let pts: Vec<(f64, usize)> = runs
            .iter()
            .filter_map(|r| Some((r.centre_ms?, r)))
            .flat_map(|(c, r)| sweep(&r.outcomes, class).into_iter().map(move |(x, k)| (frames(x - c), k)))
            .collect();
        for tier in 0..3 {
            let ok: Vec<(f64, bool)> = pts.iter().map(|&(x, k)| (x, k <= tier)).collect();
            if let Some((lo, hi, err)) = fit_interval(&ok) {
                out.insert((class, tier), (lo, hi, err, pts.len()));
            }
        }
    }
    out
}

/// Markdown report: edges, sweep histograms, miss display times and probe tables.
pub fn report(runs: &[RunResult]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "## Runs\n\n| Run | Lag ms | Touch centre ms | Tests | Resolved |\n|---|---|---|---|---|");
    for r in runs {
        let resolved = r.outcomes.iter().filter(|o| o.real.is_some()).count();
        let _ = writeln!(
            s,
            "| {} | {:.1} | {} | {} | {resolved} |",
            r.run,
            r.lag * 1000.0,
            r.centre_ms.map_or("–".into(), |c| format!("{c:.1}")),
            r.outcomes.len()
        );
    }
    let _ = writeln!(
        s,
        "\n## Window edges (frames from the touch centre; 50 % points)\n\n| Class | Tier | Early | Late | Width | Misfits / n |\n|---|---|---|---|---|---|"
    );
    for ((class, tier), (lo, hi, err, n)) in edges(runs) {
        let _ = writeln!(
            s,
            "| {class:?} | {} | {lo:+.2} | {hi:+.2} | {:.2} | {err} / {n} |",
            TIERS[tier],
            hi - lo
        );
    }
    let _ = writeln!(s, "\n## Sweeps (frames from the touch centre, rounded to ½)\n");
    for class in [Class::Touch, Class::Hold, Class::Chain, Class::Slide, Class::Snap] {
        let mut rows: BTreeMap<i64, [usize; 5]> = BTreeMap::new();
        for r in runs {
            let Some(c) = r.centre_ms else { continue };
            for o in r.outcomes.iter().filter(|o| o.test.class == class) {
                let Some(ms) = shift_ms(&o.test) else { continue };
                let key = (frames(ms - c) * 2.0).round() as i64;
                rows.entry(key).or_default()[o.real.map_or(4, rank)] += 1;
            }
        }
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(s, "{class:?}: frame → M/Gr/Gd/Miss/?");
        let line: Vec<String> = rows
            .iter()
            .map(|(k, c)| format!("{:+.1}: {}/{}/{}/{}/{}", *k as f64 / 2.0, c[0], c[1], c[2], c[3], c[4]))
            .collect();
        let _ = writeln!(s, "  {}\n", line.join(" · "));
    }
    let _ = writeln!(s, "## When misses are shown (frames after the note, relative to a centred hit)\n");
    for class in [Class::Touch, Class::Hold, Class::Chain, Class::Slide, Class::Snap] {
        let mut v: Vec<f64> = Vec::new();
        for r in runs {
            let Some(c) = r.centre_ms else { continue };
            // A centred hit is shown at `hit0` after the note; misses are compared with it.
            let mut hit: Vec<f64> = r
                .outcomes
                .iter()
                .filter(|o| o.test.class == Class::Touch && o.real == Some(Judgement::Marvelous))
                .filter_map(|o| Some(o.shown? - shift_ms(&o.test)? / 1000.0))
                .collect();
            hit.sort_by(f64::total_cmp);
            let Some(&k) = hit.get(hit.len() / 2) else { continue };
            let hit0 = k + c / 1000.0;
            v.extend(
                r.outcomes
                    .iter()
                    .filter(|o| o.test.class == class && o.real == Some(Judgement::Miss))
                    .filter(|o| matches!(o.test.probe, Probe::Shift { .. }))
                    .filter_map(|o| Some(frames((o.shown? - hit0) * 1000.0))),
            );
        }
        v.sort_by(f64::total_cmp);
        if let [first, .., last] = v[..] {
            let _ = writeln!(
                s,
                "- {class:?}: median {:+.2}, range {first:+.2}..{last:+.2} (n {})",
                v[v.len() / 2],
                v.len()
            );
        }
    }
    let _ = writeln!(s, "\n## Other probes (real / ours)\n");
    let mut table: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for o in runs.iter().flat_map(|r| &r.outcomes) {
        let key = match o.test.probe {
            Probe::Shift { .. } => continue,
            Probe::HoldGap { at_ms, ms } => format!("hold gap {ms:.0} ms at +{at_ms:.0} ms"),
            Probe::HoldRelease { ms } => format!("hold released {ms:.0} ms before end"),
            Probe::SlideJump { lanes } => format!("slide jump {lanes:.1} lanes"),
            Probe::SnapMove { from, to } => format!("snap {:?} ring {from}→{to}", o.test.class),
        };
        *table
            .entry(key)
            .or_default()
            .entry(format!("{} / {}", name(o.real), name(o.test.ours)))
            .or_default() += 1;
    }
    for (k, v) in table {
        let cells: Vec<String> = v.iter().map(|(j, n)| format!("{j} ×{n}")).collect();
        let _ = writeln!(s, "- {k}: {}", cells.join(", "));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::ROWS;

    fn ev(t: f64, row: usize) -> Event {
        let mut rows = [false; ROWS];
        rows[row] = true;
        Event { t, rows }
    }

    fn test(time: f64, probe: Probe) -> Test {
        Test {
            note: 0,
            time,
            end: time,
            class: Class::Touch,
            probe,
            ours: None,
        }
    }

    #[test]
    fn lag_is_the_common_offset() {
        let times = [1.0, 2.0, 3.0, 4.0, 5.0];
        let events = [1.031, 2.03, 3.029, 4.2, 5.03];
        let lag = estimate_lag(&events, &times).unwrap();
        assert!((lag - 0.03).abs() < 1e-9, "{lag}");
        assert_eq!(estimate_lag(&[], &times), None);
    }

    #[test]
    fn resolves_single_changes_only() {
        let events = [ev(1.05, 1), ev(2.06, 0), ev(2.2, 3), ev(5.0, 0)];
        let o = resolve(&test(1.0, Probe::Shift { ms: 0.0 }), &events, 0.03);
        assert_eq!(o.real, Some(Judgement::Great));
        assert!((o.shown.unwrap() - 0.02).abs() < 1e-9);
        let two = resolve(&test(2.0, Probe::Shift { ms: 0.0 }), &events, 0.03);
        assert_eq!((two.real, two.seen.len()), (None, 2));
        assert_eq!(resolve(&test(3.5, Probe::Shift { ms: 0.0 }), &events, 0.03).real, None);
    }

    fn outcome(ms: f64, real: Judgement) -> Outcome {
        Outcome {
            test: test(0.0, Probe::Shift { ms }),
            real: Some(real),
            fast: false,
            late: false,
            shown: Some(ms / 1000.0),
            seen: vec![real],
        }
    }

    #[test]
    fn centre_and_edges() {
        // Marvelous for -50..=30 ms (centre -10), Great out to ±80 around it, Miss beyond.
        let outcomes: Vec<Outcome> = (-12..=12)
            .map(|i| {
                let ms = f64::from(i) * 10.0;
                let j = match ms + 10.0 {
                    d if d.abs() <= 40.0 => Judgement::Marvelous,
                    d if d.abs() <= 80.0 => Judgement::Great,
                    _ => Judgement::Miss,
                };
                outcome(ms, j)
            })
            .collect();
        let c = touch_centre(&outcomes, 5).unwrap();
        assert!((c + 10.0).abs() < 1e-9, "{c}");
        let run = RunResult {
            run: "r".into(),
            lag: 0.0,
            centre_ms: Some(c),
            outcomes,
        };
        let e = edges(std::slice::from_ref(&run));
        let (lo, hi, err, n) = e[&(Class::Touch, 1)];
        assert!((frames(-85.0) - lo).abs() < 1e-9 && (frames(85.0) - hi).abs() < 1e-9);
        assert_eq!((err, n), (0, 25));
        assert!(report(&[run]).contains("| Touch | Marvelous |"));
    }
}
