//! One-time converter: `assets/` -> `data/`.
//!
//! Usage: `wacca-convert --assets <dir> --out <dir> [--textures] [--fonts] [--meshes] [--export-ui <subdir>]...`.
//! Always writes `<out>/songs.json`; `--textures` writes `<out>/jackets/<JacketAssetName>.png` for every
//! catalog song, `--fonts` copies `UI/Font/*.ufont` to `<out>/fonts/`, and each `--export-ui <subdir>`
//! (relative to the assets dir, e.g. `UI/Textures/FONT` or `Vfx`) writes every Texture2D below it to
//! `<out>/ui/<path relative to assets>.png`. `--meshes` writes the note meshes (`Vfx/Mesh/Notes`) as
//! `<out>/meshes/<name>.obj`.
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod catalog;
mod fonts;
mod meshes;
mod textures;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use wacca_game::Difficulty;

const USAGE: &str = "usage: wacca-convert --assets <dir> --out <dir> [--textures] [--fonts] [--meshes] [--export-ui <subdir under assets>]...";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{USAGE}")]
    Usage,
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{table}: {source}")]
    Table {
        table: &'static str,
        source: wacca_ue4::Ue4Error,
    },
    #[error("serialising songs.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{}: {source}", path.display())]
    Texture {
        path: PathBuf,
        source: wacca_ue4::Ue4Error,
    },
    #[error("encoding PNG: {0}")]
    Png(#[from] png::EncodingError),
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    assets: PathBuf,
    out: PathBuf,
    textures: bool,
    fonts: bool,
    meshes: bool,
    export_ui: Vec<PathBuf>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, Error> {
    let (mut assets, mut out) = (None, None);
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().map(PathBuf::from).ok_or(Error::Usage);
        match flag.as_str() {
            "--assets" => assets = Some(value()?),
            "--out" => out = Some(value()?),
            "--textures" => parsed.textures = true,
            "--fonts" => parsed.fonts = true,
            "--meshes" => parsed.meshes = true,
            "--export-ui" => {
                let dir = value()?;
                if !textures::is_plain_relative(&dir) {
                    return Err(Error::Usage);
                }
                parsed.export_ui.push(dir);
            }
            _ => return Err(Error::Usage),
        }
    }
    match (assets, out) {
        (Some(assets), Some(out)) => Ok(Args {
            assets,
            out,
            ..parsed
        }),
        _ => Err(Error::Usage),
    }
}

/// [`Error::Io`] for an IO error on `path`.
fn io_err(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// Writes `data` to `path`, creating parent directories.
fn write(path: &Path, data: &[u8]) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_err(path))?;
    }
    std::fs::write(path, data).map_err(io_err(path))
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(io_err(path))
}

fn read_table(assets: &Path, table: &'static str) -> Result<wacca_ue4::DataTable, Error> {
    let base = assets.join("Table").join(table);
    let uasset = read(&base.with_extension("uasset"))?;
    let uexp = read(&base.with_extension("uexp"))?;
    wacca_ue4::read_datatable(&uasset, &uexp).map_err(|source| Error::Table { table, source })
}

fn run(args: &Args) -> Result<(), Error> {
    let table = read_table(&args.assets, "MusicParameterTable")?;
    let songs = catalog::songs_from_table(&table, |p| args.assets.join(p).is_file());
    let inferno = songs
        .iter()
        .filter(|s| s.chart(Difficulty::Inferno).is_some())
        .count();
    let path = args.out.join("songs.json");
    write(&path, &serde_json::to_vec_pretty(&songs)?)?;
    log::info!(
        "wrote {} ({} songs, {inferno} with Inferno)",
        path.display(),
        songs.len()
    );
    if args.textures {
        let start = Instant::now();
        let jackets: std::collections::BTreeSet<&str> =
            songs.iter().map(|s| s.jacket.as_str()).collect();
        let jobs: Vec<_> = jackets
            .into_iter()
            .filter(|j| textures::is_plain_relative(Path::new(j)))
            .map(|j| textures::jacket_job(&args.assets, &args.out, j))
            .collect();
        let summary = textures::export_all(&jobs);
        log::info!("jackets: {summary} in {:.1?}", start.elapsed());
    }
    if args.fonts {
        let (counts, failed) = fonts::copy_fonts(&args.assets, &args.out)?;
        log::info!("fonts: {counts:?}, {failed} failed");
    }
    if args.meshes {
        let (written, failed) = meshes::export_notes(&args.assets, &args.out)?;
        log::info!("meshes: {written} written, {failed} failed");
    }
    for subdir in &args.export_ui {
        let start = Instant::now();
        let jobs = textures::dir_jobs(&args.assets, subdir, &args.out)?;
        let summary = textures::export_all(&jobs);
        log::info!("{}: {summary} in {:.1?}", subdir.display(), start.elapsed());
    }
    Ok(())
}

/// Minimal stderr logger (info and above).
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> ExitCode {
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    match parse_args(std::env::args().skip(1)).and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wacca-convert: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<Args, Error> {
        parse_args(s.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn parses_flags_in_any_order() {
        let expected = Args {
            assets: "a".into(),
            out: "o".into(),
            ..Args::default()
        };
        assert_eq!(args("--assets a --out o").unwrap(), expected);
        assert_eq!(args("--out o --assets a").unwrap(), expected);
    }

    #[test]
    fn parses_texture_font_and_export_flags() {
        let a =
            args("--textures --assets a --export-ui UI/Textures --fonts --meshes --out o --export-ui Vfx")
                .unwrap();
        assert!(a.textures && a.fonts && a.meshes);
        assert_eq!(a.export_ui, [PathBuf::from("UI/Textures"), "Vfx".into()]);
        assert!(matches!(
            args("--assets a --out o --export-ui ../x"),
            Err(Error::Usage)
        ));
        assert!(matches!(
            args("--assets a --out o --export-ui"),
            Err(Error::Usage)
        ));
    }

    #[test]
    fn rejects_missing_or_unknown_flags() {
        assert!(matches!(args(""), Err(Error::Usage)));
        assert!(matches!(args("--assets a"), Err(Error::Usage)));
        assert!(matches!(args("--assets a --out"), Err(Error::Usage)));
        assert!(matches!(
            args("--assets a --out o --x y"),
            Err(Error::Usage)
        ));
    }

    #[test]
    #[ignore = "needs WACCA_ASSETS"]
    fn real_catalog_counts() {
        let assets = PathBuf::from(std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS"));
        let table = read_table(&assets, "MusicParameterTable").unwrap();
        let songs = catalog::songs_from_table(&table, |p| assets.join(p).is_file());
        let inferno = songs
            .iter()
            .filter(|s| s.chart(Difficulty::Inferno).is_some())
            .count();
        let charts: usize = songs.iter().map(|s| s.charts.len()).sum();
        assert_eq!((songs.len(), inferno, charts), (395, 26, 395 * 3 + 26));
    }
}
