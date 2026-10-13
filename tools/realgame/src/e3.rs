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
use wacca_chart::{Arc, Chart, LANES, Note, NoteBody, SlideDir, SnapDir};
use wacca_game::input::{DEPTHS, InputFrame};
use wacca_game::judge::{Judge, JudgeTable};
use wacca_game::rules::{FRAME_SECS, JudgeEvent, Judgement};

/// Note classes probed separately (snap in/out and slide cw/ccw share one probe list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Touch,
    Hold,
    Chain,
    Slide,
    /// Snap in (and, in plans made before E6, snap out).
    Snap,
    /// Snap out (E6).
    SnapOut,
    /// E6 two-note groups ([`isolated_pairs`]): two touches at the same time with disjoint / overlapping arcs, and
    /// two touches less than [`PAIR_DT`] apart with disjoint / overlapping arcs. The test note is the earlier one,
    /// its partner the next note.
    Chord,
    ChordOverlap,
    Pair,
    PairOverlap,
}

/// Largest start distance (s) of a close pair (E6).
pub const PAIR_DT: f64 = 0.19;

pub fn class(body: &NoteBody) -> Class {
    match body {
        NoteBody::Touch => Class::Touch,
        NoteBody::Hold { .. } => Class::Hold,
        NoteBody::Chain => Class::Chain,
        NoteBody::Slide(_) => Class::Slide,
        NoteBody::Snap(SnapDir::In) => Class::Snap,
        NoteBody::Snap(SnapDir::Out) => Class::SnapOut,
    }
}

/// Which lanes a group press covers (E6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cover {
    /// The shortest arc covering both notes (lanes between them included).
    Union,
    /// The earlier (first) note's arc only.
    First,
    /// The later (second) note's arc only.
    Second,
    /// Only the lanes both arcs share.
    Overlap,
}

/// The group class of the pair `(i, i + 1)`, if both are touches.
pub fn group_class(chart: &Chart, i: usize) -> Option<Class> {
    let (a, b) = (chart.notes.get(i)?, chart.notes.get(i + 1)?);
    let touchy = |n: &Note| matches!(n.body, NoteBody::Touch | NoteBody::Hold { .. });
    if !touchy(a) || !touchy(b) {
        return None;
    }
    let ovl = arcs_overlap(a.arc, b.arc);
    Some(match ((b.time - a.time).abs() < 1e-6, ovl) {
        (true, false) => Class::Chord,
        (true, true) => Class::ChordOverlap,
        (false, false) => Class::Pair,
        (false, true) => Class::PairOverlap,
    })
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
    /// Chain (E5) or touch (E6): its contact replaced by one held from `from_ms` to `to_ms` (relative to the note) on ring 0 of
    /// every lane of the arc — a finger landing early and staying, resting long before, or landing late.
    Press { from_ms: f64, to_ms: f64 },
    /// Group (E6): both notes' own input removed; one contact on ring 0 of the `cover` lanes at the *second*
    /// note's time + `ms`.
    GroupPress { cover: Cover, ms: f64 },
    /// Group (E6): both notes' own input removed; the first note's arc pressed (ring 0) at its time + `a_ms`, the
    /// second's (ring 1) at its time + `b_ms`. `None`: that note is not pressed.
    GroupSplit {
        a_ms: Option<f64>,
        b_ms: Option<f64>,
    },
}

