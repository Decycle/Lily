//! Experiment E4 (doc 10 §8): offline judge parity. The exact input an E3 run injected (`inject.jsonl`) is put on
//! the game's judge clock and fed through our [`wacca_game::judge::Judge`]; its verdicts are compared per note with
//! the real ones read from the debug panel ([`crate::e3_analysis`]).
//!
//! Judge clock: an edge written at chart (audio) time `t` is judged at `t - δ(t) - c`, where `δ` is the run's
//! visual-timing drift of the edge's 8-note block relative to the run mean (`analyze_e2`, as in
//! [`crate::e3_analysis::clock_at`]) and `c` the run's touch Marvelous centre (script ms that land on the centre of
//! the touch window). The real game samples input once per 60 Hz frame, so verdicts within about one frame of a
//! window or rule edge are measurement noise ([`NOISE_FRAMES`]).

use crate::cells::from_cells;
use crate::e3::{Class, Probe};
use crate::inject::{Kind, Record, cells_from_hex};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write;
use wacca_chart::{Arc, Chart, HoldNode, LANES, NoteBody};
use wacca_game::input::{InputFrame, SensorState};
use wacca_game::judge::{HOLD_DROP_LENIENCY, JudgeTable, class_of};
use wacca_game::rules::{FRAME_SECS, JudgeClass, Judgement};

/// A disagreement this close (frames) to a window or rule edge is attributed to 1-frame input sampling.
pub const NOISE_FRAMES: f64 = 1.0;

/// Visual-timing drift (ms) at chart time `t` relative to the mean of all blocks `(start s, δ ms)`; 0 without
/// blocks. Unlike [`crate::e3_analysis::clock_at`] it never refuses (input next to a jump still needs a time).
pub fn drift_ms(blocks: &[(f64, f64)], t: f64) -> f64 {
    if blocks.is_empty() {
        return 0.0;
    }
    let mean = blocks.iter().map(|b| b.1).sum::<f64>() / blocks.len() as f64;
    let i = blocks.iter().rposition(|b| b.0 <= t).unwrap_or(0);
    blocks[i].1 - mean
}

/// Judge-clock chart time (s) of an input written at chart time `t` (s).
pub fn judge_time(blocks: &[(f64, f64)], centre_ms: f64, t: f64) -> f64 {
    t - (drift_ms(blocks, t) + centre_ms) / 1000.0
}

/// The injected sensor states (edge and clear records, at their actual write time) on the judge clock. Times
/// are kept non-decreasing where a drift step between blocks would reorder them.
pub fn injected_frames(
    records: &[Record],
    blocks: &[(f64, f64)],
    centre_ms: f64,
) -> Vec<InputFrame> {
    let mut out: Vec<InputFrame> = Vec::new();
    for r in records.iter().filter(|r| r.kind != Kind::Renew) {
        let sensors = match r.kind {
            Kind::Clear => SensorState::default(),
            _ => match cells_from_hex(&r.cells) {
                Some(c) => from_cells(&c),
                None => {
                    log::warn!("record {}: bad cells {:?}", r.seq, r.cells);
                    continue;
                }
            },
        };
        let written = (r.t_script_ms + r.write_error_ms()) / 1000.0;
        let t = judge_time(blocks, centre_ms, written);
        let time = out.last().map_or(t, |f| f.time.max(t));
        out.push(InputFrame { time, sensors });
    }
    out
}

/// Repeats each state every `step` seconds until the next edge, as the game loop feeds the judge every frame (an
/// edge list alone never shows a chain or hold body the state held between two edges).
pub fn densify(frames: &[InputFrame], step: f64) -> Vec<InputFrame> {
    let mut out = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        out.push(*f);
        let Some(next) = frames.get(i + 1) else {
            continue;
        };
        let mut t = f.time + step;
        while t < next.time - 1e-9 {
            out.push(InputFrame { time: t, ..*f });
            t += step;
        }
    }
    out
}

