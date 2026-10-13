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

/// Panel changes further than this (s) from every note are not assigned.
pub const MAX_DISTANCE: f64 = 0.4;

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
    let (best, _) = hist
        .iter()
        .enumerate()
        .max_by_key(|&(i, n)| (*n, usize::MAX - i))?;
    let centre = best as f64 * 0.005 - 0.4 + 0.0025;
    let mut near: Vec<f64> = rs
        .into_iter()
        .filter(|r| (r - centre).abs() <= 0.025)
        .collect();
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
    /// The game clock's drift (ms) against the audio at this note, relative to the run mean ([`clock_at`]);
    /// `None` next to a clock jump (timing unusable).
    #[serde(default)]
    pub clock_ms: Option<f64>,
}

/// Clock jumps larger than this (ms) between timing blocks make the blocks around them unusable.
pub const JUMP_MS: f64 = 30.0;

/// The visual-vs-audio offset (`analyze_e2` timing check: blocks `(start chart s, δ ms)`) at chart time `t`,
/// relative to the mean over all blocks; `None` in a block next to a jump of more than [`JUMP_MS`]. A press at
/// audio offset `x` from the note is judged at `x - δ` (the judge clock runs with the visuals).
pub fn clock_at(blocks: &[(f64, f64)], t: f64) -> Option<f64> {
    if blocks.is_empty() {
        return Some(0.0);
    }
    let mean = blocks.iter().map(|b| b.1).sum::<f64>() / blocks.len() as f64;
    let i = blocks.iter().rposition(|b| b.0 <= t).unwrap_or(0);
    let jump = |a: usize, b: usize| match (blocks.get(a), blocks.get(b)) {
        (Some(x), Some(y)) => (x.1 - y.1).abs() > JUMP_MS,
        _ => false,
    };
    (!(i > 0 && jump(i - 1, i)) && !jump(i, i + 1)).then(|| blocks[i].1 - mean)
}

/// Assigns every judgement change to the note whose expected span `(start, end)` (chart s, a test's shift
/// included) is nearest after removing `lag`; ties go to the shorter span (a tap inside a hold body).
pub fn assign(events: &[Event], spans: &[(f64, f64)], lag: f64) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); spans.len()];
    for (k, e) in events.iter().enumerate() {
        if e.judgements().is_empty() {
            continue;
        }
        let t = e.t - lag;
        let score = |&(s, end): &(f64, f64)| (s - t).max(t - end).max(0.0) + (end - s) * 1e-3;
        let best = spans
            .iter()
            .enumerate()
            .min_by(|a, b| score(a.1).total_cmp(&score(b.1)));
        if let Some((i, span)) = best.filter(|(_, sp)| score(sp) <= MAX_DISTANCE) {
            let _ = span;
            out[i].push(k);
        }
    }
    out
}

/// The outcome of `test` from the panel changes assigned to it.
pub fn resolve(test: &Test, hits: &[&Event], lag: f64) -> Outcome {
    let seen: Vec<Judgement> = hits.iter().flat_map(|e| e.judgements()).collect();
    let single = (hits.len() == 1 && seen.len() == 1).then(|| hits[0]);
    Outcome {
        test: test.clone(),
        real: single.map(|_| seen[0]),
        fast: single.is_some_and(Event::fast),
        late: single.is_some_and(Event::late),
        shown: single.map(|e| e.t - lag - test.time),
        seen,
        clock_ms: Some(0.0),
    }
}

fn shift_ms(t: &Test) -> Option<f64> {
    match t.probe {
        Probe::Shift { ms } => Some(ms),
        _ => None,
    }
}

/// Shift (ms) of a shift test on the judge clock (script shift minus clock drift).
fn judged_shift(o: &Outcome) -> Option<f64> {
    Some(shift_ms(&o.test)? - o.clock_ms?)
}

/// `(judged shift ms, real rank)` of a run's resolved shift tests of `class`.
fn sweep(outcomes: &[Outcome], class: Class) -> Vec<(f64, usize)> {
    outcomes
        .iter()
        .filter(|o| o.test.class == class)
        .filter_map(|o| Some((judged_shift(o)?, rank(o.real?))))
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
    /// Injector write error (actual - scheduled) of the edges: p95 and max (ms).
    pub write_p95_ms: f64,
    pub write_max_ms: f64,
    pub outcomes: Vec<Outcome>,
}

/// p95 and max of write errors (ms); `(0, 0)` without samples.
pub fn write_stats(errors: &[f64]) -> (f64, f64) {
    let mut v: Vec<f64> = errors.to_vec();
    v.sort_by(f64::total_cmp);
    match v.last() {
        Some(&max) => (
            v[((v.len() as f64 * 0.95).ceil() as usize).clamp(1, v.len()) - 1],
            max,
        ),
        None => (0.0, 0.0),
    }
}