/// A probe list in a spec file, expanded by [`ProbeSpec::expand`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProbeSpec {
    /// Shifts `from..=to` frames in `step` frames.
    Sweep {
        from: f64,
        to: f64,
        step: f64,
    },
    HoldGap {
        at_ms: f64,
        ms: Vec<f64>,
    },
    HoldRelease {
        ms: Vec<f64>,
    },
    SlideJump {
        lanes: Vec<f64>,
    },
    SnapMove {
        moves: Vec<[u8; 2]>,
    },
    /// Contacts `[from, to]` in frames relative to the note.
    Press {
        frames: Vec<[f64; 2]>,
    },
    /// One group press per `(cover, frames)`.
    GroupPress {
        presses: Vec<(Cover, f64)>,
    },
    /// Split presses `[a, b]` in frames (`null`: not pressed).
    GroupSplit {
        frames: Vec<[Option<f64>; 2]>,
    },
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
            Self::Press { frames } => frames
                .iter()
                .map(|&[from, to]| Probe::Press {
                    from_ms: from * FRAME_SECS * 1000.0,
                    to_ms: to * FRAME_SECS * 1000.0,
                })
                .collect(),
            Self::GroupPress { presses } => presses
                .iter()
                .map(|&(cover, f)| Probe::GroupPress {
                    cover,
                    ms: f * FRAME_SECS * 1000.0,
                })
                .collect(),
            Self::GroupSplit { frames } => frames
                .iter()
                .map(|&[a, b]| Probe::GroupSplit {
                    a_ms: a.map(|f| f * FRAME_SECS * 1000.0),
                    b_ms: b.map(|f| f * FRAME_SECS * 1000.0),
                })
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
    /// Group probes (E6): the second note and our verdict for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ours_partner: Option<Judgement>,
}

