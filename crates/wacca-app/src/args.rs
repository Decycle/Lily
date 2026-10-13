//! Command line: which chart to play and how. Pure (no filesystem access).

use std::path::{Path, PathBuf};

use wacca_chart::Difficulty;

use crate::screen::Screen;

pub const USAGE: &str = "usage: lily [--assets <dir>] [--data <dir>] [--save <file>] [--portable] \
[--windowed] [--autoplay] [--speed 1.0..6.0] [--offset-ms N] \
[--screen title|mode|category|select|options|calibrate|results|setup] \
[--setup-source <dir> [--setup-start]] \
[--chart <MusicData/<song>/<song>_NN.mer | MusicData/<song>> [--difficulty normal|hard|expert|inferno]]";

pub const SPEED_RANGE: std::ops::RangeInclusive<f64> = 1.0..=6.0;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ArgsError {
    #[error("{USAGE}")]
    Usage,
    #[error("unknown argument `{0}`\n{USAGE}")]
    Unknown(String),
    #[error("{flag}: invalid value `{value}`")]
    Value { flag: &'static str, value: String },
    #[error("{0}: a song folder needs --difficulty")]
    NeedsDifficulty(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    /// Game `Content` folder (default: `assets` next to the executable, see [`crate::paths`]).
    pub assets: Option<PathBuf>,
    /// Play this `.mer` file or song folder (relative to `assets` unless absolute) right away
    /// instead of starting at song select.
    pub chart: Option<PathBuf>,
    pub difficulty: Option<Difficulty>,
    /// Initial autoplay option.
    pub autoplay: bool,
    /// Note speed for this session (overrides the saved setting).
    pub speed: Option<f64>,
    /// User timing offset in seconds, added to chart time (overrides the saved setting, rounded to
    /// its step: see `options::offset_units`).
    pub offset: Option<f64>,
    /// Directory with `songs.json`, `jackets/`, `fonts/` (from `wacca-convert`).
    pub data: Option<PathBuf>,
    /// Save file (default `%APPDATA%/lily/save.json`, or `save/save.json` next to the
    /// executable when portable).
    pub save: Option<PathBuf>,
    /// Keep the save next to the executable (also on with a `portable.txt` there).
    pub portable: bool,
    /// Setup: inspect this folder right away instead of asking for one (debugging).
    pub setup_source: Option<PathBuf>,
    /// With `setup_source`: also start the setup (without videos) and continue to the title.
    pub setup_start: bool,
    pub windowed: bool,
    /// Open this menu screen instead of the title (debugging, screenshots).
    pub screen: Option<Screen>,
}

fn screen(v: &str) -> Option<Screen> {
    Some(match v {
        "title" => Screen::Title,
        "mode" => Screen::ModeSelect,
        "category" => Screen::Category,
        "select" => Screen::SongSelect,
        "options" => Screen::Options,
        "calibrate" => Screen::Calibrate,
        "results" => Screen::Results,
        "setup" => Screen::Setup,
        _ => return None,
    })
}

fn value<T: std::str::FromStr>(flag: &'static str, v: Option<String>) -> Result<T, ArgsError> {
    let v = v.ok_or(ArgsError::Usage)?;
    v.parse().map_err(|_| ArgsError::Value { flag, value: v })
}

fn difficulty(v: &str) -> Option<Difficulty> {
    match v.to_ascii_lowercase().as_str() {
        "normal" | "0" => Some(Difficulty::Normal),
        "hard" | "1" => Some(Difficulty::Hard),
        "expert" | "2" => Some(Difficulty::Expert),
        "inferno" | "3" => Some(Difficulty::Inferno),
        _ => None,
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, ArgsError> {
    let mut args = args.into_iter();
    let mut diff = None;
    let mut out = Args {
        assets: None,
        chart: None,
        difficulty: None,
        autoplay: false,
        speed: None,
        offset: None,
        data: None,
        save: None,
        portable: false,
        setup_source: None,
        setup_start: false,
        windowed: false,
        screen: None,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--assets" => out.assets = Some(value::<PathBuf>("--assets", args.next())?),
            "--portable" => out.portable = true,
            "--setup-source" => out.setup_source = Some(value("--setup-source", args.next())?),
            "--setup-start" => out.setup_start = true,
            "--chart" => out.chart = Some(value::<PathBuf>("--chart", args.next())?),
            "--save" => out.save = Some(value::<PathBuf>("--save", args.next())?),
            "--windowed" => out.windowed = true,
            "--screen" => {
                let v: String = value("--screen", args.next())?;
                out.screen = Some(screen(&v).ok_or(ArgsError::Value {
                    flag: "--screen",
                    value: v,
                })?);
            }
            "--data" => out.data = Some(value("--data", args.next())?),
            "--difficulty" => diff = Some(value::<String>("--difficulty", args.next())?),
            "--autoplay" => out.autoplay = true,
            "--speed" => {
                let speed = value("--speed", args.next())?;
                if !SPEED_RANGE.contains(&speed) {
                    return Err(ArgsError::Value {
                        flag: "--speed",
                        value: speed.to_string(),
                    });
                }
                out.speed = Some(speed);
            }
            "--offset-ms" => {
                let ms: f64 = value("--offset-ms", args.next())?;
                if !ms.is_finite() {
                    return Err(ArgsError::Value {
                        flag: "--offset-ms",
                        value: ms.to_string(),
                    });
                }
                out.offset = Some(ms / 1000.0);
            }
            "-h" | "--help" => return Err(ArgsError::Usage),
            _ => return Err(ArgsError::Unknown(flag)),
        }
    }
    out.difficulty = diff
        .map(|d| {
            difficulty(&d).ok_or(ArgsError::Value {
                flag: "--difficulty",
                value: d,
            })
        })
        .transpose()?;
    Ok(out)
}

/// What the chart path says about the song.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRef {
    pub path: PathBuf,
    /// Song folder name, e.g. `S00-003` (= `Song::folder`).
    pub folder: Option<String>,
    pub difficulty: Option<Difficulty>,
}

/// Resolve `chart` (+ `--difficulty`) under `assets` to a `.mer` path: a song folder becomes
/// `<folder>/<folder>_NN.mer`; a file's difficulty comes from its `_NN` suffix unless given.
pub fn resolve_chart(args: &Args, assets: &Path, chart: &Path) -> Result<ChartRef, ArgsError> {
    let path = assets.join(chart);
    let is_file = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mer"));
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    if is_file {
        let suffix = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit_once('_'))
            .and_then(|(_, n)| n.parse::<u8>().ok())
            .and_then(Difficulty::from_file_suffix);
        return Ok(ChartRef {
            folder: path.parent().and_then(name),
            difficulty: args.difficulty.or(suffix),
            path,
        });
    }
    let d = args
        .difficulty
        .ok_or_else(|| ArgsError::NeedsDifficulty(path.clone()))?;
    let folder = name(&path).ok_or(ArgsError::NeedsDifficulty(path.clone()))?;
    Ok(ChartRef {
        path: path.join(d.chart_file_name(&folder)),
        folder: Some(folder),
        difficulty: Some(d),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<Args, ArgsError> {
        parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_all_flags() {
        let a = args("--assets A --chart c.mer --autoplay --speed 2.5 --offset-ms -20 --data D --difficulty hard --save S --windowed").unwrap();
        assert_eq!(a.assets, Some(PathBuf::from("A")));
        assert_eq!(a.chart, Some(PathBuf::from("c.mer")));
        assert!(a.autoplay && a.windowed);
        assert_eq!(a.speed, Some(2.5));
        assert!((a.offset.unwrap() + 0.020).abs() < 1e-12);
        assert_eq!(a.data, Some(PathBuf::from("D")));
        assert_eq!(a.save, Some(PathBuf::from("S")));
        assert_eq!(a.difficulty, Some(Difficulty::Hard));
        assert_eq!(a.screen, None);
        let a = args("--assets A --screen select").unwrap();
        assert_eq!(a.screen, Some(Screen::SongSelect));
        assert!(matches!(
            args("--assets A --screen play"),
            Err(ArgsError::Value { .. })
        ));
    }

    #[test]
    fn defaults_and_errors() {
        let a = args("--assets A").unwrap();
        assert_eq!(
            (a.speed, a.offset, a.autoplay, a.windowed),
            (None, None, false, false)
        );
        assert_eq!((a.chart, a.save), (None, None));
        assert_eq!((a.data, a.portable), (None, false));
        // Everything is optional (portable layout next to the executable).
        assert_eq!(args("").unwrap().assets, None);
        assert_eq!(args("--chart c.mer").unwrap().assets, None);
        assert_eq!(args("--assets"), Err(ArgsError::Usage));
        assert!(matches!(
            args("--assets A --chart c --bogus"),
            Err(ArgsError::Unknown(_))
        ));
        assert!(matches!(
            args("--assets A --chart c --speed 9"),
            Err(ArgsError::Value { .. })
        ));
        assert!(matches!(
            args("--assets A --chart c --speed x"),
            Err(ArgsError::Value { .. })
        ));
        assert!(matches!(
            args("--assets A --chart c --difficulty easy"),
            Err(ArgsError::Value { .. })
        ));
    }

    #[test]
    fn portable_and_setup_flags() {
        let a = args("--portable --screen setup --setup-source C:/G --setup-start").unwrap();
        assert!(a.portable && a.setup_start);
        assert_eq!(a.screen, Some(Screen::Setup));
        assert_eq!(a.setup_source, Some(PathBuf::from("C:/G")));
    }

    #[test]
    fn resolves_chart_file_with_suffix_difficulty() {
        let a = args("--assets A --chart MusicData/S00-003/S00-003_02.mer").unwrap();
        let c = resolve_chart(&a, Path::new("A"), a.chart.as_deref().unwrap()).unwrap();
        assert_eq!(
            c.path,
            Path::new("A").join("MusicData/S00-003/S00-003_02.mer")
        );
        assert_eq!(c.folder.as_deref(), Some("S00-003"));
        assert_eq!(c.difficulty, Some(Difficulty::Expert));
    }

    #[test]
    fn resolves_song_folder_and_difficulty() {
        let a = args("--assets A --chart MusicData/S03-011 --difficulty inferno").unwrap();
        let c = resolve_chart(&a, Path::new("A"), a.chart.as_deref().unwrap()).unwrap();
        assert_eq!(
            c.path,
            Path::new("A")
                .join("MusicData/S03-011")
                .join("S03-011_03.mer")
        );
        assert_eq!(c.folder.as_deref(), Some("S03-011"));
        assert_eq!(c.difficulty, Some(Difficulty::Inferno));
        let no_diff = args("--assets A --chart MusicData/S03-011").unwrap();
        assert!(matches!(
            resolve_chart(&no_diff, Path::new("A"), Path::new("MusicData/S03-011")),
            Err(ArgsError::NeedsDifficulty(_))
        ));
    }
}
