//! `MusicParameterTable` → song catalog. Spec: docs/research/02-data-tables.md §7.

use wacca_game::Difficulty;
use wacca_game::catalog::{ChartEntry, Song};
use wacca_ue4::{DataTable, Value};

/// Per difficulty: column suffix for `Difficulty*Lv` / `ClearNormaRate*`, and its movie column.
const DIFFICULTY_COLUMNS: [(Difficulty, &str, &str); 4] = [
    (Difficulty::Normal, "Normal", "MovieAssetName"),
    (Difficulty::Hard, "Hard", "MovieAssetNameHard"),
    (Difficulty::Expert, "Extreme", "MovieAssetNameExpert"),
    (Difficulty::Inferno, "Inferno", "MovieAssetNameInferno"),
];

#[derive(Debug, thiserror::Error)]
#[error("field {0} is missing or has an unexpected type/value")]
pub struct FieldError(String);

struct Row<'a>(&'a Value);

impl<'a> Row<'a> {
    fn get<T>(&self, name: &str, f: impl FnOnce(&'a Value) -> Option<T>) -> Result<T, FieldError> {
        self.0
            .field(name)
            .and_then(f)
            .ok_or_else(|| FieldError(name.to_owned()))
    }
    fn str(&self, name: &str) -> Result<&'a str, FieldError> {
        self.get(name, Value::as_str)
    }
    fn int<T: TryFrom<i64>>(&self, name: &str) -> Result<T, FieldError> {
        self.get(name, |v| v.as_i64().and_then(|i| T::try_from(i).ok()))
    }
    fn float(&self, name: &str) -> Result<f32, FieldError> {
        self.get(name, Value::as_f32)
    }
}

/// `f32` → `f64` through its shortest decimal form (57.3, not 57.29999923706055).
fn widen(x: f32) -> f64 {
    x.to_string().parse().unwrap_or(f64::from(x))
}

/// UNVERIFIED (02-data-tables §7): a per-difficulty movie of `""` inherits the base movie, `"-"` means none.
fn movie(base: &str, column: &str) -> Option<String> {
    let name = if column.is_empty() { base } else { column };
    (!name.is_empty() && name != "-").then(|| name.to_owned())
}

pub fn chart_path(folder: &str, d: Difficulty) -> String {
    format!("MusicData/{folder}/{folder}_{:02}.mer", d as u8)
}

/// One table row → song; `Ok(None)` if it has no playable chart.
fn song_from_row(
    row: &Value,
    chart_exists: &impl Fn(&str) -> bool,
) -> Result<Option<Song>, FieldError> {
    let row = Row(row);
    let folder = row.str("AssetDirectory")?.to_owned();
    let base_movie = row.str("MovieAssetName")?;
    let mut charts = Vec::new();
    for (difficulty, suffix, movie_column) in DIFFICULTY_COLUMNS {
        let level = row.float(&format!("Difficulty{suffix}Lv"))?;
        let chart_path = chart_path(&folder, difficulty);
        if level <= 0.0 || !chart_exists(&chart_path) {
            continue;
        }
        charts.push(ChartEntry {
            difficulty,
            level,
            norma: row.float(&format!("ClearNormaRate{suffix}"))?,
            movie: movie(base_movie, row.str(movie_column)?),
            chart_path,
        });
    }
    if charts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Song {
        id: row.int("UniqueID")?,
        title: row.str("MusicMessage")?.to_owned(),
        artist: row.str("ArtistMessage")?.to_owned(),
        reading: row.str("Rubi")?.to_owned(),
        bpm: row.str("Bpm")?.to_owned(),
        genre: row.int("ScoreGenre")?,
        version: row.int("VersionNo")?,
        jacket: row.str("JacketAssetName")?.to_owned(),
        preview_begin: widen(row.float("PreviewBeginTime")?),
        preview_seconds: widen(row.float("PreviewSeconds")?),
        charts,
        folder,
    }))
}

