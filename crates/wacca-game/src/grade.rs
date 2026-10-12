//! Final play result: grade (`ClearRateTable`) and clear lamp.
//!
//! Spec: `docs/research/02-data-tables.md` §4, `05-save-and-song-select.md` §1.2–§1.3.

use serde::{Deserialize, Serialize};

use crate::score::Counts;

/// Score rank, ordered worst → best.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Grade {
    D,
    C,
    B,
    A,
    AA,
    AAA,
    S,
    #[serde(rename = "S+")]
    SPlus,
    SS,
    #[serde(rename = "SS+")]
    SsPlus,
    SSS,
    #[serde(rename = "SSS+")]
    SssPlus,
    #[serde(rename = "MASTER")]
    Master,
}

/// `ClearRateTable`: (grade, RequiredScore, RequiredHitCount), best first.
const CLEAR_RATE: [(Grade, u32, u32); 13] = [
    (Grade::Master, 1_000_000, 0),
    (Grade::SssPlus, 990_000, 0),
    (Grade::SSS, 980_000, 0),
    (Grade::SsPlus, 970_000, 0),
    (Grade::SS, 950_000, 0),
    (Grade::SPlus, 930_000, 0),
    (Grade::S, 900_000, 0),
    (Grade::AAA, 850_000, 0),
    (Grade::AA, 800_000, 0),
    (Grade::A, 700_000, 0),
    (Grade::B, 0, 10),
    (Grade::C, 0, 1),
    (Grade::D, 0, 0),
];

impl Grade {
    /// Best row whose score and hit-count requirements are both met.
    /// UNVERIFIED: "hit count" = notes judged better than Miss (B ≥ 10, C ≥ 1).
    pub fn of(score: u32, hits: u32) -> Self {
        CLEAR_RATE
            .iter()
            .find(|&&(_, s, h)| score >= s && hits >= h)
            .map_or(Grade::D, |&(g, _, _)| g)
    }
}

/// `RhythmGameRuleTable.FlawlesslyMissCount`.
pub const MISSLESS_MAX_MISSES: u32 = 5;

/// Clear lamp, ordered worst → best.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Lamp {
    #[default]
    Failed,
    Clear,
    Missless,
    FullCombo,
    AllMarvelous,
}

impl Lamp {
    /// `completed`: every note was judged (no give-up). `gauge_cleared`: gauge ≥ norma at the end.
    pub fn of(completed: bool, gauge_cleared: bool, c: &Counts) -> Self {
        if !completed {
            return Self::Failed;
        }
        // UNVERIFIED: a play with 0 misses always counts as cleared (doc 05 recommendation);
        // Missless needs the gauge like a plain clear.
        match c.miss {
            0 if c.great == 0 && c.good == 0 => Self::AllMarvelous,
            0 => Self::FullCombo,
            _ if !gauge_cleared => Self::Failed,
            m if m <= MISSLESS_MAX_MISSES => Self::Missless,
            _ => Self::Clear,
        }
    }
}

/// Outcome of one play, for the results screen and the save.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayResult {
    pub score: u32,
    pub grade: Grade,
    pub lamp: Lamp,
    pub counts: Counts,
    pub max_combo: u32,
    pub total_notes: u32,
    /// Final gauge fill, 0..=1.
    pub gauge: f64,
}

impl PlayResult {
    pub fn cleared(&self) -> bool {
        self.lamp >= Lamp::Clear
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn grade_boundaries() {
        let cases = [
            (1_000_000, Grade::Master),
            (999_999, Grade::SssPlus),
            (990_000, Grade::SssPlus),
            (989_999, Grade::SSS),
            (980_000, Grade::SSS),
            (970_000, Grade::SsPlus),
            (969_999, Grade::SS),
            (950_000, Grade::SS),
            (930_000, Grade::SPlus),
            (900_000, Grade::S),
            (899_999, Grade::AAA),
            (850_000, Grade::AAA),
            (800_000, Grade::AA),
            (700_000, Grade::A),
        ];
        for (score, g) in cases {
            assert_eq!(Grade::of(score, 100), g, "{score}");
        }
    }

    #[test]
    fn low_grades_by_hit_count() {
        assert_eq!(Grade::of(699_999, 10), Grade::B);
        assert_eq!(Grade::of(699_999, 9), Grade::C);
        assert_eq!(Grade::of(0, 1), Grade::C);
        assert_eq!(Grade::of(0, 0), Grade::D);
    }

    fn counts(great: u32, miss: u32) -> Counts {
        Counts {
            marvelous: 100,
            great,
            miss,
            ..Counts::default()
        }
    }

    #[test]
    fn lamps() {
        assert_eq!(Lamp::of(true, false, &counts(0, 0)), Lamp::AllMarvelous);
        assert_eq!(Lamp::of(true, false, &counts(1, 0)), Lamp::FullCombo);
        assert_eq!(Lamp::of(true, true, &counts(0, 1)), Lamp::Missless);
        assert_eq!(Lamp::of(true, true, &counts(0, 5)), Lamp::Missless);
        assert_eq!(Lamp::of(true, true, &counts(0, 6)), Lamp::Clear);
        assert_eq!(Lamp::of(true, false, &counts(0, 1)), Lamp::Failed);
        assert_eq!(Lamp::of(false, true, &counts(0, 0)), Lamp::Failed);
    }

    #[test]
    fn grade_serde_names() {
        let s = serde_json::to_string(&[Grade::SssPlus, Grade::Master, Grade::AA]).unwrap();
        assert_eq!(s, r#"["SSS+","MASTER","AA"]"#);
        assert_eq!(
            serde_json::to_string(&Lamp::FullCombo).unwrap(),
            r#""fullcombo""#
        );
    }
}