/// Whether a class is an E6 two-note group.
pub fn is_group(c: Class) -> bool {
    matches!(
        c,
        Class::Chord | Class::ChordOverlap | Class::Pair | Class::PairOverlap
    )
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

/// Pairs of notes `(i, j)`, `i < j`, starting at most `max_dt` apart (chords: 0) whose joint span keeps `gap` seconds
/// from every other note's span, at or after `min_time` (E6 chord probes, doc 10 §10).
pub fn isolated_pairs(chart: &Chart, gap: f64, min_time: f64, max_dt: f64) -> Vec<(usize, usize)> {
    let spans: Vec<(f64, f64)> = chart.notes.iter().map(span).collect();
    (0..spans.len().saturating_sub(1))
        .filter_map(|i| {
            let j = i + 1;
            let (s, e) = (spans[i].0, spans[i].1.max(spans[j].1));
            let ok = s >= min_time
                && spans[j].0 - spans[i].0 <= max_dt
                && spans
                    .iter()
                    .enumerate()
                    .all(|(k, &(s2, e2))| k == i || k == j || e2 < s - gap || s2 > e + gap);
            ok.then_some((i, j))
        })
        .collect()
}

/// Whether two arcs share a lane.
pub fn arcs_overlap(a: Arc, b: Arc) -> bool {
    arc_bits(a) & arc_bits(b) != 0
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
        (Probe::Press { from_ms, to_ms }, NoteBody::Chain | NoteBody::Touch) => to_ms > from_ms,
        (
            Probe::GroupPress { .. } | Probe::GroupSplit { .. },
            NoteBody::Touch | NoteBody::Hold { .. },
        ) => true,
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
    let groups: Vec<(usize, Class)> = isolated_pairs(chart, spec.isolation, spec.min_time, PAIR_DT)
        .into_iter()
        .filter_map(|(i, _)| Some((i, group_class(chart, i)?)))
        .collect();
    let mut out = Vec::new();
    for (class_, specs) in &spec.probes {
        let probes: Vec<Probe> = specs.iter().flat_map(ProbeSpec::expand).collect();
        if probes.is_empty() {
            continue;
        }
        let mut queue: std::collections::VecDeque<Probe> = Default::default();
        let notes: Vec<usize> = if is_group(*class_) {
            groups
                .iter()
                .filter(|g| g.1 == *class_)
                .map(|g| g.0)
                .collect()
        } else {
            candidates
                .iter()
                .copied()
                .filter(|&i| class(&chart.notes[i].body) == *class_)
                .collect()
        };
        for i in notes {
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

/// A group press on `lanes` (ring `depth`) at `t` for `on`; on the start lanes of a hold in `notes` it is held
/// until `held_to`, where the hold's perfect input resumes (the probes cut it up to there).
fn group_cells(
    lanes: &[u8],
    depth: usize,
    t: f64,
    on: f64,
    notes: [&Note; 2],
    held_to: f64,
) -> Vec<Contact> {
    let holds = notes
        .iter()
        .filter(|n| matches!(n.body, NoteBody::Hold { .. }))
        .fold(0, |m, n| m | arc_bits(n.arc));
    lanes
        .iter()
        .map(|&lane| {
            let end = if holds & lane_bit(lane) != 0 {
                held_to.max(t + on)
            } else {
                t + on
            };
            Contact {
                lane: lane % LANES,
                depth,
                start: t,
                end,
            }
        })
        .collect()
}

/// Lanes of the shortest arc covering both arcs.
fn union_arc(a: Arc, b: Arc) -> u64 {
    let n = u16::from(LANES);
    // From one arc's start round to the other's end; the shorter of the two ways.
    let cover = |x: Arc, y: Arc| {
        let len = (u16::from(y.lane) + u16::from(y.width) + n - u16::from(x.lane)) % n;
        let len = if len == 0 { n } else { len };
        let len = len.max(u16::from(x.width)).min(n);
        let mask = (0..len).fold(0u64, |m, o| {
            m | lane_bit(((u16::from(x.lane) + o) % n) as u8)
        });
        (len, mask)
    };
    let (la, ma) = cover(a, b);
    let (lb, mb) = cover(b, a);
    if la <= lb { ma } else { mb }
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
            (Probe::GroupPress { cover, ms }, _) => {
                let Some(m) = chart.notes.get(i + 1) else {
                    continue;
                };
                cs = cut(&cs, bits | note_bits(m), start - 0.2, m.time + 0.2);
                let lanes = match cover {
                    Cover::Union => union_arc(n.arc, m.arc),
                    Cover::First => arc_bits(n.arc),
                    Cover::Second => arc_bits(m.arc),
                    Cover::Overlap => arc_bits(n.arc) & arc_bits(m.arc),
                };
                let lanes: Vec<u8> = (0..LANES).filter(|&l| lanes & lane_bit(l) != 0).collect();
                let t = m.time + ms / 1000.0;
                cs.extend(group_cells(&lanes, 0, t, on, [n, m], m.time + 0.2));
            }
            (Probe::GroupSplit { a_ms, b_ms }, _) => {
                let Some(m) = chart.notes.get(i + 1) else {
                    continue;
                };
                cs = cut(&cs, bits | note_bits(m), start - 0.2, m.time + 0.2);
                for (note, off, depth) in [(n, a_ms, 0), (m, b_ms, 1)] {
                    if let Some(ms) = off {
                        let lanes: Vec<u8> =
                            (0..note.arc.width).map(|o| note.arc.lane + o).collect();
                        let t = note.time + ms / 1000.0;
                        cs.extend(group_cells(&lanes, depth, t, on, [n, m], m.time + 0.2));
                    }
                }
            }
            (Probe::Press { from_ms, to_ms }, _) => {
                let (from, to) = (start + from_ms / 1000.0, start + to_ms / 1000.0);
                cs = cut(&cs, bits, from.min(start - 0.2), to.max(start + 0.2));
                let lanes: Vec<u8> = (0..n.arc.width).map(|o| n.arc.lane + o).collect();
                cs.extend(cells(&lanes, &[0], from, to));
            }
            _ => {}
        }
    }
    cs.sort_by(|a, b| a.start.total_cmp(&b.start));
    cs
}

/// Our judge's verdict per note for a tap script, the clock running one second past the last edge.
pub fn predict(chart: &Chart, taps: &[Tap]) -> Vec<Option<Judgement>> {
    judge_frames(chart, timeline::expand(taps))
        .into_iter()
        .map(|e| e.map(|e| e.judgement))
        .collect()
}

/// Our judge's event per note for an edge list, the clock running one second past the last edge. Held states
/// are repeated every ¼ frame ([`crate::parity::densify`]): the game feeds the judge every frame, and chains and
/// hold bodies are only checked on the frames the judge sees.
pub fn judge_frames(chart: &Chart, frames: Vec<InputFrame>) -> Vec<Option<JudgeEvent>> {
    let mut frames = crate::parity::densify(&frames, FRAME_SECS / 4.0);
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
            *slot = Some(e);
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
            let group = matches!(probe, Probe::GroupPress { .. } | Probe::GroupSplit { .. });
            let partner = group.then_some(i + 1);
            let end = partner
                .and_then(|j| chart.notes.get(j))
                .map_or(end, |m| span(m).1.max(end));
            Test {
                note: i,
                time,
                end,
                class: if group {
                    group_class(chart, i).unwrap_or(Class::Chord)
                } else {
                    class(&n.body)
                },
                probe,
                ours: ours[i],
                partner,
                ours_partner: partner.and_then(|j| ours.get(j).copied().flatten()),
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
    let step = xs
        .windows(2)
        .map(|w| w[1] - w[0])
        .fold(f64::INFINITY, f64::min);
    let step = if step.is_finite() { step } else { 1.0 };
    let mut cuts = vec![xs[0] - step / 2.0];
    cuts.extend(xs.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    cuts.push(xs[xs.len() - 1] + step / 2.0);
    // Errors of [cuts[i], cuts[j]] = ok points outside + bad points inside.
    let count = |lo: f64, hi: f64| -> usize {
        points
            .iter()
            .filter(|p| p.1 != (p.0 > lo && p.0 < hi))
            .count()
    };
    let mut best = (usize::MAX, Vec::new());
    for i in 0..cuts.len() {
        for j in i..cuts.len() {
            let e = count(cuts[i], cuts[j]);
            if e < best.0 {
                best = (e, Vec::new());
            }
            if e == best.0 {
                best.1.push((cuts[i], cuts[j]));
            }
        }
    }
    let (err, pairs) = best;
    let mid = |f: fn(&(f64, f64)) -> f64| {
        let (min, max) = pairs
            .iter()
            .map(f)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                (a.min(x), b.max(x))
            });
        (min + max) / 2.0
    };
    Some((mid(|p| p.0), mid(|p| p.1), err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{BeatPos, Bonus, ChartHeader, HoldNode};

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
            control: true,
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
        let s = spec(vec![(
            Class::Slide,
            ProbeSpec::SlideJump { lanes: vec![4.0] },
        )]);
        assert_eq!(assign(&c, &s), [(1, Probe::SlideJump { lanes: 4.0 })]);
        assert!(feasible(&c.notes[0], &Probe::SlideJump { lanes: 2.0 }));
        assert!(!feasible(&c.notes[0], &Probe::SlideJump { lanes: 2.5 }));
        assert!(!feasible(
            &c.notes[0],
            &Probe::HoldGap {
                at_ms: 0.0,
                ms: 1.0
            }
        ));
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
        // Shifts of -8, -4, 0 or +4 frames: Miss, Great, Marvelous, Great (touch: 3/5/6 frames).
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
                -4 | 4 => Judgement::Great,
                _ => Judgement::Miss,
            };
            assert_eq!(t.ours, Some(want), "{p:?}");
        }
    }

    #[test]
    fn hold_gaps_and_slide_and_snap_probes_reach_our_judge() {
        let c = chart(vec![hold(2.0, 3.0, 10)]);
        let gap = |ms| {
            spec(vec![(
                Class::Hold,
                ProbeSpec::HoldGap {
                    at_ms: 300.0,
                    ms: vec![ms],
                },
            )])
        };
        assert_eq!(judged(&c, &gap(150.0)), [Some(Judgement::Marvelous)]);
        assert_eq!(judged(&c, &gap(300.0)), [Some(Judgement::Good)]);
        let release = |ms| spec(vec![(Class::Hold, ProbeSpec::HoldRelease { ms: vec![ms] })]);
        assert_eq!(judged(&c, &release(500.0)), [Some(Judgement::Good)]);

        let c = chart(vec![note(2.0, 58, 6, NoteBody::Slide(SlideDir::Cw))]);
        let jump = |lanes| {
            spec(vec![(
                Class::Slide,
                ProbeSpec::SlideJump { lanes: vec![lanes] },
            )])
        };
        assert_eq!(judged(&c, &jump(1.5)), [Some(Judgement::Miss)]);
        assert_eq!(judged(&c, &jump(2.0)), [Some(Judgement::Marvelous)]);

        let c = chart(vec![note(2.0, 20, 4, NoteBody::Snap(SnapDir::In))]);
        let mv = |from, to| {
            spec(vec![(
                Class::Snap,
                ProbeSpec::SnapMove {
                    moves: vec![[from, to]],
                },
            )])
        };
        assert_eq!(judged(&c, &mv(1, 0)), [Some(Judgement::Marvelous)]);
        assert_eq!(judged(&c, &mv(0, 3)), [Some(Judgement::Miss)]);
    }

    #[test]
    fn chain_presses_reach_our_judge() {
        let c = chart(vec![note(2.0, 20, 4, NoteBody::Chain)]);
        let press = |from, to| {
            spec(vec![(
                Class::Chain,
                ProbeSpec::Press {
                    frames: vec![[from, to]],
                },
            )])
        };
        // Resting long before and held through, lifted inside the window, lifted before it, too late.
        assert_eq!(judged(&c, &press(-18.0, 8.0)), [Some(Judgement::Marvelous)]);
        assert_eq!(
            judged(&c, &press(-10.0, -3.0)),
            [Some(Judgement::Marvelous)]
        );
        assert_eq!(judged(&c, &press(-10.0, -5.0)), [Some(Judgement::Miss)]);
        assert_eq!(judged(&c, &press(5.0, 15.0)), [Some(Judgement::Miss)]);
        assert!(!feasible(
            &note(2.0, 20, 4, NoteBody::Slide(SlideDir::Cw)),
            &Probe::Press {
                from_ms: 0.0,
                to_ms: 10.0
            }
        ));
    }

    #[test]
    fn group_probes_reach_our_judge() {
        // A side-by-side chord and an overlapping pair 9.5 f apart, isolated from each other.
        let c = chart(vec![
            note(2.0, 10, 4, NoteBody::Touch),
            note(2.0, 30, 4, NoteBody::Touch),
            note(4.0, 35, 10, NoteBody::Touch),
            note(4.0 + 9.5 * FRAME_SECS, 41, 10, NoteBody::Touch),
        ]);
        assert_eq!(isolated_pairs(&c, 0.35, 1.0, PAIR_DT), [(0, 1), (2, 3)]);
        assert_eq!(group_class(&c, 0), Some(Class::Chord));
        assert_eq!(group_class(&c, 2), Some(Class::PairOverlap));
        let probe = |class, p| spec(vec![(class, p)]);
        let split = probe(
            Class::Chord,
            ProbeSpec::GroupSplit {
                frames: vec![[Some(0.0), Some(4.0)]],
            },
        );
        let (_, tests) = build(&c, &split);
        assert_eq!(
            (tests[0].ours, tests[0].partner, tests[0].ours_partner),
            (Some(Judgement::Marvelous), Some(1), Some(Judgement::Great))
        );
        // One press over the pair 4.2 f before the second goes to the second only.
        let press = probe(
            Class::PairOverlap,
            ProbeSpec::GroupPress {
                presses: vec![(Cover::Union, -4.2)],
            },
        );
        let (_, tests) = build(&c, &press);
        assert_eq!(
            (tests[0].ours, tests[0].ours_partner),
            (Some(Judgement::Miss), Some(Judgement::Great))
        );
        assert_eq!(
            union_arc(Arc { lane: 58, width: 4 }, Arc { lane: 2, width: 2 }).count_ones(),
            6
        );
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
        assert_eq!(fit_interval(&stray), Some((-4.0, 4.5, 2)));
        // A soft edge (alternating samples) lands in the middle of its equally good cuts.
        let soft = [
            (0.0, false),
            (1.0, true),
            (2.0, false),
            (3.0, true),
            (4.0, true),
            (5.0, false),
        ];
        assert_eq!(fit_interval(&soft), Some((1.5, 4.5, 1)));
        assert_eq!(fit_interval(&[(1.0, false)]), None);
    }
}
