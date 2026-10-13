//! Live play tally: judgement counts, fast/late, combo, score and clear (norma) gauge.
//!
//! Spec: `docs/research/04-gameplay-mechanics.md` §5–§7, tables `GameScoreTable` and `NormaTable`.

use serde::{Deserialize, Serialize};
use wacca_chart::Bonus;

use crate::grade::{Grade, Lamp, PlayResult};
use crate::rules::{JudgeClass, JudgeEvent, Judgement};

/// `GameScoreTable.MaxScore`.
pub const MAX_SCORE: u32 = 1_000_000;

/// `GameScoreTable.JudgeScoreRate*` (1.0 / 0.7 / 0.5 / 0.0) in tenths, so scoring stays integer.
fn rate_tenths(j: Judgement) -> u64 {
    match j {
        Judgement::Marvelous => 10,
        Judgement::Great => 7,
        Judgement::Good => 5,
        Judgement::Miss => 0,
    }
}

/// Score weight of one judged note: R notes count double.
fn weight(bonus: Bonus) -> u64 {
    if bonus == Bonus::R { 2 } else { 1 }
}

/// Row of `NormaTable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NormaRow {
    Normal,
    NormalBonus,
    Hold,
    Chain,
    Slide,
    SlideBonus,
    Flick,
}

impl NormaRow {
    /// `bonus` = the note is a bonus note and the Bonus effect option is on.
    fn of(class: JudgeClass, bonus: bool) -> Self {
        match class {
            JudgeClass::Touch if bonus => Self::NormalBonus,
            JudgeClass::Touch => Self::Normal,
            JudgeClass::Chain => Self::Chain,
            JudgeClass::Hold | JudgeClass::HoldEnd => Self::Hold,
            JudgeClass::SlideCw | JudgeClass::SlideCcw if bonus => Self::SlideBonus,
            JudgeClass::SlideCw | JudgeClass::SlideCcw => Self::Slide,
            JudgeClass::SnapIn | JudgeClass::SnapOut => Self::Flick,
        }
    }

    /// `NormaGauge{SuperJust,Just,Good,Miss}`.
    fn points(self, j: Judgement) -> i64 {
        let [m, gr, gd, miss] = match self {
            Self::Normal | Self::Hold | Self::Slide | Self::Flick => [10, 7, 4, -5],
            Self::NormalBonus | Self::SlideBonus => [15, 10, 7, -5],
            Self::Chain => [2, 1, 1, -5],
        };
        match j {
            Judgement::Marvelous => m,
            Judgement::Great => gr,
            Judgement::Good => gd,
            Judgement::Miss => miss,
        }
    }
}

/// Judgement counters shown on the results screen and stored with best scores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub marvelous: u32,
    pub great: u32,
    pub good: u32,
    pub miss: u32,
    pub fast: u32,
    pub late: u32,
}

impl Counts {
    /// Count one judgement (not FAST/LATE).
    fn add(&mut self, j: Judgement) {
        match j {
            Judgement::Marvelous => self.marvelous += 1,
            Judgement::Great => self.great += 1,
            Judgement::Good => self.good += 1,
            Judgement::Miss => self.miss += 1,
        }
    }

    /// Notes judged better than Miss.
    pub fn hits(&self) -> u32 {
        self.marvelous + self.great + self.good
    }

    pub fn judged(&self) -> u32 {
        self.hits() + self.miss
    }
}

/// Accumulates [`JudgeEvent`]s for one play.
#[derive(Clone, Debug)]
pub struct PlayTally {
    total_notes: u32,
    total_weight: u64,
    gauge_max: i64,
    bonus_effect: bool,
    counts: Counts,
    /// R notes of the chart and their judgements (results "R-Notes" column).
    r_total: u32,
    r_counts: Counts,
    /// Σ weight × rate (tenths).
    points: u64,
    /// Σ weight of judged notes.
    judged_weight: u64,
    combo: u32,
    max_combo: u32,
    gauge: i64,
}

