//! Save file: personal bests per chart and settings. Spec: `docs/research/05-save-and-song-select.md` §2.
//!
//! Versioned JSON, unknown fields kept, atomic write (tmp → sync → `.bak` → rename).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wacca_chart::Difficulty;

use crate::grade::{Grade, Lamp, PlayResult};
use crate::score::Counts;

pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("serialize save: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> SaveError + '_ {
    move |source| SaveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SaveData {
    pub version: u32,
    pub settings: Settings,
    /// Keyed by [`chart_key`].
    pub records: BTreeMap<String, ChartRecord>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            version: SAVE_VERSION,
            settings: Settings::default(),
            records: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

/// Play options. Minimal for now; unknown keys survive in `extra`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Hi-speed, 1.0–6.0.
    pub note_speed: f32,
    /// In-game judgement adjustment, −10.0..+10.0 (the app's options module owns the unit).
    pub judge_offset: f32,
    pub mirror: bool,
    pub bonus_effect: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            note_speed: 4.0,
            judge_offset: 0.0,
            mirror: false,
            bonus_effect: true,
            extra: Map::new(),
        }
    }
}

/// Personal bests of one chart. Each best is merged independently.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChartRecord {
    pub best_score: u32,
    /// Judgements of the best-score play.
    pub best_score_judgements: Counts,
    pub best_lamp: Lamp,
    pub best_grade: Option<Grade>,
    pub best_combo: u32,
    pub lowest_miss: Option<u32>,
    pub play_count: u32,
    pub clear_count: u32,
    pub missless_count: u32,
    pub full_combo_count: u32,
    pub all_marvelous_count: u32,
    pub grade_counts: BTreeMap<Grade, u32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ChartRecord {
    /// Merge one finished play (incl. give-ups). Returns `true` on a new best score.
    /// A play counts toward every lamp counter at or below its lamp (an FC is also missless).
    pub fn merge(&mut self, r: &PlayResult) -> bool {
        let new_best = self.play_count == 0 || r.score > self.best_score;
        if new_best {
            self.best_score = r.score;
            self.best_score_judgements = r.counts;
        }
        self.best_lamp = self.best_lamp.max(r.lamp);
        self.best_grade = self.best_grade.max(Some(r.grade));
        self.best_combo = self.best_combo.max(r.max_combo);
        self.lowest_miss = Some(
            self.lowest_miss
                .map_or(r.counts.miss, |m| m.min(r.counts.miss)),
        );
        self.play_count += 1;
        for (lamp, n) in [
            (Lamp::Clear, &mut self.clear_count),
            (Lamp::Missless, &mut self.missless_count),
            (Lamp::FullCombo, &mut self.full_combo_count),
            (Lamp::AllMarvelous, &mut self.all_marvelous_count),
        ] {
            if r.lamp >= lamp {
                *n += 1;
            }
        }
        *self.grade_counts.entry(r.grade).or_default() += 1;
        new_best
    }
}

/// Record key: `"<songId>:<difficulty>"`, e.g. `3023:expert`.
pub fn chart_key(song_id: u32, d: Difficulty) -> String {
    let d = match d {
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
        Difficulty::Inferno => "inferno",
    };
    format!("{song_id}:{d}")
}

impl SaveData {
    pub fn record(&self, song_id: u32, d: Difficulty) -> Option<&ChartRecord> {
        self.records.get(&chart_key(song_id, d))
    }

    /// Merge a play into its chart's record. Returns `true` on a new best score.
    pub fn merge(&mut self, song_id: u32, d: Difficulty, r: &PlayResult) -> bool {
        self.records
            .entry(chart_key(song_id, d))
            .or_default()
            .merge(r)
    }
}

/// `<appdata>\wacca-slim\save.json`.
pub fn save_path_in(appdata: &Path) -> PathBuf {
    appdata.join("wacca-slim").join("save.json")
}

/// Default save path from `%APPDATA%`, if set.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| save_path_in(Path::new(&a)))
}

/// `path` with `suffix` appended to the file name (`save.json` → `save.json.bak`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

pub fn bak_path(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

/// Read and parse one file. `Ok(None)` if missing; an unparseable file is renamed to
/// `<name>.corrupt-<unix ms>` (never overwritten) and also yields `Ok(None)`.
fn read_file(path: &Path) -> Result<Option<SaveData>, SaveError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(path)(e)),
    };
    match serde_json::from_slice::<SaveData>(&bytes) {
        Ok(save) => {
            if save.version > SAVE_VERSION {
                log::warn!(
                    "{}: save version {} is newer than {SAVE_VERSION}",
                    path.display(),
                    save.version
                );
            }
            Ok(Some(save))
        }
        Err(e) => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let aside = sibling(path, &format!(".corrupt-{ms}"));
            log::warn!(
                "{}: unreadable save ({e}); moved to {}",
                path.display(),
                aside.display()
            );
            fs::rename(path, &aside).map_err(io_err(path))?;
            Ok(None)
        }
    }
}

/// Load `path`, falling back to its `.bak`, then to a fresh save.
pub fn load(path: &Path) -> Result<SaveData, SaveError> {
    if let Some(s) = read_file(path)? {
        return Ok(s);
    }
    let bak = bak_path(path);
    if let Some(s) = read_file(&bak)? {
        log::warn!("{}: loaded backup {}", path.display(), bak.display());
        return Ok(s);
    }
    Ok(SaveData::default())
}