/// Hold rows of a `.mer` body: note index -> (render flag, linked next index). The flag is the column after the
/// width (1 = authored control point drawn by the game, 0 = generated joint).
pub fn hold_rows(mer: &str) -> HashMap<u32, (bool, Option<u32>)> {
    let mut out = HashMap::new();
    for line in mer.lines() {
        let t: Vec<i64> = line
            .split_whitespace()
            .map_while(|s| s.parse().ok())
            .collect();
        if let [
            _,
            _,
            1,
            kind @ (9 | 10 | 11 | 25),
            index,
            _,
            _,
            flag,
            ref rest @ ..,
        ] = t[..]
        {
            let next = (kind != 11)
                .then(|| rest.first().and_then(|&n| u32::try_from(n).ok()))
                .flatten();
            if let Ok(index) = u32::try_from(index) {
                out.insert(index, (flag != 0, next));
            }
        }
    }
    out
}

/// Control-point flags of the hold whose head row has `head` index, following the links (start and end always
/// count as control points); `None` if the chain does not have `len` nodes.
pub fn control_flags(
    rows: &HashMap<u32, (bool, Option<u32>)>,
    head: u32,
    len: usize,
) -> Option<Vec<bool>> {
    let mut out = Vec::with_capacity(len);
    let mut i = Some(head);
    while let Some(idx) = i {
        if out.len() == len {
            break;
        }
        let &(flag, next) = rows.get(&idx)?;
        out.push(flag);
        i = next;
    }
    if out.len() != len {
        return None;
    }
    if let Some(f) = out.first_mut() {
        *f = true;
    }
    if let Some(f) = out.last_mut() {
        *f = true;
    }
    Some(out)
}

