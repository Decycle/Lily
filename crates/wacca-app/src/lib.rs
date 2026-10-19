//! Lily: category → song select (with difficulty) → options → play → results, with save data.
//!
//! `lily [--assets <dir>] [--data data] [--autoplay] [--windowed] [--chart <mer>]` (see
//! [`args::USAGE`]). Without `--assets` the game files are `assets/` and `data/` next to the
//! executable ([`paths`]); when they are missing, first-run setup opens ([`setup_flow`]).
//! Menus: Left/Right wheel, Up/Down or D difficulty, PageUp/PageDown genre, Enter / Esc, tap, drag
//! or mouse wheel to scroll, S sort, F filter.
//! Play: Esc give up, Space pause, Up/Down note speed. F11 / Alt+Enter toggles fullscreen.
//! `RUST_LOG=lily=debug` logs every judgement.
//!
//! Performance: F3 shows frame/latency stats (`perf`), also logged every 2 s with
//! `RUST_LOG=lily::perf=debug`. wgpu's `WGPU_BACKEND` / `WGPU_POWER_PREF` choose the GPU;
//! `WACCA_PRESENT=immediate` presents without vsync (tearing, uncapped frame rate);
//! `WACCA_PACE=0` turns off vsync frame pacing (`pacer`).
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod args;
mod calibrate;
mod canvas;
mod category;
mod chrome;
mod circle;
mod folder_dialog;
mod gesture;
mod hitsound;
mod hud;
#[cfg(target_os = "ios")]
mod ios;
mod jacket;
mod labels;
mod menu;
mod menu_audio;
mod mode;
mod options;
mod pacer;
mod paths;
mod perf;
mod play;
mod preview;
mod record;
mod results;
mod screen;
mod select;
mod select_ui;
mod session;
mod setup_backend;
mod setup_flow;
mod setup_task;
mod setup_ui;
mod shell;
mod title;
mod touch_mapping;
mod ui;
mod url_prompt;
mod wipe;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use wacca_audio::{BgmBank, Player, SeBank};
use wacca_game::catalog::Song;
use wacca_game::save::{self, SaveData, Settings};
use wacca_render::text::{Face, Fonts, TextError};

use crate::args::{Args, ArgsError};
use crate::menu::Menu;
use crate::paths::Paths;
use crate::play::PlaySpec;
use crate::record::RecordError;
use crate::select::SongList;
use crate::setup_flow::Reason;
use crate::shell::{Setup, SetupStart, ShellError};

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

