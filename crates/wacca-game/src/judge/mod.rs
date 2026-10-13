//! Judge engine: turns time-ordered sensor snapshots into one `JudgeEvent` per chart note.
//!
//! Spec: `docs/research/04-gameplay-mechanics.md` §3–4; windows, hold rules, slide threshold and
//! snap motion are measured on the real game in `docs/research/10-e3-judge-results.md`. Rules
//! still taken from the Saturn reimplementation without a measurement are marked `UNVERIFIED`.
//! Judging is continuous-time with windows of `frames × FRAME_SECS` (doc 10 §0).

mod autoplay;
mod sense;
mod table;
#[cfg(test)]
mod tests;

pub use autoplay::autoplay_frames;
pub use table::{JudgeTable, class_of};

use crate::input::{InputFrame, SensorState};
use crate::rules::{FRAME_SECS, JudgeClass, JudgeEvent, JudgeWindows, Judgement};
use wacca_chart::{Arc, Chart, Note, NoteBody};

/// A hold released for longer than this (seconds) is dropped (downgraded to Good, or Miss when
/// its start was missed). Measured between 167 and 183 ms (doc 10 §0, §6.1).
pub const HOLD_DROP_LENIENCY: f64 = 0.175;

/// Absorbs f64 rounding so the table's inclusive frame edges behave exactly.
const EPS: f64 = 1e-9;

#[derive(Clone, Debug, PartialEq)]
enum State {
    Open,
    /// Chain touched inside its window; awarded once the clock reaches the note.
    ChainTouched,
    /// Lowest slide position seen per tracked sub-arc.
    Slide(Vec<Option<f64>>),
    Hold(HoldState),
    Done,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HoldState {
    /// Start judgement and its offset, once the start was pressed.
    start: Option<(Judgement, f64)>,
    touched: bool,
    released_at: Option<f64>,
    dropped: bool,
}

#[derive(Clone, Debug)]
struct Slot {
    class: JudgeClass,
    /// Window extents in seconds (positive), after overlap cutting.
    early: f64,
    late: f64,
    state: State,
}

#[derive(Debug)]
pub struct Judge {
    notes: Vec<Note>,
    slots: Vec<Slot>,
    table: JudgeTable,
    prev: SensorState,
    /// All notes before this index are done.
    first_open: usize,
    /// Largest early window of any class (seconds): notes further ahead are not yet open.
    lookahead: f64,
    now: f64,
}

impl Judge {
    /// `chart.notes` must be sorted by time (the parser guarantees this).
    pub fn new(chart: &Chart, table: JudgeTable) -> Self {
        let notes = chart.notes.clone();
        if notes.windows(2).any(|w| w[1].time < w[0].time) {
            log::warn!("judge: chart notes are not sorted by time; notes may be judged late");
        }
        let mut slots: Vec<Slot> = notes
            .iter()
            .map(|n| {
                let class = class_of(n);
                let w = table.windows(class);
                let state = match &n.body {
                    NoteBody::Slide(_) => State::Slide(vec![None; sense::slide_spans(n.arc).len()]),
                    NoteBody::Hold { .. } => State::Hold(HoldState::default()),
                    _ => State::Open,
                };
                Slot {
                    class,
                    early: w.early_limit(),
                    late: w.late_limit(),
                    state,
                }
            })
            .collect();
        cut_overlaps(&notes, &mut slots);
        cut_at_hold_ends(&notes, &mut slots, &table);
        let lookahead = slots.iter().map(|s| s.early).fold(0.0, f64::max);
        Self {
            notes,
            slots,
            table,
            prev: SensorState::default(),
            first_open: 0,
            lookahead,
            now: 0.0,
        }
    }

    /// Feed the next sensor snapshot (time non-decreasing); returns the judgements it caused.
    pub fn update(&mut self, frame: &InputFrame) -> Vec<JudgeEvent> {
        let t = frame.time;
        let cur = frame.sensors;
        let open_end = self
            .notes
            .partition_point(|n| n.time - self.lookahead <= t + EPS);
        let mut events = Vec::new();
        let mut pressed = sense::new_presses(&self.prev, &cur);
        while pressed != 0 && self.press(&mut pressed, t, open_end, &mut events) {}
        for i in self.first_open..open_end {
            self.step(i, &cur, t, &mut events);
        }
        while self
            .slots
            .get(self.first_open)
            .is_some_and(|s| s.state == State::Done)
        {
            self.first_open += 1;
        }
        self.prev = cur;
        self.now = t;
        events
    }

