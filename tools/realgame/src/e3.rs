//! Experiment E3 (doc 10): judgement-rule probes on real charts.
//!
//! The input is the perfect script of [`crate::perfect`] with a controlled change on selected *test notes*: notes
//! isolated in time from every other note (and hold body), so the debug panel's judgement change near a test note
//! belongs to it alone. Each test note gets one [`Probe`]: a timing shift (window sweeps), a hold release gap or
//! early release, a slide jump of a given distance, or a snap between two given rings. Everything else is played
//! perfectly. [`fit_interval`] turns sweep results into window edges.

use crate::perfect::{self, Contact};
use crate::timeline::{self, Tap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wacca_chart::{Arc, Chart, LANES, Note, NoteBody, SlideDir};
use wacca_game::input::{DEPTHS, InputFrame};
use wacca_game::judge::{Judge, JudgeTable};
use wacca_game::rules::{FRAME_SECS, Judgement};

/// Note classes probed separately (snap in/out and slide cw/ccw share one probe list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Touch,
    Hold,
    Chain,
    Slide,
    Snap,
}

pub fn class(body: &NoteBody) -> Class {
    match body {
        NoteBody::Touch => Class::Touch,
        NoteBody::Hold { .. } => Class::Hold,
        NoteBody::Chain => Class::Chain,
        NoteBody::Slide(_) => Class::Slide,
        NoteBody::Snap(_) => Class::Snap,
    }
}

/// One controlled change of the perfect input for one note.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "probe", rename_all = "snake_case")]
pub enum Probe {
    /// Every contact of the note moved by `ms` (a hold: its start only).
    Shift { ms: f64 },
    /// Hold released for `ms`, starting `at_ms` after the hold start.
    HoldGap { at_ms: f64, ms: f64 },
    /// Hold released `ms` before its end.
    HoldRelease { ms: f64 },
    /// Slide: all rings of the trailing-edge lane, then one jump `lanes` forward (x.5: the two lanes around).
    SlideJump { lanes: f64 },
    /// Snap: the whole arc on ring `from`, then on ring `to` (0 = innermost).
    SnapMove { from: u8, to: u8 },
}

/// A probe list in a spec file, expanded by [`ProbeSpec::expand`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProbeSpec {
    /// Shifts `from..=to` frames in `step` frames.
    Sweep { from: f64, to: f64, step: f64 },
    HoldGap { at_ms: f64, ms: Vec<f64> },
    HoldRelease { ms: Vec<f64> },
    SlideJump { lanes: Vec<f64> },
    SnapMove { moves: Vec<[u8; 2]> },
}

impl ProbeSpec {
    pub fn expand(&self) -> Vec<Probe> {
        match self {
            Self::Sweep { from, to, step } => {
                let n = ((to - from) / step + 1e-9).floor().max(-1.0) as i64;
                (0..=n)
                    .map(|i| Probe::Shift {
                        ms: (from + i as f64 * step) * FRAME_SECS * 1000.0,
                    })
                    .collect()
            }
            Self::HoldGap { at_ms, ms } => ms
                .iter()
                .map(|&ms| Probe::HoldGap { at_ms: *at_ms, ms })
                .collect(),
            Self::HoldRelease { ms } => ms.iter().map(|&ms| Probe::HoldRelease { ms }).collect(),
            Self::SlideJump { lanes } => lanes
                .iter()
                .map(|&lanes| Probe::SlideJump { lanes })
                .collect(),
            Self::SnapMove { moves } => moves
                .iter()
                .map(|&[from, to]| Probe::SnapMove { from, to })
                .collect(),
        }
    }
}

/// Experiment spec (`*.spec.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub seed: u64,
    /// No test note before this chart time (s): live sync needs the first seconds of the song.
    pub min_time: f64,
    /// Minimum distance (s) from a test note (or hold body) to any other note.
    pub isolation: f64,
    /// Contact length (s) of scripted presses; E1: one 16.7 ms frame always registers.
    pub min_on: f64,
    pub probes: BTreeMap<Class, Vec<ProbeSpec>>,
}

/// One probed note, with our judge's verdict for the generated input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Test {
    pub note: usize,
    pub time: f64,
    pub end: f64,
    pub class: Class,
    pub probe: Probe,
    pub ours: Option<Judgement>,
}