impl PlayTally {
    /// `notes`: one entry per judged note of the chart (holds count once).
    /// `bonus_effect`: the Bonus effect option; when off, bonus notes use the normal gauge rows.
    pub fn new(notes: &[(JudgeClass, Bonus)], bonus_effect: bool) -> Self {
        let total_weight = notes.iter().map(|&(_, b)| weight(b)).sum();
        // UNVERIFIED: NormaPointMax = Σ Marvelous points of each note's non-bonus row, so an all-Marvelous
        // play fills the gauge exactly and bonus notes (option on) overfill it.
        let gauge_max = notes
            .iter()
            .map(|&(c, _)| NormaRow::of(c, false).points(Judgement::Marvelous))
            .sum();
        Self {
            total_notes: u32::try_from(notes.len()).unwrap_or(u32::MAX),
            total_weight,
            gauge_max,
            bonus_effect,
            counts: Counts::default(),
            r_total: u32::try_from(notes.iter().filter(|&&(_, b)| b == Bonus::R).count())
                .unwrap_or(u32::MAX),
            r_counts: Counts::default(),
            points: 0,
            judged_weight: 0,
            combo: 0,
            max_combo: 0,
            gauge: 0,
        }
    }

    pub fn apply(&mut self, e: &JudgeEvent) {
        let w = weight(e.bonus);
        self.points += w * rate_tenths(e.judgement);
        self.judged_weight += w;

        let c = &mut self.counts;
        c.add(e.judgement);
        if e.bonus == Bonus::R {
            self.r_counts.add(e.judgement);
        }
        // FAST/LATE is shown for non-Marvelous hits only; chains are excluded (COMMUNITY).
        if !matches!(e.judgement, Judgement::Marvelous | Judgement::Miss)
            && e.class != JudgeClass::Chain
        {
            match e.offset {
                Some(o) if o < 0.0 => c.fast += 1,
                Some(o) if o > 0.0 => c.late += 1,
                _ => {}
            }
        }

        if e.judgement == Judgement::Miss {
            self.combo = 0;
        } else {
            self.combo += 1;
            self.max_combo = self.max_combo.max(self.combo);
        }

        let bonus = self.bonus_effect && e.bonus == Bonus::Bonus;
        // UNVERIFIED: the gauge is clamped to [0, max] after every judgement.
        self.gauge = (self.gauge + NormaRow::of(e.class, bonus).points(e.judgement))
            .clamp(0, self.gauge_max);
    }

    fn scaled(&self, points: u64) -> u32 {
        if self.total_weight == 0 {
            return 0;
        }
        // UNVERIFIED: floor of the final ratio (Saturn); the game might accumulate per-note floats.
        let s = u128::from(points) * u128::from(MAX_SCORE) / (10 * u128::from(self.total_weight));
        u32::try_from(s).unwrap_or(MAX_SCORE).min(MAX_SCORE)
    }

    /// Current score ("plus" display): judged notes so far over the whole chart.
    pub fn score(&self) -> u32 {
        self.scaled(self.points)
    }

    /// Best score still reachable ("minus" display): unjudged notes assumed Marvelous.
    pub fn max_attainable(&self) -> u32 {
        let remaining = self.total_weight.saturating_sub(self.judged_weight);
        self.scaled(self.points + 10 * remaining)
    }

