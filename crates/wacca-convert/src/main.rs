//! One-time converter: `assets/` -> `data/`.
//!
//! Usage: `wacca-convert --assets <dir> --out <dir>`. Writes `<out>/songs.json`.
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod catalog;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use wacca_game::Difficulty;

const USAGE: &str = "usage: wacca-convert --assets <dir> --out <dir>";

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
}

#[derive(Debug, PartialEq)]
struct Args {
    assets: PathBuf,
    out: PathBuf,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, Error> {
    let (mut assets, mut out) = (None, None);
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--assets" => &mut assets,
            "--out" => &mut out,
            _ => return Err(Error::Usage),
        };
        *slot = Some(PathBuf::from(args.next().ok_or(Error::Usage)?));
    }
    match (assets, out) {
        (Some(assets), Some(out)) => Ok(Args { assets, out }),
        _ => Err(Error::Usage),
    }
}

fn read(path: PathBuf) -> Result<Vec<u8>, Error> {
    std::fs::read(&path).map_err(|source| Error::Io { path, source })
}

fn read_table(assets: &Path, table: &'static str) -> Result<wacca_ue4::DataTable, Error> {
    let base = assets.join("Table").join(table);
    let uasset = read(base.with_extension("uasset"))?;
    let uexp = read(base.with_extension("uexp"))?;
    wacca_ue4::read_datatable(&uasset, &uexp).map_err(|source| Error::Table { table, source })
}

fn run(args: &Args) -> Result<(), Error> {
    let table = read_table(&args.assets, "MusicParameterTable")?;
    let songs = catalog::songs_from_table(&table, |p| args.assets.join(p).is_file());
    let inferno = songs
        .iter()
        .filter(|s| s.chart(Difficulty::Inferno).is_some())
        .count();
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(&args.out).map_err(io(&args.out))?;
    let path = args.out.join("songs.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&songs)?).map_err(io(&path))?;
    log::info!(
        "wrote {} ({} songs, {inferno} with Inferno)",
        path.display(),
        songs.len()
    );
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
        };
        assert_eq!(args("--assets a --out o").unwrap(), expected);
        assert_eq!(args("--out o --assets a").unwrap(), expected);
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