/// `*.plan.json`, written next to the tap script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub chart: String,
    pub spec: Spec,
    pub tests: Vec<Test>,
}

/// `x.taps.json` -> `x.plan.json`.
pub fn plan_path(taps: &std::path::Path) -> std::path::PathBuf {
    let name = taps.file_name().map_or_else(String::new, |n| {
        let n = n.to_string_lossy();
        format!("{}.plan.json", n.strip_suffix(".taps.json").unwrap_or(&n))
    });
    taps.with_file_name(name)
}

/// Start and end time of a note (a hold ends at its last node).
pub fn span(n: &Note) -> (f64, f64) {
    match &n.body {
        NoteBody::Hold { nodes } => (n.time, nodes.last().map_or(n.time, |e| e.time)),
        _ => (n.time, n.time),
    }
}

/// Notes at or after `min_time` whose span keeps `gap` seconds from every other note's span.
pub fn isolated(chart: &Chart, gap: f64, min_time: f64) -> Vec<usize> {
    let spans: Vec<(f64, f64)> = chart.notes.iter().map(span).collect();
    (0..spans.len())
        .filter(|&i| {
            let (s, e) = spans[i];
            s >= min_time
                && spans
                    .iter()
                    .enumerate()
                    .all(|(j, &(s2, e2))| j == i || e2 < s - gap || s2 > e + gap)
        })
        .collect()
}

/// Whether `p` can be applied to `n` (right note kind, long enough hold, wide enough slide).
pub fn feasible(n: &Note, p: &Probe) -> bool {
    let (start, end) = span(n);
    let len_ms = (end - start) * 1000.0;
    match (p, &n.body) {
        (Probe::Shift { ms }, NoteBody::Hold { nodes }) => nodes
            .get(1)
            .is_some_and(|next| (next.time - start) * 1000.0 > ms + 50.0),
        (Probe::Shift { .. }, _) => true,
        (Probe::HoldGap { at_ms, ms }, NoteBody::Hold { .. }) => len_ms >= at_ms + ms + 100.0,
        (Probe::HoldRelease { ms }, NoteBody::Hold { .. }) => len_ms >= ms + 100.0,
        (Probe::SlideJump { lanes }, NoteBody::Slide(_)) => {
            n.arc.width < LANES && f64::from(n.arc.width) >= lanes.ceil() + 1.0
        }
        (Probe::SnapMove { from, to }, NoteBody::Snap(_)) => {
            usize::from(*from) < DEPTHS && usize::from(*to) < DEPTHS && from != to
        }
        _ => false,
    }
}

/// Deterministic xorshift64* for the probe order.
#[derive(Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// Probes for the isolated notes of each class: the class's probe list is shuffled and dealt out in time order,
/// reshuffled each time it runs out; a probe that does not fit a note goes to the back of the queue.
pub fn assign(chart: &Chart, spec: &Spec) -> Vec<(usize, Probe)> {
    let mut rng = Rng(spec.seed.max(1));
    let candidates = isolated(chart, spec.isolation, spec.min_time);
    let mut out = Vec::new();
    for (class_, specs) in &spec.probes {
        let probes: Vec<Probe> = specs.iter().flat_map(ProbeSpec::expand).collect();
        if probes.is_empty() {
            continue;
        }
        let mut queue: std::collections::VecDeque<Probe> = Default::default();
        for &i in candidates
            .iter()
            .filter(|&&i| class(&chart.notes[i].body) == *class_)
        {
            if queue.is_empty() {
                let mut deck = probes.clone();
                rng.shuffle(&mut deck);
                queue.extend(deck);
            }
            for _ in 0..queue.len() {
                let Some(p) = queue.pop_front() else { break };
                if feasible(&chart.notes[i], &p) {
                    out.push((i, p));
                    break;
                }
                queue.push_back(p);
            }
        }
    }
    out.sort_by_key(|&(i, _)| i);
    out
}

fn lane_bit(lane: u8) -> u64 {
    1 << (lane % LANES)
}

fn arc_bits(a: Arc) -> u64 {
    (0..a.width).fold(0, |m, o| m | lane_bit(a.lane + o))
}