    /// Miss every note not yet judged (song ended or abandoned).
    pub fn finish(&mut self) -> Vec<JudgeEvent> {
        let mut events = Vec::new();
        for i in self.first_open..self.slots.len() {
            if self.slots[i].state != State::Done {
                self.emit(i, Judgement::Miss, None, self.now, &mut events);
            }
        }
        self.first_open = self.slots.len();
        events
    }

    /// Newly pressed lanes hit the earliest open touch / hold start they overlap, together with
    /// every other such note at the same time (stacked chords). The hit notes' lanes are then
    /// used up, so notes stacked at different times need separate presses while side-by-side
    /// notes can share one. Returns whether a note was hit.
    // UNVERIFIED: `ProtectInvolveRockFrameNum = -1` may mean one press can hit several notes.
    fn press(
        &mut self,
        pressed: &mut u64,
        t: f64,
        open_end: usize,
        events: &mut Vec<JudgeEvent>,
    ) -> bool {
        let lanes = *pressed;
        let hittable = |i: usize| {
            let s = &self.slots[i];
            let waiting = match &s.state {
                State::Open => s.class == JudgeClass::Touch,
                State::Hold(h) => h.start.is_none(),
                _ => false,
            };
            waiting
                && sense::arc_mask(self.notes[i].arc) & lanes != 0
                && in_window(s, t - self.notes[i].time)
        };
        let Some(first) = (self.first_open..open_end).find(|&i| hittable(i)) else {
            return false;
        };
        let chord: Vec<usize> = (first..open_end)
            .filter(|&i| (self.notes[i].time - self.notes[first].time).abs() < EPS && hittable(i))
            .collect();
        for i in chord {
            *pressed &= !sense::arc_mask(self.notes[i].arc);
            let offset = t - self.notes[i].time;
            let judgement = grade(self.table.windows(self.slots[i].class), offset);
            if let State::Hold(h) = &mut self.slots[i].state {
                h.start = Some((judgement, offset));
                h.touched = true;
                h.released_at = None;
            } else {
                self.emit(i, judgement, Some(offset), t, events);
            }
        }
        true
    }

    fn step(&mut self, i: usize, cur: &SensorState, t: f64, events: &mut Vec<JudgeEvent>) {
        if let Some((judgement, offset)) = self.decide(i, cur, t) {
            self.emit(i, judgement, offset, t, events);
        }
    }

    /// Advance note `i` by one frame; returns its judgement once it is decided.
    fn decide(&mut self, i: usize, cur: &SensorState, t: f64) -> Option<(Judgement, Option<f64>)> {
        let note = &self.notes[i];
        let offset = t - note.time;
        let windows = self.table.windows(self.slots[i].class);
        // Hold bodies are only checked until `end − HoldEnd` (7 f; doc 10 §6.2).
        let hold_tail = self.table.windows(JudgeClass::HoldEnd).early_limit();
        let slot = &mut self.slots[i];
        let open = in_window(slot, offset);
        let closed = offset > slot.late + EPS;
        let miss = closed.then_some((Judgement::Miss, None));
        match (&mut slot.state, &note.body) {
            (State::Done, _) => None,
            (State::Hold(h), NoteBody::Hold { nodes }) => {
                let end = nodes.last().map_or(note.time, |n| n.time);
                let checked_until = end - hold_tail;
                // Release / re-press in the last 7 f is free; an earlier release counts as a gap
                // up to `checked_until` (doc 10 §6.2).
                if (h.start.is_some() || offset >= -EPS) && t <= checked_until + EPS {
                    track_hold(h, sense::arc_touched(cur, hold_arc_at(note, t)), t);
                }
                if t < end - EPS {
                    return None;
                }
                if h.released_at
                    .is_some_and(|r| checked_until - r > HOLD_DROP_LENIENCY)
                {
                    h.dropped = true;
                }
                // Doc 10 §6.3: a missed start that is also dropped is a Miss.
                Some(match (h.touched, h.start, h.dropped) {
                    (false, _, _) | (true, None, true) => (Judgement::Miss, None),
                    (true, Some((j, o)), false) => (j, Some(o)),
                    (true, start, _) => (Judgement::Good, start.map(|(_, o)| o)),
                })
            }
            (state @ (State::Open | State::ChainTouched), NoteBody::Chain) => {
                if open && sense::arc_touched(cur, note.arc) {
                    *state = State::ChainTouched;
                }
                if *state == State::ChainTouched && offset >= -EPS {
                    // Chains carry no timing (excluded from FAST/SLOW).
                    Some((Judgement::Marvelous, Some(0.0)))
                } else {
                    miss
                }
            }
            (State::Open, NoteBody::Snap(dir)) => {
                if open && sense::snapped(&self.prev, cur, note.arc, *dir) {
                    Some((grade(windows, offset), Some(offset)))
                } else {
                    miss
                }
            }
            (State::Slide(mins), NoteBody::Slide(dir)) => {
                let mut hit = false;
                if open {
                    for (span, min) in sense::slide_spans(note.arc).into_iter().zip(mins) {
                        if let Some(pos) = sense::slide_position(cur, span, *dir) {
                            let m = min.map_or(pos, |m: f64| m.min(pos));
                            *min = Some(m);
                            hit |= sense::slid(m, pos);
                        }
                    }
                }
                if hit {
                    Some((grade(windows, offset), Some(offset)))
                } else {
                    miss
                }
            }
            _ => miss,
        }
    }

