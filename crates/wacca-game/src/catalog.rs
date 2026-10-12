//! Song catalog (`data/songs.json`), produced by `wacca-convert` from `MusicParameterTable`.

use serde::{Deserialize, Serialize};
use wacca_chart::Difficulty;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Song {
    /// `UniqueID`, e.g. 3030.
    pub id: u32,
    /// `AssetDirectory`, e.g. `S03-030` (folder under `MusicData/`).
    pub folder: String,
    pub title: String,
    pub artist: String,
    /// Reading (kana) for sorting.
    pub reading: String,
    /// Display BPM text, e.g. `150` or `120-180`.
    pub bpm: String,
    /// `ScoreGenre`; -1 marks test songs.
    pub genre: i8,
    pub version: u8,
    /// Jacket texture asset, e.g. `S03/uT_J_S03_30`.
    pub jacket: String,
    pub preview_begin: f64,
    pub preview_seconds: f64,
    /// Only difficulties that exist (level > 0 and chart file present).
    pub charts: Vec<ChartEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartEntry {
    pub difficulty: Difficulty,
    /// Decimal level, e.g. 12.6.
    pub level: f32,
    /// Clear gauge target (0..1).
    pub norma: f32,
    /// Background movie asset name, if any.
    pub movie: Option<String>,
    /// Chart path relative to the assets root, e.g. `MusicData/S03-030/S03-030_02.mer`.
    pub chart_path: String,
}

impl Song {
    pub fn chart(&self, d: Difficulty) -> Option<&ChartEntry> {
        self.charts.iter().find(|c| c.difficulty == d)
    }
}