/// Largest write error (ms) of an edge that a test may have.
pub const MAX_WRITE_ERROR_MS: f64 = 2.0;

/// A run is usable for 1-frame timing when the injector kept its schedule (p95 <= 2 ms); single late edges only
/// remove their own test notes ([`late_writes`]).
pub fn writes_ok(r: &RunResult) -> bool {
    r.write_p95_ms <= MAX_WRITE_ERROR_MS
}

/// Whether any edge at chart times `late` (s) falls within a test's span (±0.3 s).
pub fn touched_by(test: &Test, late: &[f64]) -> bool {
    late.iter()
        .any(|&t| t >= test.time - 0.3 && t <= test.end + 0.3)
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
    for class in [
        Class::Touch,
        Class::Hold,
        Class::Chain,
        Class::Slide,
        Class::Snap,
    ] {
        let pts: Vec<(f64, usize)> = runs
            .iter()
            .filter_map(|r| Some((r.centre_ms?, r)))
            .flat_map(|(c, r)| {
                sweep(&r.outcomes, class)
                    .into_iter()
                    .map(move |(x, k)| (frames(x - c), k))
            })
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
pub fn report(all: &[RunResult]) -> String {
    let mut s = String::new();
    let runs: Vec<RunResult> = all.iter().filter(|r| writes_ok(r)).cloned().collect();
    let runs = &runs[..];
    let _ = writeln!(
        s,
        "## Runs\n\n| Run | Lag ms | Touch centre ms | Write err p95 / max ms | Tests | Resolved |\n|---|---|---|---|---|---|"
    );
    for r in all {
        let resolved = r.outcomes.iter().filter(|o| o.real.is_some()).count();
        let _ = writeln!(
            s,
            "| {} | {:.1} | {} | {:.2} / {:.2}{} | {} | {resolved} |",
            r.run,
            r.lag * 1000.0,
            r.centre_ms.map_or("–".into(), |c| format!("{c:.1}")),
            r.write_p95_ms,
            r.write_max_ms,
            if writes_ok(r) { "" } else { " **excluded**" },
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
    let _ = writeln!(
        s,
        "\n## Sweeps (frames from the touch centre, rounded to ½)\n"
    );
    for class in [
        Class::Touch,
        Class::Hold,
        Class::Chain,
        Class::Slide,
        Class::Snap,
    ] {
        let mut rows: BTreeMap<i64, [usize; 5]> = BTreeMap::new();
        for r in runs {
            let Some(c) = r.centre_ms else { continue };
            for o in r.outcomes.iter().filter(|o| o.test.class == class) {
                let Some(ms) = judged_shift(o) else { continue };
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
            .map(|(k, c)| {
                format!(
                    "{:+.1}: {}/{}/{}/{}/{}",
                    *k as f64 / 2.0,
                    c[0],
                    c[1],
                    c[2],
                    c[3],
                    c[4]
                )
            })
            .collect();
        let _ = writeln!(s, "  {}\n", line.join(" · "));
    }
    let _ = writeln!(
        s,
        "## When misses are shown (frames after the note, relative to a centred hit)\n"
    );
    for class in [
        Class::Touch,
        Class::Hold,
        Class::Chain,
        Class::Slide,
        Class::Snap,
    ] {
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
            let Some(&k) = hit.get(hit.len() / 2) else {
                continue;
            };
            let hit0 = k + c / 1000.0;
            v.extend(
                r.outcomes
                    .iter()
                    .filter(|o| o.test.class == class && o.real == Some(Judgement::Miss))
                    .filter(|o| matches!(o.test.probe, Probe::Shift { .. }))
                    .filter_map(|o| {
                        Some(frames((o.shown? - o.clock_ms? / 1000.0 - hit0) * 1000.0))
                    }),
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
    let _ = writeln!(
        s,
        "\n## Other probes\n\nThe probed note is otherwise played at script offset 0, which lands at the start offset below \
         on the judge clock (frames from the touch centre); `start` is what a touch there would get.\n\n\
         | Probe | Start offset | start | Real | Ours |\n|---|---|---|---|---|"
    );
    let touch = edges(runs);
    let tier_at = |x: f64| {
        (0..3)
            .find(|&t| {
                touch
                    .get(&(Class::Touch, t))
                    .is_some_and(|e| x > e.0 && x < e.1)
            })
            .map_or("Miss", |t| ["M", "Gr", "Gd"][t])
    };
    let mut rows: Vec<(String, f64, String)> = Vec::new();
    for r in runs {
        let Some(c) = r.centre_ms else { continue };
        // Probes next to a clock jump are left out: their timing is unknown.
        for o in r.outcomes.iter() {
            let Some(clock) = o.clock_ms else { continue };
            let key = match o.test.probe {
                Probe::Shift { .. } => continue,
                Probe::HoldGap { at_ms, ms } => format!("hold gap {ms:>3.0} ms at +{at_ms:.0} ms"),
                Probe::HoldRelease { ms } => format!("hold released {ms:>3.0} ms before end"),
                Probe::SlideJump { lanes } => format!("slide jump {lanes:.1} lanes"),
                Probe::SnapMove { from, to } => format!("snap ring {from}→{to}"),
                Probe::Press { .. } => continue,
            };
            let start = frames(-clock - c);
            let cells = format!(
                "| {} | {} | {} |",
                tier_at(start),
                name(o.real),
                name(o.test.ours)
            );
            rows.push((key, start, cells));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (key, start, cells) in rows {
        let _ = writeln!(s, "| {key} | {start:+.1} {cells}");
    }
    let _ = writeln!(
        s,
        "
## Chain presses (E5)

Contact on/off in frames from the note on the judge clock (scripted + drift +          touch centre).

| Scripted on..off | On | Off | Real | Ours |
|---|---|---|---|---|"
    );
    let mut rows: Vec<(f64, f64, String)> = Vec::new();
    for r in runs {
        let Some(c) = r.centre_ms else { continue };
        for o in &r.outcomes {
            let (Some(clock), Probe::Press { from_ms, to_ms }) = (o.clock_ms, o.test.probe) else {
                continue;
            };
            let on = frames(from_ms - clock - c);
            let off = frames(to_ms - clock - c);
            rows.push((
                on,
                off,
                format!(
                    "| {:+.1}..{:+.1} | {on:+.2} | {off:+.2} | {} | {} |",
                    frames(from_ms),
                    frames(to_ms),
                    name(o.real),
                    name(o.test.ours)
                ),
            ));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (_, _, row) in rows {
        let _ = writeln!(s, "{row}");
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
    fn late_edges_remove_their_tests() {
        let t = test(10.0, Probe::Shift { ms: 0.0 });
        assert!(touched_by(&t, &[10.2]));
        assert!(!touched_by(&t, &[10.4, 9.0]));
    }

    #[test]
    fn clock_drift_and_jumps() {
        let blocks = [
            (0.0, -80.0),
            (10.0, -70.0),
            (20.0, -60.0),
            (30.0, 40.0),
            (40.0, 40.0),
        ];
        assert_eq!(clock_at(&blocks, 5.0), Some(-80.0 + 26.0));
        assert_eq!(clock_at(&blocks, 15.0), Some(-70.0 + 26.0));
        assert_eq!(clock_at(&blocks, 25.0), None, "block before the jump");
        assert_eq!(clock_at(&blocks, 35.0), None, "block after the jump");
        assert_eq!(clock_at(&blocks, 45.0), Some(40.0 + 26.0));
        assert_eq!(clock_at(&[], 1.0), Some(0.0));
    }

    #[test]
    fn write_error_stats() {
        let e: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(write_stats(&e), (19.0, 20.0));
        assert_eq!(write_stats(&[]), (0.0, 0.0));
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
        let events = [ev(1.05, 1), ev(2.06, 0), ev(2.2, 3), ev(2.6, 0), ev(5.0, 0)];
        // Notes at 1, 2 and 2.7 s (the last one shifted from 3.0 to 2.7) and a hold 4..5 s.
        let spans = [(1.0, 1.0), (2.0, 2.0), (2.7, 2.7), (4.0, 5.0)];
        let a = assign(&events, &spans, 0.03);
        assert_eq!(a, [vec![0], vec![1, 2], vec![3], vec![4]]);
        let pick = |i: usize| a[i].iter().map(|&k| &events[k]).collect::<Vec<_>>();
        let o = resolve(&test(1.0, Probe::Shift { ms: 0.0 }), &pick(0), 0.03);
        assert_eq!(o.real, Some(Judgement::Great));
        assert!((o.shown.unwrap() - 0.02).abs() < 1e-9);
        let two = resolve(&test(2.0, Probe::Shift { ms: 0.0 }), &pick(1), 0.03);
        assert_eq!((two.real, two.seen.len()), (None, 2));
        assert_eq!(
            resolve(&test(3.5, Probe::Shift { ms: 0.0 }), &[], 0.03).real,
            None
        );
    }

    fn outcome(ms: f64, real: Judgement) -> Outcome {
        Outcome {
            test: test(0.0, Probe::Shift { ms }),
            real: Some(real),
            fast: false,
            late: false,
            shown: Some(ms / 1000.0),
            seen: vec![real],
            clock_ms: Some(0.0),
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
            write_p95_ms: 0.5,
            write_max_ms: 1.0,
            outcomes,
        };
        let e = edges(std::slice::from_ref(&run));
        let (lo, hi, err, n) = e[&(Class::Touch, 1)];
        assert!((frames(-85.0) - lo).abs() < 1e-9 && (frames(85.0) - hi).abs() < 1e-9);
        assert_eq!((err, n), (0, 25));
        assert!(report(&[run]).contains("| Touch | Marvelous |"));
    }
}