/// Lanes a note ever covers (a hold: every node).
fn note_bits(n: &Note) -> u64 {
    match &n.body {
        NoteBody::Hold { nodes } => nodes.iter().fold(0, |m, h| m | arc_bits(h.arc)),
        _ => arc_bits(n.arc),
    }
}

/// Removes `[from, to)` from every contact on a lane in `lanes`; parts outside the gap stay.
pub fn cut(cs: &[Contact], lanes: u64, from: f64, to: f64) -> Vec<Contact> {
    let mut out = Vec::with_capacity(cs.len());
    for c in cs {
        if lanes & lane_bit(c.lane) == 0 || c.end <= from || c.start >= to {
            out.push(*c);
            continue;
        }
        if c.start < from {
            out.push(Contact { end: from, ..*c });
        }
        if c.end > to {
            out.push(Contact { start: to, ..*c });
        }
    }
    out
}

fn cells(lanes: &[u8], depths: &[usize], start: f64, end: f64) -> Vec<Contact> {
    lanes
        .iter()
        .flat_map(|&lane| {
            depths.iter().map(move |&depth| Contact {
                lane: lane % LANES,
                depth,
                start,
                end,
            })
        })
        .collect()
}

/// Lead-in on the trailing-edge lane (all rings) for `[t - on, t)`, then the lane(s) `lanes` ahead for `[t, t + on)`.
fn slide_jump(n: &Note, dir: SlideDir, lanes: f64, on: f64) -> Vec<Contact> {
    let last = n.arc.width - 1;
    let at = |off: u8| match dir {
        SlideDir::Ccw => n.arc.lane + off,
        SlideDir::Cw => n.arc.lane + last - off,
    };
    let near = lanes.floor() as u8;
    let mut to = vec![at(near)];
    if lanes.fract() > 0.0 {
        to.push(at(near + 1));
    }
    let all: Vec<usize> = (0..DEPTHS).collect();
    let mut out = cells(&[at(0)], &all, n.time - on, n.time);
    out.extend(cells(&to, &all, n.time, n.time + on));
    out
}

fn snap_move(n: &Note, from: u8, to: u8, on: f64) -> Vec<Contact> {
    let lanes: Vec<u8> = (0..n.arc.width).map(|o| n.arc.lane + o).collect();
    let mut out = cells(&lanes, &[usize::from(from)], n.time - on, n.time);
    out.extend(cells(&lanes, &[usize::from(to)], n.time, n.time + on));
    out
}

/// Moves a note (a hold: its start node only) by `dt` seconds.
fn shift(n: &mut Note, dt: f64) {
    n.time += dt;
    if let NoteBody::Hold { nodes } = &mut n.body
        && let Some(first) = nodes.first_mut()
    {
        first.time += dt;
    }
}

/// Contacts for the whole chart: perfect input (contacts at least `on` long, `min_gap` apart per cell) with every
/// probe applied.
pub fn contacts(chart: &Chart, probes: &[(usize, Probe)], on: f64, min_gap: f64) -> Vec<Contact> {
    let mut shifted = chart.clone();
    for &(i, p) in probes {
        if let (Probe::Shift { ms }, Some(n)) = (p, shifted.notes.get_mut(i)) {
            shift(n, ms / 1000.0);
        }
    }
    let frames = wacca_game::judge::autoplay_frames(&shifted);
    let mut cs = perfect::stretch(&perfect::contacts(&frames), on, min_gap);
    for &(i, p) in probes {
        let Some(n) = chart.notes.get(i) else {
            continue;
        };
        let (start, end) = span(n);
        let bits = note_bits(n);
        match (p, &n.body) {
            (Probe::HoldGap { at_ms, ms }, _) => {
                let from = start + at_ms / 1000.0;
                cs = cut(&cs, bits, from, from + ms / 1000.0);
            }
            (Probe::HoldRelease { ms }, _) => cs = cut(&cs, bits, end - ms / 1000.0, end + 1.0),
            (Probe::SlideJump { lanes }, NoteBody::Slide(dir)) => {
                cs = cut(&cs, bits, start - 0.2, start + 0.2);
                cs.extend(slide_jump(n, *dir, lanes, on));
            }
            (Probe::SnapMove { from, to }, _) => {
                cs = cut(&cs, bits, start - 0.2, start + 0.2);
                cs.extend(snap_move(n, from, to, on));
            }
            _ => {}
        }
    }
    cs.sort_by(|a, b| a.start.total_cmp(&b.start));
    cs
}