    fn emit(
        &mut self,
        i: usize,
        judgement: Judgement,
        offset: Option<f64>,
        t: f64,
        events: &mut Vec<JudgeEvent>,
    ) {
        let slot = &mut self.slots[i];
        slot.state = State::Done;
        log::trace!("judge: note {i} {:?} {judgement:?} {offset:?}", slot.class);
        events.push(JudgeEvent {
            note: i,
            class: slot.class,
            bonus: self.notes[i].bonus,
            judgement,
            offset,
            time: t,
        });
    }
}

/// Best tier for `offset`; callers have already checked it lies inside the (cut) window.
fn grade(w: &JudgeWindows, offset: f64) -> Judgement {
    // Nudge toward zero so exact frame edges stay inside (windows are inclusive).
    w.judge(offset - EPS.copysign(offset))
        .unwrap_or(Judgement::Good)
}

fn in_window(slot: &Slot, offset: f64) -> bool {
    offset >= -slot.early - EPS && offset <= slot.late + EPS
}

/// Release bookkeeping for a hold body at time `t`.
fn track_hold(h: &mut HoldState, touching: bool, t: f64) {
    if touching {
        if h.released_at.is_some_and(|r| t - r > HOLD_DROP_LENIENCY) {
            h.dropped = true;
        }
        h.released_at = None;
        h.touched = true;
    } else if h.released_at.is_none() {
        h.released_at = Some(t);
    }
}

/// Span of the hold segment active at `t` (the latest node at or before `t`).
// UNVERIFIED: Saturn tests the active segment's span, not an interpolated one.
fn hold_arc_at(note: &Note, t: f64) -> Arc {
    match &note.body {
        NoteBody::Hold { nodes } => nodes
            .iter()
            .rev()
            .find(|n| n.time <= t + EPS)
            .or(nodes.first())
            .map_or(note.arc, |n| n.arc),
        _ => note.arc,
    }
}

/// Where an earlier note's late window overlaps a later note's early window on shared lanes,
/// both are cut at the midpoint between the two notes.
// UNVERIFIED: Saturn `ChartLoader` / MercuryMapper `CutOverlappingJudgementWindows`.
fn cut_overlaps(notes: &[Note], slots: &mut [Slot]) {
    let reach = slots.iter().map(|s| s.early).fold(0.0, f64::max);
    for i in 0..notes.len() {
        for j in i + 1..notes.len() {
            let (a, b) = (&notes[i], &notes[j]);
            if b.time - a.time > slots[i].late + reach {
                break;
            }
            let overlapping = b.time - a.time > EPS
                && a.time + slots[i].late > b.time - slots[j].early
                && sense::arcs_overlap(a.arc, b.arc);
            if overlapping {
                let half = (b.time - a.time) / 2.0;
                slots[i].late = slots[i].late.min(half);
                slots[j].early = slots[j].early.min(half);
            }
        }
    }
}

/// A note at the same time as an overlapping hold end keeps only its Marvelous early window.
// UNVERIFIED: Saturn / MercuryMapper `CutEarlyJudgementWindowOnHolds`.
fn cut_at_hold_ends(notes: &[Note], slots: &mut [Slot], table: &JudgeTable) {
    for (h, hold) in notes.iter().enumerate() {
        let NoteBody::Hold { nodes } = &hold.body else {
            continue;
        };
        let Some(end) = nodes.last() else {
            log::warn!("judge: hold note {h} has no nodes");
            continue;
        };
        for (i, note) in notes.iter().enumerate() {
            if i != h
                && (note.time - end.time).abs() < EPS
                && sense::arcs_overlap(note.arc, end.arc)
            {
                let marvelous = table.windows(slots[i].class).marvelous.early * FRAME_SECS;
                slots[i].early = slots[i].early.min(marvelous);
            }
        }
    }
}
