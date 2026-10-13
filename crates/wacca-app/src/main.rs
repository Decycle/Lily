//! WACCA_slim: category → song select (with difficulty) → options → play → results, with save data.
//!
//! `wacca-app --assets <dir> [--data data] [--autoplay] [--windowed] [--chart <mer>]` (see [`args::USAGE`]).
//! Menus: Left/Right wheel, Up/Down or D difficulty, PageUp/PageDown genre, Enter / Esc, tap, drag
//! or mouse wheel to scroll, S sort, F filter.
//! Play: Esc give up, Space pause, Up/Down note speed. F11 / Alt+Enter toggles fullscreen.
//! `RUST_LOG=wacca_app=debug` logs every judgement.
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod args;
mod canvas;
mod category;
mod chrome;
mod circle;
mod gesture;
mod hitsound;
mod hud;
mod jacket;
mod labels;
mod menu;
mod mode;
mod options;
mod play;
mod preview;
mod record;
mod results;
mod screen;
mod select;
mod select_ui;
mod session;
mod shell;
mod title;
mod touch_mapping;
mod ui;
mod wipe;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use wacca_audio::{BgmBank, Player, SeBank};
use wacca_game::catalog::Song;
use wacca_game::save::{self, SaveData, Settings};
use wacca_render::text::{Fonts, TextError};