/// Atomic write: `<path>.tmp` + sync, current file → `<path>.bak`, tmp → `path`.
pub fn save(path: &Path, data: &SaveData) -> Result<(), SaveError> {
    let json = serde_json::to_vec_pretty(data)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(io_err(dir))?;
    }
    let tmp = sibling(path, ".tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(io_err(&tmp))?;
        f.write_all(&json).map_err(io_err(&tmp))?;
        f.sync_all().map_err(io_err(&tmp))?;
    }
    if path.exists() {
        fs::rename(path, bak_path(path)).map_err(io_err(path))?;
    }
    fs::rename(&tmp, path).map_err(io_err(path))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Fresh empty directory under the system temp dir.
    fn temp_dir(name: &str) -> PathBuf {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("wacca-save-{name}-{}-{ns}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn result(score: u32, lamp: Lamp, miss: u32, max_combo: u32) -> PlayResult {
        let counts = Counts {
            marvelous: 90,
            great: 10 - miss.min(10),
            miss,
            ..Counts::default()
        };
        PlayResult {
            score,
            grade: Grade::of(score, counts.hits()),
            lamp,
            counts,
            max_combo,
            total_notes: 100,
            gauge: 0.9,
        }
    }

    #[test]
    fn merge_keeps_bests_independently() {
        let mut s = SaveData::default();
        assert!(s.merge(
            3023,
            Difficulty::Expert,
            &result(950_000, Lamp::Clear, 7, 40)
        ));
        assert!(!s.merge(
            3023,
            Difficulty::Expert,
            &result(900_000, Lamp::FullCombo, 0, 100)
        ));
        assert!(!s.merge(3023, Difficulty::Expert, &result(0, Lamp::Failed, 100, 0)));
        let r = s.record(3023, Difficulty::Expert).unwrap();
        assert_eq!(r.best_score, 950_000);
        assert_eq!(r.best_score_judgements.miss, 7);
        assert_eq!(r.best_lamp, Lamp::FullCombo);
        assert_eq!(r.best_grade, Some(Grade::SS));
        assert_eq!(r.best_combo, 100);
        assert_eq!(r.lowest_miss, Some(0));
        assert_eq!(
            (
                r.play_count,
                r.clear_count,
                r.missless_count,
                r.full_combo_count,
                r.all_marvelous_count
            ),
            (3, 2, 1, 1, 0)
        );
        assert_eq!(r.grade_counts.get(&Grade::S), Some(&1));
        assert!(s.record(3023, Difficulty::Hard).is_none());
    }

    #[test]
    fn first_play_sets_best_even_at_zero() {
        let mut r = ChartRecord::default();
        assert!(r.merge(&result(0, Lamp::Failed, 100, 0)));
        assert_eq!(r.lowest_miss, Some(100));
        assert_eq!((r.best_score, r.best_score_judgements.miss), (0, 100));
    }

    #[test]
    fn keys_and_paths() {
        assert_eq!(chart_key(3023, Difficulty::Expert), "3023:expert");
        assert_eq!(chart_key(1, Difficulty::Inferno), "1:inferno");
        let p = save_path_in(Path::new("C:/AppData"));
        assert_eq!(
            p,
            Path::new("C:/AppData").join("wacca-slim").join("save.json")
        );
        assert_eq!(bak_path(&p).file_name().unwrap(), "save.json.bak");
    }

    #[test]
    fn round_trip_with_unknown_fields() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("sub").join("save.json");
        let mut s = SaveData::default();
        s.merge(
            3023,
            Difficulty::Expert,
            &result(987_654, Lamp::Missless, 1, 640),
        );
        s.settings.note_speed = 5.5;
        s.settings
            .extra
            .insert("keyBeam".into(), Value::Bool(false));
        s.extra.insert("history".into(), Value::Array(vec![]));
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);
        assert!(!sibling(&path, ".tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_gives_fresh_save() {
        let dir = temp_dir("missing");
        assert_eq!(load(&dir.join("save.json")).unwrap(), SaveData::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn second_save_keeps_backup() {
        let dir = temp_dir("bak");
        let path = dir.join("save.json");
        let mut s = SaveData::default();
        save(&path, &s).unwrap();
        assert!(!bak_path(&path).exists());
        s.settings.mirror = true;
        save(&path, &s).unwrap();
        let bak: SaveData = serde_json::from_slice(&fs::read(bak_path(&path)).unwrap()).unwrap();
        assert!(!bak.settings.mirror);
        assert!(load(&path).unwrap().settings.mirror);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_moved_aside_and_backup_used() {
        let dir = temp_dir("corrupt");
        let path = dir.join("save.json");
        let mut s = SaveData::default();
        s.settings.judge_offset = 1.5;
        save(&path, &s).unwrap();
        save(&path, &s).unwrap(); // now .bak holds a good copy
        fs::write(&path, b"{ not json").unwrap();

        assert_eq!(load(&path).unwrap(), s);
        assert!(!path.exists());
        let aside: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("save.json.corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(fs::read(dir.join(&aside[0])).unwrap(), b"{ not json");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_without_backup_gives_fresh_save() {
        let dir = temp_dir("corrupt-nobak");
        let path = dir.join("save.json");
        fs::write(&path, b"garbage").unwrap();
        assert_eq!(load(&path).unwrap(), SaveData::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