/// Builds the catalog in table order. Rows without a playable chart (level > 0 and chart file
/// present) or with malformed fields are skipped with a log message.
pub fn songs_from_table(table: &DataTable, chart_exists: impl Fn(&str) -> bool) -> Vec<Song> {
    let mut songs = Vec::new();
    for (name, row) in &table.rows {
        match song_from_row(row, &chart_exists) {
            Ok(Some(song)) => songs.push(song),
            Ok(None) => log::warn!("song {name}: no playable charts, skipped"),
            Err(e) => log::warn!("song {name}: {e}, skipped"),
        }
    }
    songs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(folder: &str, levels: [f32; 4], movies: [&str; 4]) -> Value {
        let mut f = vec![
            ("UniqueID".to_owned(), Value::UInt(3030)),
            ("MusicMessage".into(), Value::Str("Title".into())),
            ("ArtistMessage".into(), Value::Str("Artist".into())),
            ("Rubi".into(), Value::Str("たいとる".into())),
            ("Bpm".into(), Value::Str("150".into())),
            ("ScoreGenre".into(), Value::Int(0)),
            ("VersionNo".into(), Value::UInt(5)),
            ("AssetDirectory".into(), Value::Str(folder.into())),
            (
                "JacketAssetName".into(),
                Value::Str("S03/uT_J_S03_30".into()),
            ),
            ("PreviewBeginTime".into(), Value::Float(57.3)),
            ("PreviewSeconds".into(), Value::Float(10.0)),
        ];
        for (i, (_, suffix, movie_column)) in DIFFICULTY_COLUMNS.iter().enumerate() {
            f.push((format!("Difficulty{suffix}Lv"), Value::Float(levels[i])));
            f.push((
                format!("ClearNormaRate{suffix}"),
                Value::Float([0.45, 0.55, 0.65, 0.8][i]),
            ));
            f.push((movie_column.to_string(), Value::Str(movies[i].into())));
        }
        Value::Struct(f)
    }

    fn table(rows: Vec<Value>) -> DataTable {
        DataTable {
            row_struct: "MusicParameterTableData".into(),
            rows: rows
                .into_iter()
                .enumerate()
                .map(|(i, r)| (i.to_string(), r))
                .collect(),
        }
    }

    #[test]
    fn maps_fields() {
        let t = table(vec![row(
            "S03-030",
            [3.0, 9.7, 12.6, 0.0],
            ["-", "", "", ""],
        )]);
        let songs = songs_from_table(&t, |_| true);
        assert_eq!(songs.len(), 1);
        let s = &songs[0];
        assert_eq!(
            (s.id, s.folder.as_str(), s.title.as_str()),
            (3030, "S03-030", "Title")
        );
        assert_eq!(
            (s.artist.as_str(), s.reading.as_str(), s.bpm.as_str()),
            ("Artist", "たいとる", "150")
        );
        assert_eq!(
            (s.genre, s.version, s.jacket.as_str()),
            (0, 5, "S03/uT_J_S03_30")
        );
        assert_eq!((s.preview_begin, s.preview_seconds), (57.3, 10.0));
        assert_eq!(s.charts.len(), 3, "level 0 Inferno is not a chart");
        let expert = s.chart(Difficulty::Expert).unwrap();
        assert_eq!(expert.level, 12.6);
        assert_eq!(expert.norma, 0.65);
        assert_eq!(expert.movie, None);
        assert_eq!(expert.chart_path, "MusicData/S03-030/S03-030_02.mer");
    }

    #[test]
    fn movie_override_rules() {
        assert_eq!(movie("mv_a", ""), Some("mv_a".into()));
        assert_eq!(movie("mv_a", "-"), None);
        assert_eq!(movie("-", ""), None);
        assert_eq!(movie("mv_a", "mv_b"), Some("mv_b".into()));
        assert_eq!(movie("", ""), None);
        let t = table(vec![row(
            "S03-011",
            [1.0, 2.0, 3.0, 4.0],
            ["mv_S03_11", "", "mv_x", "-"],
        )]);
        let movies: Vec<_> = songs_from_table(&t, |_| true)[0]
            .charts
            .iter()
            .map(|c| c.movie.clone())
            .collect();
        assert_eq!(
            movies,
            [
                Some("mv_S03_11".into()),
                Some("mv_S03_11".into()),
                Some("mv_x".into()),
                None
            ]
        );
    }

    #[test]
    fn missing_chart_files_are_dropped_and_empty_songs_skipped() {
        let t = table(vec![
            row("S01-001", [1.0, 5.0, 9.0, 13.0], ["-"; 4]),
            row("S01-002", [1.0, 5.0, 9.0, 0.0], ["-"; 4]),
        ]);
        let songs = songs_from_table(&t, |p| {
            p.starts_with("MusicData/S01-001/") && !p.ends_with("_01.mer")
        });
        assert_eq!(songs.len(), 1);
        let ds: Vec<_> = songs[0].charts.iter().map(|c| c.difficulty).collect();
        assert_eq!(
            ds,
            [Difficulty::Normal, Difficulty::Expert, Difficulty::Inferno]
        );
    }

    #[test]
    fn test_genre_and_malformed_rows() {
        let mut test_song = row("S99-001", [1.0, 2.0, 3.0, 0.0], ["-"; 4]);
        if let Value::Struct(f) = &mut test_song {
            f.retain(|(n, _)| n != "ScoreGenre");
            f.push(("ScoreGenre".into(), Value::Int(-1)));
        }
        let broken = Value::Struct(vec![(
            "AssetDirectory".into(),
            Value::Str("S00-001".into()),
        )]);
        let songs = songs_from_table(&table(vec![test_song, broken]), |_| true);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].genre, -1);
    }
}
