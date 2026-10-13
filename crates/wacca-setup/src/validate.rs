//! Lenient post-conversion check: every chart must parse and its BGM cue must resolve; broken
//! charts are dropped from `songs.json`, only missing core files are fatal.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use wacca_audio::BgmBank;
use wacca_game::catalog::Song;

use crate::{SetupError, SkippedChart, Summary, io_err};

/// Files in `data/` without which nothing can be played or drawn.
pub const CORE_DATA: [&str; 3] = [
    "meshes/vMsh_DefaultNote00_01_s.obj",
    "ui/Vfx/Textures/vT_NoteBase00_00_m.png",
    "ui/UI/Textures/FONT/uT_FTdecision01.png",
];

/// What a chart needs to be playable.
pub trait ChartCheck {
    /// `Ok(cue)` if the chart at `chart_path` (relative to the assets) parses.
    fn chart_cue(&mut self, chart_path: &str) -> Result<String, String>;
    /// `Ok` if the BGM cue resolves to audio on disk.
    fn cue(&mut self, cue: &str) -> Result<(), String>;
    /// Whether the background movie `name` exists.
    fn movie(&mut self, name: &str) -> bool;
}

/// Drops the charts `check` rejects (and songs left without charts). Pure logic: returns the
/// kept songs plus a summary (`warnings` empty, sizes 0).
pub fn filter_songs(
    songs: Vec<Song>,
    check: &mut dyn ChartCheck,
    movies: bool,
) -> (Vec<Song>, Summary) {
    let mut summary = Summary::default();
    let mut cues: HashMap<String, Result<(), String>> = HashMap::new();
    let (mut present, mut missing) = (BTreeSet::new(), BTreeSet::new());
    let mut kept = Vec::new();
    for mut song in songs {
        song.charts.retain(|c| {
            let ok = check.chart_cue(&c.chart_path).and_then(|cue| {
                cues.entry(cue.clone())
                    .or_insert_with(|| check.cue(&cue))
                    .clone()
                    .map_err(|e| format!("BGM cue {cue}: {e}"))
            });
            if let Err(reason) = ok {
                summary.charts_skipped.push(SkippedChart {
                    song: song.title.clone(),
                    chart_path: c.chart_path.clone(),
                    reason,
                });
                return false;
            }
            if let (true, Some(m)) = (movies, &c.movie) {
                if check.movie(m) {
                    present.insert(m.clone());
                } else {
                    missing.insert(m.clone());
                }
            }
            true
        });
        if song.charts.is_empty() {
            summary.songs_skipped += 1;
        } else {
            summary.charts_playable += song.charts.len();
            kept.push(song);
        }
    }
    summary.songs_playable = kept.len();
    summary.movies_present = present.len();
    summary.movies_missing = missing.len();
    (kept, summary)
}

/// [`ChartCheck`] against real files.
struct Disk<'a> {
    assets: &'a Path,
    bgm: BgmBank,
}

impl ChartCheck for Disk<'_> {
    fn chart_cue(&mut self, chart_path: &str) -> Result<String, String> {
        let bytes = std::fs::read(self.assets.join(chart_path)).map_err(|e| e.to_string())?;
        let chart = wacca_chart::parse(&bytes).map_err(|e| e.to_string())?;
        Ok(chart.header.music_file_path)
    }
    fn cue(&mut self, cue: &str) -> Result<(), String> {
        let loc = self.bgm.locate(cue).map_err(|e| e.to_string())?;
        if loc.awb.is_file() {
            Ok(())
        } else {
            Err(format!("{} is missing", loc.awb.display()))
        }
    }
    fn movie(&mut self, name: &str) -> bool {
        self.assets
            .join("Movie")
            .join(format!("{name}.usm"))
            .is_file()
    }
}