fn lerp_node(a: &HoldNode, b: &HoldNode, t: f64) -> (f64, f64) {
    let k = if b.time > a.time {
        ((t - a.time) / (b.time - a.time)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let lane = f64::from(a.lane_unwrapped) + k * f64::from(b.lane_unwrapped - a.lane_unwrapped);
    let width = f64::from(a.arc.width) + k * (f64::from(b.arc.width) - f64::from(a.arc.width));
    (lane, width)
}

fn node_at(time: f64, lane: f64, width: f64, pos: wacca_chart::BeatPos) -> HoldNode {
    let l = lane.round();
    HoldNode {
        pos,
        time,
        arc: Arc {
            lane: l.rem_euclid(f64::from(LANES)) as u8,
            width: width.round().clamp(1.0, f64::from(LANES)) as u8,
        },
        lane_unwrapped: l as f32,
        // Samples of the drawn (control-point) path are themselves on that path.
        control: true,
    }
}

/// The hold path the game draws: straight (linear lane and width) between control points, as nodes every frame.
pub fn control_path(nodes: &[HoldNode], controls: &[bool]) -> Vec<HoldNode> {
    let ctrl: Vec<&HoldNode> = nodes
        .iter()
        .zip(controls)
        .filter_map(|(n, &c)| c.then_some(n))
        .collect();
    let mut out = Vec::new();
    for w in ctrl.windows(2) {
        let (a, b) = (w[0], w[1]);
        let steps = ((b.time - a.time) / FRAME_SECS).floor().max(0.0) as usize;
        for k in 0..=steps {
            let t = a.time + k as f64 * FRAME_SECS;
            if t >= b.time - 1e-9 && k > 0 {
                break;
            }
            let (lane, width) = lerp_node(a, b, t);
            out.push(node_at(t, lane, width, a.pos));
        }
    }
    if let Some(last) = ctrl.last() {
        out.push(**last);
    }
    out
}

/// Largest distance (lanes, lane or width) of a generated joint from the straight path between its control points.
pub fn path_deviation(nodes: &[HoldNode], controls: &[bool]) -> f64 {
    let mut worst: f64 = 0.0;
    let mut prev = 0;
    for (i, &c) in controls.iter().enumerate().skip(1) {
        if !c {
            continue;
        }
        for n in &nodes[prev + 1..i] {
            let (lane, width) = lerp_node(&nodes[prev], &nodes[i], n.time);
            worst = worst
                .max((f64::from(n.lane_unwrapped) - lane).abs())
                .max((f64::from(n.arc.width) - width).abs());
        }
        prev = i;
    }
    worst
}

/// `chart` with every hold whose flags are known replaced by its [`control_path`].
pub fn with_control_paths(chart: &Chart, flags: &[Option<Vec<bool>>]) -> Chart {
    let mut c = chart.clone();
    for (n, f) in c.notes.iter_mut().zip(flags) {
        if let (NoteBody::Hold { nodes }, Some(f)) = (&mut n.body, f) {
            *nodes = control_path(nodes, f);
        }
    }
    c
}

/// Window edges (frames, signed) of a judge class.
fn edges(table: &JudgeTable, class: JudgeClass) -> Vec<f64> {
    let w = table.windows(class);
    [w.marvelous, w.great, w.good]
        .iter()
        .flat_map(|x| [-x.early, x.late])
        .collect()
}

/// Distance (frames) of a note's input from the nearest edge that could flip its verdict: the start's window
/// edges at `offset_f` and, for hold probes, the drop allowance (gap length, or release time before the end
/// against allowance + HoldEnd). `clock_f` is the script → judge clock offset (frames): a release scripted `ms`
/// before the end lands `ms - clock` before it on the judge clock.
pub fn margin_frames(
    table: &JudgeTable,
    class: JudgeClass,
    probe: Option<Probe>,
    offset_f: f64,
    clock_f: f64,
) -> f64 {
    let window = edges(table, class)
        .iter()
        .map(|e| (offset_f - e).abs())
        .fold(f64::INFINITY, f64::min);
    let allowance = HOLD_DROP_LENIENCY * 1000.0;
    let hold_end = table.windows(JudgeClass::HoldEnd).early_limit() * 1000.0;
    let rule = match probe {
        Some(Probe::HoldGap { ms, .. }) => (ms - allowance).abs(),
        Some(Probe::HoldRelease { ms }) => {
            (ms - clock_f * FRAME_SECS * 1000.0 - allowance - hold_end).abs()
        }
        _ => f64::INFINITY,
    } / (FRAME_SECS * 1000.0);
    window.min(rule)
}

/// One compared note.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub run: String,
    pub note: usize,
    pub time: f64,
    pub class: Class,
    /// The E3 probe on this note (`None`: played perfectly).
    pub probe: Option<Probe>,
    /// Input offset on the judge clock (frames; holds: the start): our judge's where it has one, else scripted.
    pub offset_f: f64,
    /// Script → judge clock offset at the note (frames, `-(δ + c)`).
    pub clock_f: f64,
    pub real: Judgement,
    pub ours: Option<Judgement>,
    /// Ours with holds following straight lines between control points (only holds with generated joints).
    pub ours_control: Option<Judgement>,
    /// Holds: largest deviation (lanes) of a generated joint from the straight control path.
    pub path_dev: Option<f64>,
    pub margin_f: f64,
}

impl Row {
    pub fn agrees(&self) -> bool {
        self.ours == Some(self.real)
    }

    pub fn diagnosis(&self) -> &'static str {
        match () {
            _ if self.agrees() => "",
            _ if self.margin_f <= NOISE_FRAMES => "noise (≤ 1 f from an edge)",
            _ => "rule?",
        }
    }
}

pub fn name(j: Option<Judgement>) -> &'static str {
    match j {
        Some(Judgement::Marvelous) => "M",
        Some(Judgement::Great) => "Gr",
        Some(Judgement::Good) => "Gd",
        Some(Judgement::Miss) => "Miss",
        None => "–",
    }
}

pub fn probe_text(p: Option<Probe>) -> String {
    match p {
        None => "perfect".into(),
        Some(Probe::Shift { ms }) => format!("shift {ms:+.0} ms"),
        Some(Probe::HoldGap { at_ms, ms }) => format!("gap {ms:.0} ms at +{at_ms:.0}"),
        Some(Probe::HoldRelease { ms }) => format!("release {ms:.0} ms before end"),
        Some(Probe::SlideJump { lanes }) => format!("jump {lanes:.1} lanes"),
        Some(Probe::SnapMove { from, to }) => format!("ring {from}→{to}"),
    }
}

