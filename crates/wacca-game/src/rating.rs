//! Player rating ("Rate"). Spec: `docs/research/02-data-tables.md` §6, `05-save-and-song-select.md` §1.4.
//!
//! Rates are derived data: recompute them from the save's best scores, never persist them.

use crate::catalog::Song;
use crate::save::SaveData;

/// `MusicParameterTable.Version` of Reverse songs: the "new" songs of the rating.
/// UNVERIFIED (doc 05 §1.4); the table's own rule is a release date ≥ 2021081007.
pub const NEW_VERSION: u8 = 5;

/// `UserRateCoefficientTable`: (RequiredScore, CalcRateCoefficient), best first.
const COEFFICIENTS: [(u32, f64); 19] = [
    (990_000, 4.0),
    (980_000, 3.75),
    (970_000, 3.5),
    (960_000, 3.25),
    (950_000, 3.0),
    (940_000, 2.75),
    (920_000, 2.5),
    (900_000, 2.0),
    (850_000, 1.5),
    (800_000, 1.0),
    (700_000, 0.8),
    (600_000, 0.7),
    (500_000, 0.6),
    (400_000, 0.5),
    (300_000, 0.4),
    (200_000, 0.3),
    (100_000, 0.2),
    (1, 0.1),
    (0, 0.0),
];

/// `UserRateTargetTable.CountMusicNum` for new (Reverse) and old songs.
pub const NEW_CHARTS: usize = 15;
pub const OLD_CHARTS: usize = 35;

pub fn coefficient(score: u32) -> f64 {
    COEFFICIENTS
        .iter()
        .find(|&&(s, _)| score >= s)
        .map_or(0.0, |&(_, c)| c)
}

/// `level` = decimal chart constant (e.g. 13.2), rounded to one decimal to undo f32 noise.
/// UNVERIFIED: no rounding of the product (the game might floor to tenths like ARTEMiS).
pub fn chart_rate(level: f32, score: u32) -> f64 {
    (f64::from(level) * 10.0).round() / 10.0 * coefficient(score)
}

/// One chart's rate and whether its song counts as "new".
///
/// The caller decides `is_new`: the table says songs dated ≥ 2021081007 (Reverse launch); doc 05
/// recommends `VersionNo == 5` (UNVERIFIED). Each chart counts separately (UNVERIFIED).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RatedChart {
    pub rate: f64,
    pub is_new: bool,
}

/// Sum of the best [`NEW_CHARTS`] new-song rates and the best [`OLD_CHARTS`] old-song rates.
pub fn player_rating(charts: &[RatedChart]) -> f64 {
    let best = |is_new: bool, n: usize| {
        let mut rates: Vec<f64> = charts
            .iter()
            .filter(|c| c.is_new == is_new)
            .map(|c| c.rate)
            .collect();
        rates.sort_by(|a, b| b.total_cmp(a));
        rates.iter().take(n).sum::<f64>()
    };
    best(true, NEW_CHARTS) + best(false, OLD_CHARTS)
}

/// Player rating from the save's best scores of every played chart in `songs`.
pub fn save_rating(save: &SaveData, songs: &[Song]) -> f64 {
    let charts: Vec<RatedChart> = songs
        .iter()
        .flat_map(|s| {
            s.charts.iter().filter_map(move |c| {
                let r = save.record(s.id, c.difficulty)?;
                (r.play_count > 0).then(|| RatedChart {
                    rate: chart_rate(c.level, r.best_score),
                    is_new: s.version == NEW_VERSION,
                })
            })
        })
        .collect();
    player_rating(&charts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ChartEntry;
    use crate::grade::{Grade, Lamp, PlayResult};
    use crate::score::Counts;
    use wacca_chart::Difficulty;

    #[test]
    fn coefficient_boundaries() {
        let cases = [
            (1_000_000, 4.0),
            (990_000, 4.0),
            (989_999, 3.75),
            (960_000, 3.25),
            (940_000, 2.75),
            (939_999, 2.5),
            (920_000, 2.5),
            (919_999, 2.0),
            (850_000, 1.5),
            (849_999, 1.0),
            (700_000, 0.8),
            (100_000, 0.2),
            (99_999, 0.1),
            (1, 0.1),
            (0, 0.0),
        ];
        for (s, c) in cases {
            assert_eq!(coefficient(s), c, "{s}");
        }
    }

    #[test]
    fn chart_rate_uses_decimal_level() {
        assert_eq!(chart_rate(13.2, 990_000), 52.8);
        assert_eq!(chart_rate(12.7, 0), 0.0);
    }

    #[test]
    fn best_15_new_and_35_old() {
        let mut charts: Vec<RatedChart> = (1..=20)
            .map(|i| RatedChart {
                rate: f64::from(i),
                is_new: true,
            })
            .collect();
        charts.extend((1..=40).map(|i| RatedChart {
            rate: f64::from(i) * 10.0,
            is_new: false,
        }));
        // new: 6..=20 → 195; old: 10 × (6..=40) = 8050
        assert_eq!(player_rating(&charts), 195.0 + 8050.0);
        assert_eq!(player_rating(&[]), 0.0);
    }

    fn song(id: u32, version: u8, levels: &[(Difficulty, f32)]) -> Song {
        Song {
            id,
            folder: String::new(),
            title: String::new(),
            artist: String::new(),
            reading: String::new(),
            bpm: String::new(),
            genre: 0,
            version,
            jacket: String::new(),
            preview_begin: 0.0,
            preview_seconds: 0.0,
            charts: levels
                .iter()
                .map(|&(difficulty, level)| ChartEntry {
                    difficulty,
                    level,
                    norma: 0.5,
                    movie: None,
                    chart_path: String::new(),
                })
                .collect(),
        }
    }

    fn play(save: &mut SaveData, id: u32, d: Difficulty, score: u32) {
        let r = PlayResult {
            score,
            grade: Grade::of(score, 1),
            lamp: Lamp::Clear,
            counts: Counts::default(),
            r_counts: Counts::default(),
            max_combo: 0,
            total_notes: 1,
            gauge: 1.0,
        };
        save.merge(id, d, &r);
    }

    #[test]
    fn save_rating_sums_played_charts() {
        use Difficulty::{Expert, Hard, Normal};
        let songs = [
            song(1, 5, &[(Normal, 1.0), (Expert, 12.5)]),
            song(2, 4, &[(Hard, 9.0)]),
        ];
        let mut save = SaveData::default();
        assert_eq!(save_rating(&save, &songs), 0.0);
        play(&mut save, 1, Normal, 1_000_000); // new: 1.0 × 4
        play(&mut save, 1, Expert, 950_000); // new: 12.5 × 3
        play(&mut save, 2, Hard, 800_000); // old: 9 × 1
        play(&mut save, 9, Hard, 1_000_000); // not in the catalog
        assert_eq!(save_rating(&save, &songs), 4.0 + 37.5 + 9.0);
    }
}
