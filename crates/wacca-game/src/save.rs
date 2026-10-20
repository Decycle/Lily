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
    /// Reverse Points earned so far (sum of every saved play's RP, [`crate::points`]).
    pub reverse_points: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            version: SAVE_VERSION,
            settings: Settings::default(),
            records: BTreeMap::new(),
            reverse_points: 0,
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
    /// Background movie on/off (`BackMovieSettingTable`, 06-visuals §1).
    pub background_video: bool,
    /// Measure lines on the playfield (`BarLineTable`; on by default, as in real guest play per E2 frames).
    pub bar_lines: bool,
    /// Depth-ring guides and pressed-sensor highlight for touchscreens (not in the real game).
    pub touch_guide: bool,
    /// Name on the results screen's player plate (the game shows the card's user name).
    pub player_name: String,
    /// Title line above the plate (the game's earned titles; empty = none).
    pub player_title: String,
    /// Show the arcade's menu countdowns and advance when they run out (off in this home build).
    pub menu_timers: bool,
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
            background_video: true,
            bar_lines: true,
            // On by default on tablets (iPad, Android), where nothing else shows the touch area.
            touch_guide: cfg!(any(target_os = "ios", target_os = "android")),
            player_name: "PLAYER".to_string(),
            player_title: String::new(),
            menu_timers: false,
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

    /// Plays over all charts (incl. give-ups).
    pub fn total_plays(&self) -> u32 {
        self.records.values().map(|r| r.play_count).sum()
    }

    /// Merge a play into its chart's record. Returns `true` on a new best score.
    pub fn merge(&mut self, song_id: u32, d: Difficulty, r: &PlayResult) -> bool {
        self.records
            .entry(chart_key(song_id, d))
            .or_default()
            .merge(r)
    }
}

/// Folder name under the data directory (`%APPDATA%\lily`).
pub const APP_DIR: &str = "lily";
/// Folder used before the rename to Lily; its save is migrated once (see [`migrate_legacy`]).
pub const LEGACY_APP_DIR: &str = "wacca-slim";

/// `<appdata>\lily\save.json`.
pub fn save_path_in(appdata: &Path) -> PathBuf {
    appdata.join(APP_DIR).join("save.json")
}

/// `<appdata>\wacca-slim\save.json` (the location before the rename).
pub fn legacy_save_path_in(appdata: &Path) -> PathBuf {
    appdata.join(LEGACY_APP_DIR).join("save.json")
}