/// Real × ours counts, rows/columns M, Gr, Gd, Miss (ours also "none" = never judged).
pub type Confusion = [[usize; 5]; 4];

fn rank(j: Option<Judgement>) -> usize {
    j.map_or(4, crate::e3::rank)
}

pub fn confusion(rows: &[&Row]) -> Confusion {
    let mut m = [[0; 5]; 4];
    for r in rows {
        m[rank(Some(r.real))][rank(r.ours)] += 1;
    }
    m
}

const CLASSES: [Class; 5] = [
    Class::Touch,
    Class::Hold,
    Class::Chain,
    Class::Slide,
    Class::Snap,
];

/// Markdown report: agreement and confusion per class (probed and perfect notes), then every disagreement.
pub fn report(rows: &[Row]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "## Agreement\n\n| Class | Probed agree / n | % | Perfect agree / n | % | > 1 f from edges agree / n | % | Disagreements: noise / rule? |\n|---|---|---|---|---|---|---|---|"
    );
    let pct = |a: usize, n: usize| {
        if n == 0 {
            "–".to_string()
        } else {
            format!("{:.1}", 100.0 * a as f64 / n as f64)
        }
    };
    for class in CLASSES {
        let of = |probed: bool| -> (usize, usize) {
            let v: Vec<&Row> = rows
                .iter()
                .filter(|r| r.class == class && r.probe.is_some() == probed)
                .collect();
            (v.iter().filter(|r| r.agrees()).count(), v.len())
        };
        let ((pa, pn), (fa, fn_)) = (of(true), of(false));
        let bad: Vec<&Row> = rows
            .iter()
            .filter(|r| r.class == class && !r.agrees())
            .collect();
        let noise = bad.iter().filter(|r| r.margin_f <= NOISE_FRAMES).count();
        let clear: Vec<&Row> = rows
            .iter()
            .filter(|r| r.class == class && r.margin_f > NOISE_FRAMES)
            .collect();
        let (ca, cn) = (clear.iter().filter(|r| r.agrees()).count(), clear.len());
        let _ = writeln!(
            s,
            "| {class:?} | {pa} / {pn} | {} | {fa} / {fn_} | {} | {ca} / {cn} | {} | {noise} / {} |",
            pct(pa, pn),
            pct(fa, fn_),
            pct(ca, cn),
            bad.len() - noise
        );
    }
    let _ = writeln!(
        s,
        "\n## Confusion (rows real, columns ours; all compared notes)\n"
    );
    for class in CLASSES {
        let v: Vec<&Row> = rows.iter().filter(|r| r.class == class).collect();
        if v.is_empty() {
            continue;
        }
        let m = confusion(&v);
        let _ = writeln!(
            s,
            "{class:?}:\n\n| real \\ ours | M | Gr | Gd | Miss | none |\n|---|---|---|---|---|---|"
        );
        for (i, row) in m.iter().enumerate() {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} |",
                ["M", "Gr", "Gd", "Miss"][i],
                row[0],
                row[1],
                row[2],
                row[3],
                row[4]
            );
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(
        s,
        "## Disagreements\n\n| Run | Note | Class | Probe | Offset f | Real | Ours | Ours (control path) | Margin f | Diagnosis |\n|---|---|---|---|---|---|---|---|---|---|"
    );
    for r in rows.iter().filter(|r| !r.agrees()) {
        let _ = writeln!(
            s,
            "| {} | {} | {:?} | {} | {:+.1} | {} | {} | {} | {:.1} | {} |",
            r.run,
            r.note,
            r.class,
            probe_text(r.probe),
            r.offset_f,
            name(Some(r.real)),
            name(r.ours),
            name(r.ours_control),
            r.margin_f,
            r.diagnosis()
        );
    }
    let holds: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            r.path_dev.is_some_and(|d| d > 0.0)
                || r.ours_control != r.ours && r.class == Class::Hold
        })
        .collect();
    let changed: Vec<&&Row> = holds.iter().filter(|r| r.ours_control != r.ours).collect();
    let _ = writeln!(
        s,
        "\n## Hold paths: all joints vs control points\n\n{} compared holds have generated joints off the straight \
         control path (max {:.1} lanes); our verdict changes on {} of them with the control path.\n",
        holds.len(),
        holds.iter().filter_map(|r| r.path_dev).fold(0.0, f64::max),
        changed.len()
    );
    if !changed.is_empty() {
        let _ = writeln!(
            s,
            "| Run | Note | Probe | Dev lanes | Real | Ours (all joints) | Ours (control) |\n|---|---|---|---|---|---|---|"
        );
        for r in changed {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {:.1} | {} | {} | {} |",
                r.run,
                r.note,
                probe_text(r.probe),
                r.path_dev.unwrap_or(0.0),
                name(Some(r.real)),
                name(r.ours),
                name(r.ours_control)
            );
        }
    }
    s
}