/// Our judge's verdict per note for a tap script, the clock running one second past the last edge.
pub fn predict(chart: &Chart, taps: &[Tap]) -> Vec<Option<Judgement>> {
    let mut frames = timeline::expand(taps);
    if let Some(&last) = frames.last() {
        frames.push(InputFrame {
            time: last.time + 1.0,
            ..last
        });
    }
    let mut judge = Judge::new(chart, JudgeTable::reverse());
    let mut out = vec![None; chart.notes.len()];
    let mut events: Vec<_> = frames.iter().flat_map(|f| judge.update(f)).collect();
    events.extend(judge.finish());
    for e in events {
        if let Some(slot) = out.get_mut(e.note) {
            *slot = Some(e.judgement);
        }
    }
    out
}

/// Tap script and test list for `chart` under `spec`.
pub fn build(chart: &Chart, spec: &Spec) -> (Vec<Tap>, Vec<Test>) {
    let probes = assign(chart, spec);
    let taps = perfect::to_taps(&contacts(chart, &probes, spec.min_on, FRAME_SECS));
    let ours = predict(chart, &taps);
    let tests = probes
        .iter()
        .map(|&(i, probe)| {
            let n = &chart.notes[i];
            let (time, end) = span(n);
            Test {
                note: i,
                time,
                end,
                class: class(&n.body),
                probe,
                ours: ours[i],
            }
        })
        .collect();
    (taps, tests)
}

/// Best-to-worst rank of a judgement (Marvelous 0 .. Miss 3).
pub fn rank(j: Judgement) -> usize {
    match j {
        Judgement::Marvelous => 0,
        Judgement::Great => 1,
        Judgement::Good => 2,
        Judgement::Miss => 3,
    }
}

