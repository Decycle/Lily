//! Shared rule vocabulary. Values come from the game's DataTables (see `docs/research/tables/`).

use serde::{Deserialize, Serialize};

/// The game judges in 60 fps frames; window values below are in frames.
pub const FRAME_SECS: f64 = 1.0 / 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Judgement {
    Marvelous,
    Great,
    Good,
    Miss,
}

/// Which row of `JudgeParameterTable` applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JudgeClass {
    Touch,
    Chain,
    Hold,
    HoldEnd,
    SlideCw,
    SlideCcw,
    SnapIn,
    SnapOut,
}

/// Early/late extent of one judgement tier, in frames (`early` = before the note).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub early: f64,
    pub late: f64,
}

impl Window {
    /// `offset` = hit time - note time, in seconds.
    pub fn contains(self, offset: f64) -> bool {
        offset >= -self.early * FRAME_SECS && offset <= self.late * FRAME_SECS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeWindows {
    pub marvelous: Window,
    pub great: Window,
    pub good: Window,
}

impl JudgeWindows {
    /// Best tier containing `offset` (seconds), or `None` if outside the Good window.
    pub fn judge(&self, offset: f64) -> Option<Judgement> {
        [
            (self.marvelous, Judgement::Marvelous),
            (self.great, Judgement::Great),
            (self.good, Judgement::Good),
        ]
        .into_iter()
        .find(|(w, _)| w.contains(offset))
        .map(|(_, j)| j)
    }

    /// Latest time after the note at which it can still be hit (seconds).
    pub fn late_limit(&self) -> f64 {
        self.good.late.max(self.great.late).max(self.marvelous.late) * FRAME_SECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(m: f64, g: f64, gd: f64) -> JudgeWindows {
        let s = |x| Window { early: x, late: x };
        JudgeWindows {
            marvelous: s(m),
            great: s(g),
            good: s(gd),
        }
    }

    #[test]
    fn judge_tiers() {
        let t = w(3.0, 5.0, 6.0);
        assert_eq!(t.judge(0.0), Some(Judgement::Marvelous));
        assert_eq!(t.judge(3.0 * FRAME_SECS), Some(Judgement::Marvelous));
        assert_eq!(t.judge(-4.0 * FRAME_SECS), Some(Judgement::Great));
        assert_eq!(t.judge(5.5 * FRAME_SECS), Some(Judgement::Good));
        assert_eq!(t.judge(6.5 * FRAME_SECS), None);
        assert!((t.late_limit() - 0.1).abs() < 1e-12);
    }
}
