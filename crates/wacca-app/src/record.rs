//! Song lookup in `data/songs.json` and merging a result into the save file.

use std::path::Path;

use wacca_chart::Difficulty;
use wacca_game::catalog::Song;
use wacca_game::grade::PlayResult;
use wacca_game::save::{self, SaveError};

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Save(#[from] SaveError),
}

/// Song catalog at `path`; `Ok(None)` if the file does not exist.
pub fn load_songs(path: &Path) -> Result<Option<Vec<Song>>, RecordError> {
    let shown = || path.display().to_string();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(RecordError::Io {
                path: shown(),
                source,
            });
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| RecordError::Json {
            path: shown(),
            source,
        })
}

/// Song whose `MusicData` folder is `folder` (e.g. `S00-003`).
pub fn find_song<'a>(songs: &'a [Song], folder: &str) -> Option<&'a Song> {
    songs.iter().find(|s| s.folder.eq_ignore_ascii_case(folder))
}

/// Clear line for `d` when the catalog has none.
// UNVERIFIED: typical MusicParameterTable values (docs/PLAN.md §2); real charts carry their own.
pub fn default_norma(d: Difficulty) -> f32 {
    match d {
        Difficulty::Normal => 0.45,
        Difficulty::Hard => 0.55,
        Difficulty::Expert | Difficulty::Inferno => 0.80,
    }
}

/// Load the save at `path`, merge `result`, write it back. Returns `true` on a new best score.
pub fn save_result(
    path: &Path,
    song_id: u32,
    d: Difficulty,
    result: &PlayResult,
) -> Result<bool, RecordError> {
    let mut data = save::load(path)?;
    let best = data.merge(song_id, d, result);
    save::save(path, &data)?;
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u32, folder: &str) -> Song {
        Song {
            id,
            folder: folder.into(),
            title: String::new(),
            artist: String::new(),
            reading: String::new(),
            bpm: String::new(),
            genre: 0,
            version: 1,
            jacket: String::new(),
            preview_begin: 0.0,
            preview_seconds: 0.0,
            charts: vec![],
        }
    }

    #[test]
    fn finds_song_by_folder() {
        let songs = [song(1003, "S00-003"), song(3011, "S03-011")];
        assert_eq!(find_song(&songs, "S03-011").map(|s| s.id), Some(3011));
        assert_eq!(find_song(&songs, "s00-003").map(|s| s.id), Some(1003));
        assert!(find_song(&songs, "S09-999").is_none());
    }

    #[test]
    fn missing_catalog_is_none_and_bad_json_is_error() {
        let dir = std::env::temp_dir().join(format!("wacca-app-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load_songs(&dir.join("nope.json")).unwrap().is_none());
        let bad = dir.join("bad.json");
        std::fs::write(&bad, b"{").unwrap();
        assert!(matches!(load_songs(&bad), Err(RecordError::Json { .. })));
        let good = dir.join("songs.json");
        std::fs::write(&good, serde_json::to_vec(&[song(5, "S00-005")]).unwrap()).unwrap();
        assert_eq!(load_songs(&good).unwrap().unwrap()[0].id, 5);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_round_trip() {
        use wacca_game::grade::{Grade, Lamp};
        use wacca_game::score::Counts;
        let dir = std::env::temp_dir().join(format!("wacca-app-save-{}", std::process::id()));
        let path = dir.join("save.json");
        let result = PlayResult {
            score: 950_000,
            grade: Grade::SS,
            lamp: Lamp::Clear,
            counts: Counts::default(),
            max_combo: 10,
            total_notes: 10,
            gauge: 0.9,
        };
        assert!(save_result(&path, 1003, Difficulty::Expert, &result).unwrap());
        assert!(!save_result(&path, 1003, Difficulty::Expert, &result).unwrap());
        let data = save::load(&path).unwrap();
        let rec = data.record(1003, Difficulty::Expert).unwrap();
        assert_eq!((rec.best_score, rec.play_count), (950_000, 2));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