/// Per-note table as CSV.
pub fn csv(rows: &[Row]) -> String {
    let mut s = String::from(
        "run,note,time_s,class,probe,offset_f,real,ours,ours_control,path_dev,margin_f,clock_f,diagnosis\n",
    );
    for r in rows {
        let _ = writeln!(
            s,
            "{},{},{:.3},{:?},{},{:.2},{},{},{},{},{:.2},{:.2},{}",
            r.run,
            r.note,
            r.time,
            r.class,
            probe_text(r.probe),
            r.offset_f,
            name(Some(r.real)),
            name(r.ours),
            name(r.ours_control),
            r.path_dev.map_or(String::new(), |d| format!("{d:.2}")),
            r.margin_f,
            r.clock_f,
            r.diagnosis()
        );
    }
    s
}

/// Judge class of a chart note (for [`margin_frames`]).
pub fn judge_class(chart: &Chart, note: usize) -> Option<JudgeClass> {
    chart.notes.get(note).map(class_of)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::to_cells;
    use crate::inject::cells_hex;
    use wacca_chart::BeatPos;

    fn rec(kind: Kind, t: f64, err: f64, s: &SensorState) -> Record {
        Record {
            kind,
            seq: 0,
            t_script_ms: t,
            t_sched_ms: t - 1000.0,
            t_write_ms: t - 1000.0 + err,
            t_ack_ms: None,
            opbtn: 0,
            cells: cells_hex(&to_cells(s)),
        }
    }

    #[test]
    fn frames_on_the_judge_clock() {
        let mut on = SensorState::default();
        on.set(7, 2, true);
        let blocks = [(0.0, -10.0), (2.0, 10.0)];
        let recs = [
            rec(Kind::Edge, 1000.0, 0.5, &on),
            rec(Kind::Renew, 1050.0, 0.0, &on),
            rec(Kind::Edge, 1990.0, 0.0, &SensorState::default()),
            rec(Kind::Edge, 2000.0, 0.0, &on),
            rec(Kind::Clear, 3000.0, 0.0, &on),
        ];
        let f = injected_frames(&recs, &blocks, 5.0);
        let t: Vec<f64> = f.iter().map(|f| (f.time * 1e4).round() / 1e4).collect();
        // Block 1: drift -10 + centre 5 -> +5 ms later; block 2: +15 ms earlier, clamped to stay ordered.
        assert_eq!(t, [1.0055, 1.995, 1.995, 2.985]);
        assert!(f[0].sensors.get(7, 2) && f[1].sensors.is_empty() && f[3].sensors.is_empty());
        assert_eq!(drift_ms(&[], 1.0), 0.0);
        let d = densify(&f, 0.25);
        let t: Vec<f64> = d.iter().map(|f| (f.time * 1e4).round() / 1e4).collect();
        assert_eq!(
            t,
            [
                1.0055, 1.2555, 1.5055, 1.7555, 1.995, 1.995, 2.245, 2.495, 2.745, 2.985
            ]
        );
        assert!(d[3].sensors.get(7, 2) && d[4].sensors.is_empty() && d[6].sensors.get(7, 2));
    }

    const MER: &str = "#BODY\n\
        1 0 1 9 100 10 4 1 101\n\
        1 480 1 10 101 14 4 0 102\n\
        1 960 1 10 102 20 4 1 103\n\
        1 1440 1 11 103 20 4 1\n\
        1 0 1 1 7 0 4 1\n";

    fn n(time: f64, lane: u8, width: u8) -> HoldNode {
        HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width },
            lane_unwrapped: f32::from(lane),
            control: true,
        }
    }

    #[test]
    fn control_flags_follow_links() {
        let rows = hold_rows(MER);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            control_flags(&rows, 100, 4),
            Some(vec![true, false, true, true])
        );
        assert_eq!(control_flags(&rows, 100, 5), None);
        assert_eq!(control_flags(&rows, 7, 1), None, "not a hold row");
    }

    #[test]
    fn straight_paths_between_control_points() {
        let nodes = [n(0.0, 10, 4), n(0.5, 14, 4), n(1.0, 20, 4), n(1.5, 20, 4)];
        let ctrl = [true, false, true, true];
        // The joint at 0.5 s sits at 14, the straight line at 15.
        assert!((path_deviation(&nodes, &ctrl) - 1.0).abs() < 1e-9);
        assert_eq!(path_deviation(&nodes, &[true; 4]), 0.0);
        let p = control_path(&nodes, &ctrl);
        assert_eq!(p.first().map(|n| n.arc.lane), Some(10));
        let mid = p.iter().find(|n| (n.time - 0.5).abs() < 1e-9).unwrap();
        assert_eq!(mid.arc.lane, 15);
        assert_eq!(p.last().map(|n| n.time), Some(1.5));
        assert!(p.windows(2).all(|w| w[1].time > w[0].time));
        assert_eq!(p.len(), 60 + 30 + 1);
    }

    #[test]
    fn margins_cover_windows_and_hold_rules() {
        let t = JudgeTable::reverse();
        let m =
            |class, probe, offset_f, clock_f| margin_frames(&t, class, probe, offset_f, clock_f);
        assert!((m(JudgeClass::Touch, None, 3.4, 0.0) - 0.4).abs() < 1e-9);
        assert!((m(JudgeClass::Touch, None, -9.0, 0.0) - 3.0).abs() < 1e-9);
        let gap = Some(Probe::HoldGap {
            at_ms: 150.0,
            ms: 183.0,
        });
        assert!((m(JudgeClass::Hold, gap, 0.0, -2.0) - 0.48).abs() < 1e-9);
        let rel = Some(Probe::HoldRelease { ms: 300.0 });
        assert!((m(JudgeClass::Hold, rel, 0.0, 0.0) - 0.5).abs() < 1e-6);
        // Script 1 f ahead of the judge clock: the release lands 316.7 ms before the end.
        assert!((m(JudgeClass::Hold, rel, 0.0, -1.0) - 1.5).abs() < 1e-6);
    }

    fn row(real: Judgement, ours: Option<Judgement>, margin_f: f64) -> Row {
        Row {
            run: "r".into(),
            note: 1,
            time: 1.0,
            class: Class::Touch,
            probe: Some(Probe::Shift { ms: 0.0 }),
            offset_f: 0.0,
            clock_f: 0.0,
            real,
            ours,
            ours_control: ours,
            path_dev: None,
            margin_f,
        }
    }

    #[test]
    fn confusion_and_report() {
        let rows = [
            row(Judgement::Marvelous, Some(Judgement::Marvelous), 3.0),
            row(Judgement::Great, Some(Judgement::Marvelous), 0.3),
            row(Judgement::Miss, Some(Judgement::Good), 2.0),
        ];
        let m = confusion(&rows.iter().collect::<Vec<_>>());
        assert_eq!((m[0][0], m[1][0], m[3][2]), (1, 1, 1));
        assert_eq!(rows[1].diagnosis(), "noise (≤ 1 f from an edge)");
        assert_eq!(rows[2].diagnosis(), "rule?");
        let r = report(&rows);
        assert!(
            r.contains("| Touch | 1 / 3 | 33.3 | 0 / 0 | – | 1 / 2 | 50.0 | 1 / 1 |"),
            "{r}"
        );
        assert_eq!(csv(&rows).lines().count(), 4);
    }
}