/// Per-user data directory: `%APPDATA%` on Windows; elsewhere `$XDG_DATA_HOME`, else
/// `~/.local/share`.
pub fn data_base() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    var("APPDATA")
        .map(PathBuf::from)
        .or_else(|| var("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| var("HOME").map(|h| Path::new(&h).join(".local").join("share")))
}

/// Default save path: [`save_path_in`] of [`data_base`].
pub fn default_path() -> Option<PathBuf> {
    data_base().map(|b| save_path_in(&b))
}

/// One-time migration after the rename: when `<appdata>\lily\save.json` does not exist but
/// `<appdata>\wacca-slim\save.json` does, copy it (and its `.bak`, if any) to the new folder.
/// The old files are left untouched. Returns the old save's path when something was copied.
pub fn migrate_legacy(appdata: &Path) -> io::Result<Option<PathBuf>> {
    let new = save_path_in(appdata);
    let old = legacy_save_path_in(appdata);
    if new.exists() || !old.is_file() {
        return Ok(None);
    }
    if let Some(dir) = new.parent() {
        fs::create_dir_all(dir)?;
    }
    let (old_bak, new_bak) = (bak_path(&old), bak_path(&new));
    if old_bak.is_file() && !new_bak.exists() {
        fs::copy(&old_bak, &new_bak)?;
    }
    // Copy under a temporary name first so an interrupted copy never looks like a real save.
    let tmp = sibling(&new, ".migrating");
    fs::copy(&old, &tmp)?;
    fs::rename(&tmp, &new)?;
    Ok(Some(old))
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
            r_counts: Counts::default(),
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
        assert_eq!(p, Path::new("C:/AppData").join("lily").join("save.json"));
        assert_eq!(
            legacy_save_path_in(Path::new("C:/AppData")),
            Path::new("C:/AppData").join("wacca-slim").join("save.json")
        );
        assert_eq!(bak_path(&p).file_name().unwrap(), "save.json.bak");
    }

    #[test]
    fn migrates_the_legacy_save_once() {
        let base = temp_dir("migrate");
        // Nothing to migrate.
        assert!(migrate_legacy(&base).unwrap().is_none());
        assert!(!base.join(APP_DIR).exists());
        let old = legacy_save_path_in(&base);
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, b"{\"v\":1}").unwrap();
        fs::write(bak_path(&old), b"{\"v\":0}").unwrap();
        assert_eq!(migrate_legacy(&base).unwrap(), Some(old.clone()));
        let new = save_path_in(&base);
        assert_eq!(fs::read(&new).unwrap(), b"{\"v\":1}");
        assert_eq!(fs::read(bak_path(&new)).unwrap(), b"{\"v\":0}");
        assert!(!sibling(&new, ".migrating").exists());
        // The old files stay.
        assert_eq!(fs::read(&old).unwrap(), b"{\"v\":1}");
        assert_eq!(fs::read(bak_path(&old)).unwrap(), b"{\"v\":0}");
        // Only once: an existing new save is never overwritten.
        fs::write(&new, b"{\"v\":2}").unwrap();
        assert!(migrate_legacy(&base).unwrap().is_none());
        assert_eq!(fs::read(&new).unwrap(), b"{\"v\":2}");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn migrates_without_a_bak() {
        let base = temp_dir("migrate-nobak");
        let old = legacy_save_path_in(&base);
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, b"{}").unwrap();
        assert!(migrate_legacy(&base).unwrap().is_some());
        let new = save_path_in(&base);
        assert!(new.is_file());
        assert!(!bak_path(&new).exists());
        fs::remove_dir_all(&base).unwrap();
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
    fn background_video_setting_persists_and_defaults_on() {
        let old: Settings = serde_json::from_str(r#"{"noteSpeed": 3.0}"#).unwrap();
        assert!(old.background_video);
        assert_eq!(old.player_name, "PLAYER");
        let off = Settings {
            background_video: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&off).unwrap();
        assert!(json.contains(r#""backgroundVideo":false"#));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), off);
    }

    #[test]
    fn bar_lines_default_on_touch_guide_default_off_and_persist() {
        let old: Settings = serde_json::from_str(r#"{"noteSpeed": 3.0}"#).unwrap();
        let tablet = cfg!(any(target_os = "ios", target_os = "android"));
        assert!(old.bar_lines && old.touch_guide == tablet);
        let on = Settings {
            bar_lines: false,
            touch_guide: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&on).unwrap();
        assert!(json.contains(r#""barLines":false"#) && json.contains(r#""touchGuide":true"#));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), on);
    }

    #[test]
    fn plate_fields_default_and_count_plays() {
        let old: SaveData = serde_json::from_str(r#"{"settings": {"noteSpeed": 3.0}}"#).unwrap();
        assert_eq!(old.reverse_points, 0);
        assert!(old.settings.player_title.is_empty() && !old.settings.menu_timers);
        let mut s = SaveData::default();
        s.merge(1, Difficulty::Hard, &result(900_000, Lamp::Clear, 0, 10));
        s.merge(1, Difficulty::Hard, &result(910_000, Lamp::Clear, 0, 10));
        s.merge(2, Difficulty::Expert, &result(0, Lamp::Failed, 50, 0));
        assert_eq!(s.total_plays(), 3);
        s.reverse_points = 870;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""reversePoints":870"#) && json.contains("menuTimers"));
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