/// Validates `assets` + `data`, rewrites `data/songs.json` without the unplayable charts.
pub fn validate(assets: &Path, data: &Path, movies: bool) -> Result<Summary, SetupError> {
    let songs_path = data.join("songs.json");
    let bytes = std::fs::read(&songs_path).map_err(io_err(&songs_path))?;
    let songs: Vec<Song> = serde_json::from_slice(&bytes)
        .map_err(|e| SetupError::UnsupportedVersion(format!("songs.json: {e}")))?;
    if songs.is_empty() {
        return Err(SetupError::UnsupportedVersion(
            "the song table has no playable songs (table layout not recognised)".into(),
        ));
    }
    let bgm_dir = assets.join("Sound").join("Bgm");
    let bgm = BgmBank::open(&bgm_dir)
        .map_err(|e| SetupError::MissingCore(format!("BGM bank in {}: {e}", bgm_dir.display())))?;
    for f in CORE_DATA {
        if !data.join(f).is_file() {
            return Err(SetupError::MissingCore(format!(
                "{f} was not produced by the converter"
            )));
        }
    }
    let total = songs.len();
    let (kept, mut summary) = filter_songs(songs, &mut Disk { assets, bgm }, movies);
    if kept.is_empty() {
        let first = summary.charts_skipped.first().map(|s| s.reason.clone());
        return Err(SetupError::MissingCore(format!(
            "none of the {total} songs is playable (first problem: {})",
            first.unwrap_or_default()
        )));
    }
    if !summary.charts_skipped.is_empty() {
        let json = serde_json::to_vec_pretty(&kept)
            .map_err(|e| SetupError::UnsupportedVersion(e.to_string()))?;
        std::fs::write(&songs_path, json).map_err(io_err(&songs_path))?;
    }
    let se = assets.join("Sound").join("Se");
    if let Err(e) = wacca_audio::SeBank::open(&se) {
        summary
            .warnings
            .push(format!("no hit sounds: SE bank: {e}"));
    }
    if summary.songs_skipped > 0 {
        summary.warnings.push(format!(
            "{} songs have no playable chart and were left out",
            summary.songs_skipped
        ));
    }
    if movies && summary.movies_missing > 0 {
        summary.warnings.push(format!(
            "{} background movies are missing (those songs play without one)",
            summary.movies_missing
        ));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_game::Difficulty;
    use wacca_game::catalog::ChartEntry;

    fn song(id: u32, charts: &[(&str, Option<&str>)]) -> Song {
        Song {
            id,
            folder: format!("S{id}"),
            title: format!("T{id}"),
            artist: String::new(),
            reading: String::new(),
            bpm: String::new(),
            genre: 0,
            version: 1,
            jacket: String::new(),
            preview_begin: 0.0,
            preview_seconds: 0.0,
            charts: charts
                .iter()
                .map(|(p, m)| ChartEntry {
                    difficulty: Difficulty::Normal,
                    level: 1.0,
                    norma: 0.0,
                    movie: m.map(Into::into),
                    chart_path: (*p).into(),
                })
                .collect(),
        }
    }

    /// Charts named `bad*` don't parse; cue = chart path's first char; cue `x` doesn't resolve;
    /// movies named `mv*` exist.
    struct Fake(usize);
    impl ChartCheck for Fake {
        fn chart_cue(&mut self, p: &str) -> Result<String, String> {
            if p.starts_with("bad") {
                Err("parse error".into())
            } else {
                Ok(p[..1].to_owned())
            }
        }
        fn cue(&mut self, cue: &str) -> Result<(), String> {
            self.0 += 1;
            if cue == "x" {
                Err("not in bank".into())
            } else {
                Ok(())
            }
        }
        fn movie(&mut self, name: &str) -> bool {
            name.starts_with("mv")
        }
    }

    #[test]
    fn drops_broken_charts_and_empty_songs() {
        let songs = vec![
            song(
                1,
                &[("a0", Some("mv1")), ("bad1", None), ("a2", Some("gone"))],
            ),
            song(2, &[("bad", None), ("x1", None)]),
            song(3, &[("a3", Some("mv1"))]),
        ];
        let mut fake = Fake(0);
        let (kept, s) = filter_songs(songs, &mut fake, true);
        assert_eq!(kept.iter().map(|s| s.id).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(kept[0].charts.len(), 2);
        assert_eq!(
            (s.songs_playable, s.charts_playable, s.songs_skipped),
            (2, 3, 1)
        );
        assert_eq!((s.movies_present, s.movies_missing), (1, 1));
        let reasons: Vec<_> = s
            .charts_skipped
            .iter()
            .map(|c| (c.chart_path.as_str(), c.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                ("bad1", "parse error"),
                ("bad", "parse error"),
                ("x1", "BGM cue x: not in bank")
            ]
        );
        assert_eq!(fake.0, 2, "each cue is checked once");
        let (_, no_movies) =
            filter_songs(vec![song(1, &[("a", Some("gone"))])], &mut Fake(0), false);
        assert_eq!((no_movies.movies_present, no_movies.movies_missing), (0, 0));
    }

    #[test]
    fn missing_files_are_fatal_with_clear_errors() {
        let root = crate::tests::temp_dir("validate");
        let (assets, data) = (root.join("assets"), root.join("data"));
        assert!(matches!(
            validate(&assets, &data, false),
            Err(SetupError::Io { .. })
        ));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("songs.json"), "[]").unwrap();
        assert!(matches!(
            validate(&assets, &data, false),
            Err(SetupError::UnsupportedVersion(_))
        ));
        let json = serde_json::to_vec(&[song(1, &[("a", None)])]).unwrap();
        std::fs::write(data.join("songs.json"), json).unwrap();
        match validate(&assets, &data, false) {
            Err(SetupError::MissingCore(m)) => assert!(m.contains("BGM bank"), "{m}"),
            other => panic!("{other:?}"),
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
