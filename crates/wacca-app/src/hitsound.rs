//! Which hit sound a judgement plays (`docs/research/03-audio-and-jackets.md` §5), and detection
//! of hold starts (the judge reports a hold only once, at its end, but its sound starts at the press).

use wacca_audio::{HitSounds, Sfx};
use wacca_chart::{Arc, Bonus, Chart, LANES, NoteBody};
use wacca_game::input::SensorState;
use wacca_game::judge::JudgeTable;
use wacca_game::rules::{FRAME_SECS, JudgeClass, JudgeWindows, Judgement};

/// Gain of a normal note's hit sound.
pub const GAIN: f32 = 0.7;
/// R notes (2x score) play the same sound louder.
// UNVERIFIED: the real game may use a different sound or effect for R notes.
pub const R_GAIN: f32 = 1.0;

/// One cue of the SE set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Marvelous,
    Good,
    SlideMarvelous,
    SlideGood,
    SnapMarvelous,
    SnapGood,
    HoldMarvelous,
    HoldGood,
    Chain,
}

impl Cue {
    pub fn sounds(self, h: &HitSounds) -> &[Sfx] {
        match self {
            Cue::Marvelous => &h.marvelous,
            Cue::Good => &h.good,
            Cue::SlideMarvelous => &h.slide_marvelous,
            Cue::SlideGood => &h.slide_good,
            Cue::SnapMarvelous => &h.snap_marvelous,
            Cue::SnapGood => &h.snap_good,
            Cue::HoldMarvelous => &h.hold_marvelous,
            Cue::HoldGood => &h.hold_good,
            Cue::Chain => &h.chain,
        }
    }
}

/// Sound for a judgement event. Misses are silent (UNVERIFIED, doc 03 §5: no miss SE field);
/// Great and Good share the "good" sound. Holds are `None`: their sound starts at the press
/// ([`HoldStarts`]) and their (end) event only stops the sustain.
pub fn cue_for(class: JudgeClass, j: Judgement) -> Option<Cue> {
    let marv = j == Judgement::Marvelous;
    let pick = |m, g| Some(if marv { m } else { g });
    match (class, j) {
        (_, Judgement::Miss) | (JudgeClass::Hold | JudgeClass::HoldEnd, _) => None,
        (JudgeClass::Touch, _) => pick(Cue::Marvelous, Cue::Good),
        (JudgeClass::Chain, _) => Some(Cue::Chain),
        (JudgeClass::SlideCw | JudgeClass::SlideCcw, _) => {
            pick(Cue::SlideMarvelous, Cue::SlideGood)
        }
        (JudgeClass::SnapIn | JudgeClass::SnapOut, _) => pick(Cue::SnapMarvelous, Cue::SnapGood),
    }
}

pub fn hold_cue(j: Judgement) -> Cue {
    if j == Judgement::Marvelous {
        Cue::HoldMarvelous
    } else {
        Cue::HoldGood
    }
}

pub fn gain(bonus: Bonus) -> f32 {
    if bonus == Bonus::R { R_GAIN } else { GAIN }
}

fn arc_touched(s: &SensorState, arc: Arc) -> bool {
    (0..arc.width)
        .any(|i| s.lane_touched(((u16::from(arc.lane) + u16::from(i)) % u16::from(LANES)) as u8))
}

/// Watches hold notes and reports the frame in which each start is touched inside its window.
// UNVERIFIED approximation of the judge's press rule: any touch of the start arc counts, not
// only a new press.
#[derive(Debug)]
pub struct HoldStarts {
    /// `(note index, start time, start arc)`, by time.
    holds: Vec<(usize, f64, Arc)>,
    done: Vec<bool>,
    /// Holds before this index are done.
    next: usize,
    windows: JudgeWindows,
}