/// The interval `[lo, hi]` of `x` where `ok` holds that misclassifies the fewest points; each edge is the middle
/// of its optimal range (halfway between neighbouring samples), so with a soft transition it lands near the 50 %
/// point. Returns `(lo, hi, misclassified)`, or `None` without any `ok` point.
pub fn fit_interval(points: &[(f64, bool)]) -> Option<(f64, f64, usize)> {
    let mut xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    if !points.iter().any(|p| p.1) {
        return None;
    }
    // Candidate edges between neighbouring sample values (and one step outside the ends).
    let step = xs.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
    let step = if step.is_finite() { step } else { 1.0 };
    let mut cuts = vec![xs[0] - step / 2.0];
    cuts.extend(xs.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    cuts.push(xs[xs.len() - 1] + step / 2.0);
    // Errors = #bad + Σ_{x<lo}(ok - bad) + Σ_{x>hi}(ok - bad): the two edges separate.
    let score = |side: &dyn Fn(f64) -> bool| -> i64 {
        points
            .iter()
            .filter(|p| side(p.0))
            .map(|p| if p.1 { 1 } else { -1 })
            .sum()
    };
    let lo_scores: Vec<i64> = cuts.iter().map(|&c| score(&|x| x < c)).collect();
    let hi_scores: Vec<i64> = cuts.iter().map(|&c| score(&|x| x > c)).collect();
    let middle_of_best = |scores: &[i64]| -> (f64, i64) {
        let best = scores.iter().copied().min().unwrap_or(0);
        let at: Vec<f64> = cuts
            .iter()
            .zip(scores)
            .filter(|(_, s)| **s == best)
            .map(|(c, _)| *c)
            .collect();
        ((at[0] + at[at.len() - 1]) / 2.0, best)
    };
    let (lo, lo_s) = middle_of_best(&lo_scores);
    let (hi, hi_s) = middle_of_best(&hi_scores);
    let bad = points.iter().filter(|p| !p.1).count() as i64;
    Some((lo, hi, (bad + lo_s + hi_s).max(0) as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{BeatPos, Bonus, ChartHeader, HoldNode, SnapDir};

    fn note(time: f64, lane: u8, width: u8, body: NoteBody) -> Note {
        Note {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width },
            bonus: Bonus::None,
            body,
            file_index: 0,
        }
    }

    fn hold(time: f64, end: f64, lane: u8) -> Note {
        let node = |time| HoldNode {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width: 4 },
            lane_unwrapped: f32::from(lane),
        };
        note(
            time,
            lane,
            4,
            NoteBody::Hold {
                nodes: vec![node(time), node(end)],
            },
        )
    }

    fn chart(notes: Vec<Note>) -> Chart {
        Chart {
            header: ChartHeader::default(),
            notes,
            masks: vec![],
            tempo: vec![],
            scroll: vec![],
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 0.0,
        }
    }

    fn spec(probes: Vec<(Class, ProbeSpec)>) -> Spec {
        Spec {
            seed: 7,
            min_time: 1.0,
            isolation: 0.35,
            min_on: 0.025,
            probes: probes.into_iter().map(|(c, p)| (c, vec![p])).collect(),
        }
    }

    #[test]
    fn sweep_expands_in_frames() {
        let p = ProbeSpec::Sweep {
            from: -1.0,
            to: 1.0,
            step: 0.5,
        }
        .expand();
        let ms: Vec<f64> = p
            .iter()
            .map(|p| match p {
                Probe::Shift { ms } => (ms * 1000.0).round() / 1000.0,
                _ => f64::NAN,
            })
            .collect();
        assert_eq!(ms, [-16.667, -8.333, 0.0, 8.333, 16.667]);
    }

    #[test]
    fn isolation_counts_hold_bodies_and_min_time() {
        let c = chart(vec![
            note(0.5, 0, 4, NoteBody::Touch),
            note(2.0, 0, 4, NoteBody::Touch),
            hold(3.0, 4.0, 10),
            note(4.2, 20, 4, NoteBody::Touch),
            note(5.0, 30, 4, NoteBody::Touch),
            note(5.0, 40, 4, NoteBody::Touch),
            note(6.0, 30, 4, NoteBody::Touch),
        ]);
        // 0.5 is before min_time; the hold ends 0.2 s before 4.2; the chord pair is not isolated.
        assert_eq!(isolated(&c, 0.35, 1.0), [1, 6]);
    }

    #[test]
    fn assign_deals_every_probe_once_per_round() {
        let notes: Vec<Note> = (0..10)
            .map(|i| note(2.0 + f64::from(i), 0, 4, NoteBody::Touch))
            .collect();
        let c = chart(notes);
        let s = spec(vec![(
            Class::Touch,
            ProbeSpec::Sweep {
                from: -2.0,
                to: 2.0,
                step: 1.0,
            },
        )]);
        let a = assign(&c, &s);
        assert_eq!(a.len(), 10);
        let mut first: Vec<i64> = a[..5]
            .iter()
            .map(|(_, p)| match p {
                Probe::Shift { ms } => (ms / (FRAME_SECS * 1000.0)).round() as i64,
                _ => 99,
            })
            .collect();
        first.sort_unstable();
        assert_eq!(first, [-2, -1, 0, 1, 2]);
        assert_eq!(assign(&c, &s), a, "deterministic");
    }

    #[test]
    fn infeasible_probes_wait_for_a_fitting_note() {
        let c = chart(vec![
            note(2.0, 0, 3, NoteBody::Slide(SlideDir::Ccw)),
            note(3.0, 0, 6, NoteBody::Slide(SlideDir::Ccw)),
        ]);
        let s = spec(vec![(Class::Slide, ProbeSpec::SlideJump { lanes: vec![4.0] })]);
        assert_eq!(assign(&c, &s), [(1, Probe::SlideJump { lanes: 4.0 })]);
        assert!(feasible(&c.notes[0], &Probe::SlideJump { lanes: 2.0 }));
        assert!(!feasible(&c.notes[0], &Probe::SlideJump { lanes: 2.5 }));
        assert!(!feasible(&c.notes[0], &Probe::HoldGap { at_ms: 0.0, ms: 1.0 }));
    }

    #[test]
    fn cut_splits_contacts_on_the_lanes() {
        let cs = [
            Contact {
                lane: 5,
                depth: 3,
                start: 1.0,
                end: 2.0,
            },
            Contact {
                lane: 9,
                depth: 3,
                start: 1.0,
                end: 2.0,
            },
        ];
        let out = cut(&cs, lane_bit(5), 1.2, 1.5);
        let spans: Vec<(u8, f64, f64)> = out.iter().map(|c| (c.lane, c.start, c.end)).collect();
        assert_eq!(spans, [(5, 1.0, 1.2), (5, 1.5, 2.0), (9, 1.0, 2.0)]);
        assert_eq!(cut(&cs, lane_bit(5), 1.5, 3.0)[0].end, 1.5);
    }

    fn judged(c: &Chart, s: &Spec) -> Vec<Option<Judgement>> {
        let (_, tests) = build(c, s);
        tests.iter().map(|t| t.ours).collect()
    }

    #[test]
    fn shifts_reach_our_judge() {
        let c = chart(vec![
            note(2.0, 0, 4, NoteBody::Touch),
            note(3.0, 10, 4, NoteBody::Touch),
            note(4.0, 20, 4, NoteBody::Touch),
        ]);
        // Shifts of 0, +4 and -8 frames: Marvelous, Great, Miss (touch: 3/5/6 frames).
        let s = spec(vec![(
            Class::Touch,
            ProbeSpec::Sweep {
                from: -8.0,
                to: 4.0,
                step: 4.0,
            },
        )]);
        let probes = assign(&c, &s);
        let (_, tests) = build(&c, &s);
        for (t, (_, p)) in tests.iter().zip(&probes) {
            let Probe::Shift { ms } = p else { panic!() };
            let want = match (ms / (FRAME_SECS * 1000.0)).round() as i64 {
                0 => Judgement::Marvelous,
                4 => Judgement::Great,
                _ => Judgement::Miss,
            };
            assert_eq!(t.ours, Some(want), "{p:?}");
        }
    }

    #[test]
    fn hold_gaps_and_slide_and_snap_probes_reach_our_judge() {
        let c = chart(vec![hold(2.0, 3.0, 10)]);
        let gap = |ms| spec(vec![(Class::Hold, ProbeSpec::HoldGap { at_ms: 300.0, ms: vec![ms] })]);
        assert_eq!(judged(&c, &gap(150.0)), [Some(Judgement::Marvelous)]);
        assert_eq!(judged(&c, &gap(300.0)), [Some(Judgement::Good)]);
        let release = |ms| spec(vec![(Class::Hold, ProbeSpec::HoldRelease { ms: vec![ms] })]);
        assert_eq!(judged(&c, &release(500.0)), [Some(Judgement::Good)]);

        let c = chart(vec![note(2.0, 58, 6, NoteBody::Slide(SlideDir::Cw))]);
        let jump = |lanes| spec(vec![(Class::Slide, ProbeSpec::SlideJump { lanes: vec![lanes] })]);
        assert_eq!(judged(&c, &jump(1.5)), [Some(Judgement::Miss)]);
        assert_eq!(judged(&c, &jump(2.0)), [Some(Judgement::Marvelous)]);

        let c = chart(vec![note(2.0, 20, 4, NoteBody::Snap(SnapDir::In))]);
        let mv = |from, to| spec(vec![(Class::Snap, ProbeSpec::SnapMove { moves: vec![[from, to]] })]);
        assert_eq!(judged(&c, &mv(1, 0)), [Some(Judgement::Marvelous)]);
        assert_eq!(judged(&c, &mv(0, 3)), [Some(Judgement::Miss)]);
    }

    #[test]
    fn interval_fit() {
        let pts: Vec<(f64, bool)> = (-10..=10)
            .map(|x| (f64::from(x), (-3..=4).contains(&x)))
            .collect();
        assert_eq!(fit_interval(&pts), Some((-3.5, 4.5, 0)));
        // Stray points are counted, not followed.
        let mut stray = pts.clone();
        stray.extend([(-4.0, true), (8.0, true)]);
        assert_eq!(fit_interval(&stray), Some((-3.5, 4.5, 2)));
        // A soft edge (alternating samples) lands in the middle of its equally good cuts.
        let soft = [(0.0, false), (1.0, true), (2.0, false), (3.0, true), (4.0, true), (5.0, false)];
        assert_eq!(fit_interval(&soft), Some((1.5, 4.5, 1)));
        assert_eq!(fit_interval(&[(1.0, false)]), None);
    }
}
