//! Command line: which chart to play and how. Pure (no filesystem access).

use std::path::{Path, PathBuf};

use wacca_chart::Difficulty;

pub const USAGE: &str = "usage: wacca-app --assets <dir> --chart <MusicData/<song>/<song>_NN.mer | MusicData/<song>> \
[--difficulty normal|hard|expert|inferno] [--autoplay] [--speed 1.0..6.0] [--offset-ms N] [--data <dir>]";

/// Default note speed (the save file's default hi-speed).
pub const DEFAULT_SPEED: f64 = 4.0;
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
    pub assets: PathBuf,
    /// `.mer` file or song folder, relative to `assets` unless absolute.
    pub chart: PathBuf,
    pub difficulty: Option<Difficulty>,
    pub autoplay: bool,
    pub speed: f64,
    /// User timing offset in seconds, added to chart time.
    pub offset: f64,
    /// Directory with `songs.json` (from `wacca-convert`).
    pub data: PathBuf,
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
    let (mut assets, mut chart, mut diff) = (None, None, None);
    let mut out = Args {
        assets: PathBuf::new(),
        chart: PathBuf::new(),
        difficulty: None,
        autoplay: false,
        speed: DEFAULT_SPEED,
        offset: 0.0,
        data: PathBuf::from("data"),
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--assets" => assets = Some(value::<PathBuf>("--assets", args.next())?),
            "--chart" => chart = Some(value::<PathBuf>("--chart", args.next())?),
            "--data" => out.data = value("--data", args.next())?,
            "--difficulty" => diff = Some(value::<String>("--difficulty", args.next())?),
            "--autoplay" => out.autoplay = true,
            "--speed" => {
                out.speed = value("--speed", args.next())?;
                if !SPEED_RANGE.contains(&out.speed) {
                    return Err(ArgsError::Value {
                        flag: "--speed",
                        value: out.speed.to_string(),
                    });
                }
            }
            "--offset-ms" => {
                let ms: f64 = value("--offset-ms", args.next())?;
                if !ms.is_finite() {
                    return Err(ArgsError::Value {
                        flag: "--offset-ms",
                        value: ms.to_string(),
                    });
                }
                out.offset = ms / 1000.0;
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
    match (assets, chart) {
        (Some(a), Some(c)) => {
            out.assets = a;
            out.chart = c;
            Ok(out)
        }
        _ => Err(ArgsError::Usage),
    }
}

/// What the chart path says about the song.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRef {
    pub path: PathBuf,
    /// Song folder name, e.g. `S00-003` (= `Song::folder`).
    pub folder: Option<String>,
    pub difficulty: Option<Difficulty>,
}

/// Resolve `--chart` (+ `--difficulty`) to a `.mer` path: a song folder becomes
/// `<folder>/<folder>_NN.mer`; a file's difficulty comes from its `_NN` suffix unless given.
pub fn resolve_chart(args: &Args) -> Result<ChartRef, ArgsError> {
    let path = args.assets.join(&args.chart);
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
        let a = args("--assets A --chart c.mer --autoplay --speed 2.5 --offset-ms -20 --data D --difficulty hard").unwrap();
        assert_eq!(a.assets, PathBuf::from("A"));
        assert_eq!(a.chart, PathBuf::from("c.mer"));
        assert!(a.autoplay);
        assert_eq!(a.speed, 2.5);
        assert!((a.offset + 0.020).abs() < 1e-12);
        assert_eq!(a.data, PathBuf::from("D"));
        assert_eq!(a.difficulty, Some(Difficulty::Hard));
    }

    #[test]
    fn defaults_and_errors() {
        let a = args("--chart x.mer --assets A").unwrap();
        assert_eq!((a.speed, a.offset, a.autoplay), (DEFAULT_SPEED, 0.0, false));
        assert_eq!(a.data, PathBuf::from("data"));
        assert_eq!(args("--assets A"), Err(ArgsError::Usage));
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
    fn resolves_chart_file_with_suffix_difficulty() {
        let a = args("--assets A --chart MusicData/S00-003/S00-003_02.mer").unwrap();
        let c = resolve_chart(&a).unwrap();
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
        let c = resolve_chart(&a).unwrap();
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
            resolve_chart(&no_diff),
            Err(ArgsError::NeedsDifficulty(_))
        ));
    }
}
