//! Per-class judgement windows (`JudgeParameterTable`) and note → class mapping.

use crate::rules::{JudgeClass, JudgeWindows, Window};
use wacca_chart::{Note, NoteBody, SlideDir, SnapDir};

/// Windows for every `JudgeClass`, in 60 fps frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JudgeTable {
    pub touch: JudgeWindows,
    pub chain: JudgeWindows,
    pub hold: JudgeWindows,
    pub hold_end: JudgeWindows,
    pub slide_cw: JudgeWindows,
    pub slide_ccw: JudgeWindows,
    pub snap_in: JudgeWindows,
    pub snap_out: JudgeWindows,
}

/// `(front, rear)` pairs for Marvelous / Great / Good.
const fn row(m: (f64, f64), g: (f64, f64), gd: (f64, f64)) -> JudgeWindows {
    JudgeWindows {
        marvelous: Window {
            early: m.0,
            late: m.1,
        },
        great: Window {
            early: g.0,
            late: g.1,
        },
        good: Window {
            early: gd.0,
            late: gd.1,
        },
    }
}

impl JudgeTable {
    /// `JudgeParameterTable` of WACCA Reverse 3.07.01 (`docs/research/02-data-tables.md` §2).
    /// `AdjustRateFront/Rear` (Chain front = 20 %) is not applied.
    // UNVERIFIED: meaning of AdjustRate; Front = early / Rear = late.
    pub fn reverse() -> Self {
        let slide = row((5.0, 7.0), (8.0, 10.0), (10.0, 10.0));
        Self {
            touch: row((3.0, 3.0), (5.0, 5.0), (6.0, 6.0)),
            chain: row((4.0, 4.0), (4.0, 4.0), (4.0, 4.0)),
            hold: row((3.0, 3.0), (5.0, 5.0), (6.0, 6.0)),
            hold_end: row((7.0, 7.0), (7.0, 7.0), (7.0, 7.0)),
            // UNVERIFIED: which of SlideLeft/SlideRight is clockwise (rows are identical).
            slide_cw: slide,
            slide_ccw: slide,
            snap_in: row((5.0, 7.0), (8.0, 10.0), (10.0, 10.0)),
            snap_out: row((7.0, 5.0), (10.0, 8.0), (10.0, 10.0)),
        }
    }

    pub fn windows(&self, class: JudgeClass) -> &JudgeWindows {
        match class {
            JudgeClass::Touch => &self.touch,
            JudgeClass::Chain => &self.chain,
            JudgeClass::Hold => &self.hold,
            JudgeClass::HoldEnd => &self.hold_end,
            JudgeClass::SlideCw => &self.slide_cw,
            JudgeClass::SlideCcw => &self.slide_ccw,
            JudgeClass::SnapIn => &self.snap_in,
            JudgeClass::SnapOut => &self.snap_out,
        }
    }
}

/// The table row that judges `note` (a hold is judged by its start).
pub fn class_of(note: &Note) -> JudgeClass {
    match note.body {
        NoteBody::Touch => JudgeClass::Touch,
        NoteBody::Chain => JudgeClass::Chain,
        NoteBody::Hold { .. } => JudgeClass::Hold,
        NoteBody::Slide(SlideDir::Cw) => JudgeClass::SlideCw,
        NoteBody::Slide(SlideDir::Ccw) => JudgeClass::SlideCcw,
        NoteBody::Snap(SnapDir::In) => JudgeClass::SnapIn,
        NoteBody::Snap(SnapDir::Out) => JudgeClass::SnapOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::FRAME_SECS;

    #[test]
    fn reverse_values() {
        let t = JudgeTable::reverse();
        assert_eq!(t.windows(JudgeClass::Touch).good.late, 6.0);
        assert_eq!(t.windows(JudgeClass::SnapOut).marvelous.early, 7.0);
        assert_eq!(t.windows(JudgeClass::SnapOut).great.late, 8.0);
        assert_eq!(t.windows(JudgeClass::SlideCw).marvelous.late, 7.0);
        assert_eq!(t.windows(JudgeClass::HoldEnd).good.early, 7.0);
        assert!((t.windows(JudgeClass::SnapIn).early_limit() - 10.0 * FRAME_SECS).abs() < 1e-12);
    }
}