    /// Clear gauge fill, 0..=1.
    pub fn gauge(&self) -> f64 {
        if self.gauge_max == 0 {
            0.0
        } else {
            self.gauge as f64 / self.gauge_max as f64
        }
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn max_combo(&self) -> u32 {
        self.max_combo
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// Final result. `norma` = the chart's clear line (0..1). Notes never judged (give-up) count as
    /// misses and make the play failed.
    pub fn finish(&self, norma: f32) -> PlayResult {
        let mut counts = self.counts;
        let judged = counts.judged();
        if judged > self.total_notes {
            log::warn!("{judged} judgements for {} notes", self.total_notes);
        }
        let unjudged = self.total_notes.saturating_sub(judged);
        counts.miss += unjudged;
        let score = self.score();
        let gauge = self.gauge();
        // Compared in f32 so a gauge exactly on the line (e.g. 0.55) clears against an f32 norma.
        let lamp = Lamp::of(unjudged == 0, gauge as f32 >= norma, &counts);
        let mut r_counts = self.r_counts;
        r_counts.miss += self.r_total.saturating_sub(r_counts.judged());
        PlayResult {
            score,
            grade: Grade::of(score, counts.hits()),
            lamp,
            counts,
            r_counts,
            max_combo: self.max_combo,
            total_notes: self.total_notes,
            gauge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(
        class: JudgeClass,
        bonus: Bonus,
        judgement: Judgement,
        offset: Option<f64>,
    ) -> JudgeEvent {
        JudgeEvent {
            note: 0,
            class,
            bonus,
            judgement,
            offset,
            time: 0.0,
        }
    }

    fn touch(j: Judgement) -> JudgeEvent {
        ev(JudgeClass::Touch, Bonus::None, j, Some(0.0))
    }

    fn tally(n: usize) -> PlayTally {
        PlayTally::new(&vec![(JudgeClass::Touch, Bonus::None); n], true)
    }

    #[test]
    fn all_marvelous_is_exact_max() {
        // 3 notes: 1e6/3 does not divide evenly, the full total must still be exact.
        let mut t = PlayTally::new(
            &[
                (JudgeClass::Touch, Bonus::None),
                (JudgeClass::Chain, Bonus::R),
                (JudgeClass::Hold, Bonus::None),
            ],
            true,
        );
        assert_eq!(t.max_attainable(), MAX_SCORE);
        t.apply(&touch(Judgement::Marvelous));
        t.apply(&ev(JudgeClass::Chain, Bonus::R, Judgement::Marvelous, None));
        t.apply(&ev(
            JudgeClass::Hold,
            Bonus::None,
            Judgement::Marvelous,
            Some(0.0),
        ));
        assert_eq!(t.score(), MAX_SCORE);
        let r = t.finish(0.8);
        assert_eq!(r.lamp, Lamp::AllMarvelous);
        assert_eq!(r.grade, Grade::Master);
        assert_eq!(r.max_combo, 3);
    }

    #[test]
    fn rates_and_flooring() {
        let mut t = tally(3);
        t.apply(&touch(Judgement::Great));
        // 0.7 / 3 = 233_333.33 → floor
        assert_eq!(t.score(), 233_333);
        assert_eq!(t.max_attainable(), 900_000);
        t.apply(&touch(Judgement::Good));
        t.apply(&touch(Judgement::Miss));
        assert_eq!(t.score(), 400_000);
        assert_eq!(t.max_attainable(), 400_000);
    }

    #[test]
    fn r_notes_weigh_double() {
        let mut t = PlayTally::new(
            &[
                (JudgeClass::Touch, Bonus::R),
                (JudgeClass::Touch, Bonus::None),
            ],
            true,
        );
        t.apply(&ev(
            JudgeClass::Touch,
            Bonus::R,
            Judgement::Marvelous,
            Some(0.0),
        ));
        t.apply(&touch(Judgement::Miss));
        assert_eq!(t.score(), 666_666);
    }

    #[test]
    fn r_notes_are_counted_separately() {
        let r = (JudgeClass::Touch, Bonus::R);
        let mut t = PlayTally::new(&[r, r, r, (JudgeClass::Touch, Bonus::None)], true);
        t.apply(&ev(r.0, r.1, Judgement::Great, Some(0.01)));
        t.apply(&touch(Judgement::Marvelous));
        let res = t.finish(0.0);
        // one R note judged Great, the two never judged (give-up) count as misses
        assert_eq!((res.r_counts.great, res.r_counts.miss), (1, 2));
        assert_eq!(res.r_counts.marvelous, 0);
        assert_eq!(res.counts.marvelous, 1);
    }

    #[test]
    fn one_miss_is_not_master() {
        let mut t = tally(1_000);
        for _ in 0..999 {
            t.apply(&touch(Judgement::Marvelous));
        }
        t.apply(&touch(Judgement::Great));
        assert_eq!(t.score(), 999_700);
        assert_eq!(t.finish(0.8).grade, Grade::SssPlus);
    }

    #[test]
    fn combo_and_fast_late() {
        let mut t = tally(6);
        t.apply(&touch(Judgement::Marvelous));
        t.apply(&ev(
            JudgeClass::Touch,
            Bonus::None,
            Judgement::Great,
            Some(-0.07),
        ));
        t.apply(&ev(
            JudgeClass::Touch,
            Bonus::None,
            Judgement::Good,
            Some(0.09),
        ));
        t.apply(&ev(
            JudgeClass::Touch,
            Bonus::None,
            Judgement::Marvelous,
            Some(-0.01),
        ));
        assert_eq!(t.combo(), 4);
        t.apply(&ev(JudgeClass::Touch, Bonus::None, Judgement::Miss, None));
        assert_eq!(t.combo(), 0);
        t.apply(&ev(
            JudgeClass::Chain,
            Bonus::None,
            Judgement::Great,
            Some(0.05),
        ));
        let c = t.counts();
        assert_eq!((c.marvelous, c.great, c.good, c.miss), (2, 2, 1, 1));
        assert_eq!((c.fast, c.late), (1, 1));
        assert_eq!((t.combo(), t.max_combo()), (1, 4));
    }

    #[test]
    fn gauge_rows_and_clamp() {
        let notes = [
            (JudgeClass::Touch, Bonus::Bonus),
            (JudgeClass::Chain, Bonus::None),
            (JudgeClass::SnapIn, Bonus::None),
        ];
        let mut t = PlayTally::new(&notes, true);
        // max = 10 + 2 + 10
        t.apply(&ev(JudgeClass::Chain, Bonus::None, Judgement::Miss, None));
        assert_eq!(t.gauge(), 0.0);
        t.apply(&ev(
            JudgeClass::Touch,
            Bonus::Bonus,
            Judgement::Marvelous,
            Some(0.0),
        ));
        assert!((t.gauge() - 15.0 / 22.0).abs() < 1e-12);
        t.apply(&ev(
            JudgeClass::SnapIn,
            Bonus::None,
            Judgement::Marvelous,
            Some(0.0),
        ));
        assert_eq!(t.gauge(), 1.0);

        let mut off = PlayTally::new(&notes, false);
        off.apply(&ev(
            JudgeClass::Touch,
            Bonus::Bonus,
            Judgement::Marvelous,
            Some(0.0),
        ));
        assert!((off.gauge() - 10.0 / 22.0).abs() < 1e-12);
    }

    #[test]
    fn clear_and_fail_by_gauge() {
        // 10 notes, 7 Marvelous + 3 Miss → (70 − 15) / 100 = 0.55
        let mut t = tally(10);
        for _ in 0..7 {
            t.apply(&touch(Judgement::Marvelous));
        }
        for _ in 0..3 {
            t.apply(&touch(Judgement::Miss));
        }
        assert!((t.gauge() - 0.55).abs() < 1e-12);
        assert_eq!(t.finish(0.55).lamp, Lamp::Missless);
        assert_eq!(t.finish(0.8).lamp, Lamp::Failed);
    }

    #[test]
    fn give_up_counts_unjudged_as_miss_and_fails() {
        let mut t = tally(4);
        t.apply(&touch(Judgement::Marvelous));
        let r = t.finish(0.0);
        assert_eq!(r.counts.miss, 3);
        assert_eq!(r.lamp, Lamp::Failed);
        assert_eq!(r.score, 250_000);
    }

    #[test]
    fn empty_chart() {
        let t = tally(0);
        assert_eq!((t.score(), t.max_attainable(), t.gauge()), (0, 0, 0.0));
    }
}