/// The game, from `main.rs` on desktops and [`lily_main`] on iOS.
pub fn main() -> ExitCode {
    let started = std::time::Instant::now();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    #[cfg(target_os = "ios")]
    ios::start();
    match args::parse(std::env::args().skip(1))
        .map_err(Error::from)
        .and_then(|a| run(&a, started))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// iOS entry point, called by the Xcode project's `main` (winit's event loop then takes over the
/// process and never returns).
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn lily_main() -> i32 {
    if main() == ExitCode::SUCCESS { 0 } else { 1 }
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
        // The menus' UMG font is the composite `DF-KaGei-W6_Font`: its default typeface is
        // `TT_RODINNTLG-EB` (Latin and Japanese), `DF-KaGei-Kerning` only for ！ ゜ ¶, Hangul from
        // `FTTK-YDHeadB`, fallback `NotoSansSC-Medium` (doc 14 §2).
        vec![
            fonts.join("TT_RODINNTLG-EB.ttf"),
            ufont.join("TT_RODINNTLG-EB.ufont"),
        ],
        vec![
            fonts.join("DF-KaGei-W6.ttc"),
            ufont.join("DF-KaGei-W6.ufont"),
        ],
        vec![
            fonts.join("NotoSansSC-Medium.otf"),
            ufont.join("NotoSansSC-Medium.ufont"),
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
        // macOS / iOS system fonts, so the setup screens have text before any game font exists.
        // iOS 18 loads /System/Library/Fonts/Core/Helvetica.ttc; the other paths are fallbacks.
        vec![
            PathBuf::from("/System/Library/Fonts/Helvetica.ttc"),
            PathBuf::from("/System/Library/Fonts/Core/Helvetica.ttc"),
            PathBuf::from("/System/Library/Fonts/CoreUI/SFUI.ttf"),
            PathBuf::from("/System/Library/Fonts/Core/Avenir.ttc"),
        ],
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
fn start_spec(
    args: &Args,
    assets: &Path,
    chart: &Path,
    songs: &[Song],
) -> Result<PlaySpec, ArgsError> {
    let r = args::resolve_chart(args, assets, chart)?;
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
        movie: play::movie_path(assets, entry.and_then(|e| e.movie.as_deref())),
    })
}

/// Game content from `assets` + `data`; what is missing is logged and left out (the caller
/// checks [`missing_parts`]).
#[derive(Debug)]
pub struct Game {
    pub songs: Vec<Song>,
    pub bgm: Option<BgmBank>,
    pub se: Option<SeBank>,
    pub fonts: Fonts,
}

/// Loads [`Game`]; only finding no font at all (not even a Windows one) is an error.
pub fn load_game(assets: &Path, data: &Path) -> Result<Game, TextError> {
    let list = data.join("songs.json");
    let songs = match record::load_songs(&list) {
        Ok(Some(s)) => s,
        Ok(None) => {
            log::warn!("{} missing (song select will be empty)", list.display());
            Vec::new()
        }
        Err(e) => {
            log::error!("{e}");
            Vec::new()
        }
    };
    let bgm = BgmBank::open(assets.join("Sound").join("Bgm"))
        .map_err(|e| log::warn!("BGM bank: {e}"))
        .ok();
    let se = SeBank::open(assets.join("Sound").join("Se"))
        .map_err(|e| log::warn!("SE bank: {e}; no hit sounds"))
        .ok();
    let fonts = Fonts::load(&pick_fonts(font_candidates(data, assets), Path::exists))?;
    let fonts = match pick_fonts(numeral_candidates(data, assets), Path::exists).first() {
        Some(p) => fonts.with_numerals(p, NUMERAL_FACE),
        None => fonts,
    };
    // `Kakugo_kana` (player name) and `SHINBI_Font` (menu countdown).
    let mut fonts = fonts;
    for (face, file) in [
        (Face::Kakugo, "TT_UDKakugoC60-B.ttf"),
        (Face::Shinbi, "SHINBI.otf"),
    ] {
        let ufont = Path::new(file).with_extension("ufont");
        let paths = [
            data.join("fonts").join(file),
            assets.join("UI").join("Font").join(ufont),
        ];
        if let Some(p) = paths.iter().find(|p| p.exists()) {
            fonts = fonts.with_face(face, p, 0);
        }
    }
    Ok(Game {
        songs,
        bgm,
        se,
        fonts,
    })
}

/// What keeps the game from starting (empty = playable).
pub fn missing_parts(assets: &Path, data: &Path, game: &Game) -> Vec<String> {
    let mut m = paths::missing(assets, data, Path::exists);
    if m.is_empty() && game.bgm.is_none() {
        m.push("game audio (the BGM bank cannot be read)".into());
    }
    m
}

fn resolve_paths(args: &Args) -> Paths {
    // iOS: everything in the app's Documents folder, like a portable install there.
    #[cfg(target_os = "ios")]
    let exe = ios::documents().map(|d| (d, true));
    #[cfg(not(target_os = "ios"))]
    let exe = paths::exe_dir();
    // `%APPDATA%`, or the XDG data folder elsewhere.
    let appdata = save::data_base();
    paths::resolve(&paths::Inputs {
        assets: args.assets.as_deref(),
        data: args.data.as_deref(),
        save: args.save.as_deref(),
        portable_flag: args.portable,
        exe_dir: exe.as_ref().map(|(d, _)| d.as_path()),
        marker: exe.as_ref().is_some_and(|(_, m)| *m),
        appdata: appdata.as_deref(),
    })
}

/// When the save is the default one (`%APPDATA%\lily\save.json`), copy a save from the old
/// `wacca-slim` folder there once ([`save::migrate_legacy`]). Portable and `--save` saves are
/// never touched.
fn migrate_legacy_save(save_path: Option<&Path>) {
    let Some(base) = save::data_base() else {
        return;
    };
    if save_path != Some(save::save_path_in(&base).as_path()) {
        return;
    }
    match save::migrate_legacy(&base) {
        Ok(Some(old)) => log::info!(
            "copied the save from {} to {} (the old file is kept)",
            old.display(),
            save::save_path_in(&base).display()
        ),
        Ok(None) => {}
        Err(e) => log::warn!("could not copy the old save to the Lily folder: {e}"),
    }
}

// cpal's output stream is not `Send` on macOS; the player is only used on the main (event loop) thread.
#[allow(clippy::arc_with_non_send_sync)]
fn run(args: &Args, started: std::time::Instant) -> Result<(), Error> {
    let paths = resolve_paths(args);
    log::info!(
        "assets {}, data {}, save {}",
        paths.assets.display(),
        paths.data.display(),
        paths
            .save
            .as_ref()
            .map_or("none".into(), |p| p.display().to_string())
    );
    let game = load_game(&paths.assets, &paths.data)?;
    let missing = missing_parts(&paths.assets, &paths.data, &game);
    let setup = if !missing.is_empty() {
        log::warn!("game files missing ({}); opening setup", missing.join(", "));
        Some(Reason::Missing(missing))
    } else if args.screen == Some(screen::Screen::Setup)
        || args.setup_source.is_some()
        || args.setup_url.is_some()
    {
        Some(Reason::Requested)
    } else {
        None
    }
    .map(|reason| SetupStart {
        reason,
        source: args.setup_source.clone(),
        url: args.setup_url.clone(),
        start: args.setup_start,
    });
    let start = args
        .chart
        .as_deref()
        .filter(|_| setup.is_none())
        .map(|c| start_spec(args, &paths.assets, c, &game.songs))
        .transpose()?;

    migrate_legacy_save(paths.save.as_deref());
    let mut save_path = paths.save.clone();
    let save = match &save_path {
        Some(p) => save::load(p).unwrap_or_else(|e| {
            // Do not overwrite a save we could not read.
            log::error!("{e}; results will not be saved");
            save_path = None;
            SaveData::default()
        }),
        None => {
            log::warn!("no save location (%APPDATA% not set); results will not be saved");
            SaveData::default()
        }
    };
    let menu = Menu {
        list: SongList::new(game.songs),
        category: 0,
        options: options::OptionsMenu::default(),
        settings: settings(&save.settings, args),
        autoplay: args.autoplay,
        calibration: Default::default(),
    };
    // A portable folder may carry its own `ffmpeg.exe` next to the executable.
    let local_ffmpeg = paths::exe_dir()
        .map(|(d, _)| d.join("ffmpeg.exe"))
        .filter(|p| p.is_file() && std::env::var_os("WACCA_FFMPEG").is_none());
    let ffmpeg = match local_ffmpeg.map_or_else(wacca_video::find_ffmpeg_from_env, Ok) {
        Ok(p) => {
            log::info!("ffmpeg: {}", p.display());
            Some(p)
        }
        Err(e) => {
            log::warn!("{e}; no background movies");
            None
        }
    };
    shell::run(
        Setup {
            assets: paths.assets.clone(),
            data: paths.data.clone(),
            bgm: game.bgm,
            player: Arc::new(Player::new()?),
            se: game.se,
            save_path,
            save,
            menu,
            start,
            screen: args.screen.filter(|s| *s != screen::Screen::Setup),
            setup,
            setup_root: paths.setup_root.clone(),
            setup_by_url: cfg!(target_os = "ios") || args.setup_url.is_some(),
            backend: Arc::new(setup_backend::Real),
            windowed: args.windowed,
            ffmpeg,
            started,
        },
        game.fonts,
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
        assert_eq!(picked.len(), 7);
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
        let s = start_spec(&a, Path::new("A"), a.chart.as_deref().unwrap(), &[]).unwrap();
        assert_eq!((s.title.as_str(), s.song_id), ("S00-003", None));
        assert_eq!(
            s.norma,
            record::default_norma(wacca_chart::Difficulty::Expert)
        );
    }
}