use crate::args::{Args, ArgsError};
use crate::menu::Menu;
use crate::play::PlaySpec;
use crate::record::RecordError;
use crate::select::SongList;
use crate::shell::{Setup, ShellError};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Args(#[from] ArgsError),
    #[error("audio: {0}")]
    Audio(#[from] wacca_audio::Error),
    #[error(transparent)]
    Shell(#[from] ShellError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error(transparent)]
    Text(#[from] TextError),
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match args::parse(std::env::args().skip(1))
        .map_err(Error::from)
        .and_then(|a| run(&a))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Fonts in priority order, one per group (the first file of a group that exists): the game's
/// Japanese UI font (exported by `wacca-convert --fonts`, or the raw `.ufont` = plain OpenType),
/// a broad CJK fallback, then a Windows symbol font for arrows.
fn font_candidates(data: &Path, assets: &Path) -> Vec<Vec<PathBuf>> {
    let fonts = data.join("fonts");
    let ufont = assets.join("UI").join("Font");
    let windows = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
        .join("Fonts");
    vec![
        // The menus' UMG font (`DF-KaGei-W6_Font`).
        vec![
            fonts.join("DF-KaGei-W6.ttc"),
            ufont.join("DF-KaGei-W6.ufont"),
        ],
        vec![
            fonts.join("FOT-UDKakugo_SmallPr6-B.otf"),
            ufont.join("FOT-UDKakugo_SmallPr6-B.ufont"),
        ],
        vec![
            fonts.join("DroidSansFallback.ttf"),
            ufont.join("DroidSansFallback.ufont"),
            windows.join("meiryo.ttc"),
            windows.join("msgothic.ttc"),
        ],
        vec![windows.join("seguisym.ttf")],
    ]
}

/// Face of `DF-KaGei-W6.ttc` used for results counters and scores: `DFPKaGei-W6` (proportional).
// UNVERIFIED: chosen by comparing E2 result frames; the widgets name the `DF-KaGei-W6_Font`
// composite, whose sub-typeface ranges are not decoded.
const NUMERAL_FACE: u32 = 1;

fn numeral_candidates(data: &Path, assets: &Path) -> Vec<Vec<PathBuf>> {
    vec![vec![
        data.join("fonts").join("DF-KaGei-W6.ttc"),
        assets.join("UI").join("Font").join("DF-KaGei-W6.ufont"),
    ]]
}

/// First path of each group for which `exists` holds.
fn pick_fonts(groups: Vec<Vec<PathBuf>>, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    groups
        .into_iter()
        .filter_map(|g| g.into_iter().find(|p| exists(p)))
        .collect()
}

/// Saved settings with the command-line overrides applied.
fn settings(saved: &Settings, args: &Args) -> Settings {
    let mut s = saved.clone();
    if let Some(speed) = args.speed {
        s.note_speed = speed as f32;
    }
    if let Some(offset) = args.offset {
        s.judge_offset = options::offset_units(offset);
    }
    s
}

/// `--chart`: what to play straight away.
fn start_spec(args: &Args, chart: &Path, songs: &[Song]) -> Result<PlaySpec, ArgsError> {
    let r = args::resolve_chart(args, chart)?;
    let song = r
        .folder
        .as_deref()
        .and_then(|f| record::find_song(songs, f));
    let entry = song.zip(r.difficulty).and_then(|(s, d)| s.chart(d));
    Ok(PlaySpec {
        title: song.map_or_else(|| r.folder.clone().unwrap_or_default(), |s| s.title.clone()),
        jacket: song.map(|s| s.jacket.clone()).unwrap_or_default(),
        song_id: song.map(|s| s.id),
        difficulty: r.difficulty,
        level: entry.map(|e| e.level),
        norma: entry.map(|e| e.norma).unwrap_or_else(|| {
            record::default_norma(r.difficulty.unwrap_or(wacca_chart::Difficulty::Expert))
        }),
        chart_path: r.path,
        movie: play::movie_path(&args.assets, entry.and_then(|e| e.movie.as_deref())),
    })
}

fn run(args: &Args) -> Result<(), Error> {
    let songs = record::load_songs(&args.data.join("songs.json"))?.unwrap_or_else(|| {
        log::warn!(
            "{} missing; run wacca-convert (song select will be empty)",
            args.data.join("songs.json").display()
        );
        Vec::new()
    });
    let start = args
        .chart
        .as_deref()
        .map(|c| start_spec(args, c, &songs))
        .transpose()?;

    let mut save_path = args.save.clone().or_else(save::default_path);
    let save = match &save_path {
        Some(p) => save::load(p).unwrap_or_else(|e| {
            // Do not overwrite a save we could not read.
            log::error!("{e}; results will not be saved");
            save_path = None;
            SaveData::default()
        }),
        None => {
            log::warn!("%APPDATA% not set; results will not be saved");
            SaveData::default()
        }
    };
    let menu = Menu {
        list: SongList::new(songs),
        category: 0,
        options: options::OptionsMenu::default(),
        settings: settings(&save.settings, args),
        autoplay: args.autoplay,
    };
    let bgm = BgmBank::open(args.assets.join("Sound").join("Bgm"))?;
    let se = SeBank::open(args.assets.join("Sound").join("Se"))
        .map_err(|e| log::warn!("SE bank: {e}; no hit sounds"))
        .ok();
    let ffmpeg = match wacca_video::find_ffmpeg_from_env() {
        Ok(p) => {
            log::info!("ffmpeg: {}", p.display());
            Some(p)
        }
        Err(e) => {
            log::warn!("{e}; no background movies");
            None
        }
    };
    let fonts = Fonts::load(&pick_fonts(
        font_candidates(&args.data, &args.assets),
        Path::exists,
    ))?;
    let fonts = match pick_fonts(numeral_candidates(&args.data, &args.assets), Path::exists).first()
    {
        Some(p) => fonts.with_numerals(p, NUMERAL_FACE),
        None => fonts,
    };
    shell::run(
        Setup {
            assets: args.assets.clone(),
            data: args.data.clone(),
            bgm,
            player: Arc::new(Player::new()?),
            se,
            save_path,
            save,
            menu,
            start,
            windowed: args.windowed,
            ffmpeg,
        },
        fonts,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_apply_to_saved_settings() {
        let a = args::parse(
            "--assets A --speed 2.5 --offset-ms 25"
                .split_whitespace()
                .map(String::from),
        )
        .unwrap();
        let s = settings(&Settings::default(), &a);
        assert_eq!(s.note_speed, 2.5);
        assert_eq!(s.judge_offset, 3.0);
        let none = args::parse(["--assets".into(), "A".into()]).unwrap();
        assert_eq!(settings(&Settings::default(), &none), Settings::default());
    }

    #[test]
    fn game_fonts_come_first() {
        let c = font_candidates(Path::new("data"), Path::new("assets"));
        assert!(c[0][0].starts_with("data") && c[0][1].starts_with("assets"));
        // Without the exported fonts the raw game fonts are used.
        let picked = pick_fonts(c, |p| !p.starts_with("data"));
        assert_eq!(picked.len(), 4);
        assert!(picked[0].starts_with("assets") && picked[1].starts_with("assets"));
        assert!(pick_fonts(vec![vec![PathBuf::from("x")]], |_| false).is_empty());
    }

    #[test]
    fn start_spec_uses_catalog_when_known() {
        let a = args::parse(
            "--assets A --chart MusicData/S00-003/S00-003_02.mer"
                .split_whitespace()
                .map(String::from),
        )
        .unwrap();
        let s = start_spec(&a, a.chart.as_deref().unwrap(), &[]).unwrap();
        assert_eq!((s.title.as_str(), s.song_id), ("S00-003", None));
        assert_eq!(
            s.norma,
            record::default_norma(wacca_chart::Difficulty::Expert)
        );
    }
}