impl HoldStarts {
    pub fn new(chart: &Chart) -> Self {
        let holds: Vec<_> = chart
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.body, NoteBody::Hold { .. }))
            .map(|(i, n)| (i, n.time, n.arc))
            .collect();
        Self {
            done: vec![false; holds.len()],
            holds,
            next: 0,
            windows: *JudgeTable::reverse().windows(JudgeClass::Hold),
        }
    }

    /// Holds whose start is touched at `time`: `(note index, judgement of the start)`.
    pub fn update(&mut self, time: f64, sensors: &SensorState) -> Vec<(usize, Judgement)> {
        let w = &self.windows;
        let late = w.late_limit();
        let early = w.good.early.max(w.great.early).max(w.marvelous.early) * FRAME_SECS;
        let mut out = Vec::new();
        for k in self.next..self.holds.len() {
            let (note, t, arc) = self.holds[k];
            let offset = time - t;
            if offset < -early {
                break;
            }
            if self.done[k] {
                continue;
            }
            if offset > late {
                self.done[k] = true;
            } else if let Some(j) = self.windows.judge(offset)
                && arc_touched(sensors, arc)
            {
                self.done[k] = true;
                out.push((note, j));
            }
        }
        while self.done.get(self.next) == Some(&true) {
            self.next += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::{BeatPos, ChartHeader, HoldNode, Note};

    #[test]
    fn cues_per_class_and_tier() {
        use JudgeClass::*;
        use Judgement::*;
        assert_eq!(cue_for(Touch, Marvelous), Some(Cue::Marvelous));
        assert_eq!(cue_for(Touch, Great), Some(Cue::Good));
        assert_eq!(cue_for(Touch, Good), Some(Cue::Good));
        assert_eq!(cue_for(Touch, Miss), None);
        assert_eq!(cue_for(SlideCcw, Marvelous), Some(Cue::SlideMarvelous));
        assert_eq!(cue_for(SnapOut, Great), Some(Cue::SnapGood));
        assert_eq!(cue_for(Chain, Marvelous), Some(Cue::Chain));
        assert_eq!(cue_for(Hold, Marvelous), None);
        assert_eq!(hold_cue(Marvelous), Cue::HoldMarvelous);
        assert_eq!(hold_cue(Good), Cue::HoldGood);
        assert!(gain(Bonus::R) > gain(Bonus::None));
        assert_eq!(gain(Bonus::Bonus), GAIN);
    }

    fn note(time: f64, lane: u8, body: NoteBody) -> Note {
        Note {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane, width: 4 },
            bonus: Bonus::None,
            body,
            file_index: 0,
        }
    }

    fn hold(time: f64, lane: u8) -> Note {
        let node = |t| HoldNode {
            pos: BeatPos::default(),
            time: t,
            arc: Arc { lane, width: 4 },
            lane_unwrapped: f32::from(lane),
            control: true,
        };
        note(
            time,
            lane,
            NoteBody::Hold {
                nodes: vec![node(time), node(time + 1.0)],
            },
        )
    }

    fn touching(lane: u8) -> SensorState {
        let mut s = SensorState::default();
        s.set(lane, 0, true);
        s
    }

    #[test]
    fn hold_start_detected_once_inside_window() {
        let chart = Chart {
            header: ChartHeader::default(),
            notes: vec![hold(1.0, 58), note(1.5, 0, NoteBody::Touch), hold(2.0, 10)],
            masks: vec![],
            tempo: vec![],
            scroll: vec![],
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 5.0,
        };
        let mut h = HoldStarts::new(&chart);
        // Too early, wrong lane, then a wrapped lane of 58..62 inside the window.
        assert!(h.update(0.5, &touching(0)).is_empty());
        assert!(h.update(1.0, &touching(30)).is_empty());
        assert_eq!(
            h.update(1.01, &touching(1)),
            vec![(0, Judgement::Marvelous)]
        );
        assert!(h.update(1.02, &touching(1)).is_empty());
        // The second hold is never touched inside its window.
        assert!(h.update(2.5, &touching(10)).is_empty());
        assert_eq!(h.next, 2);
    }
}
