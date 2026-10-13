//! One-time converter: `assets/` -> `data/` (library; the `wacca-convert` binary and `wacca-setup`
//! call [`convert`]).
//!
//! Always writes `<out>/songs.json`; [`Options::textures`] writes `<out>/jackets/<JacketAssetName>.png`
//! for every catalog song, [`Options::fonts`] copies `UI/Font/*.ufont` to `<out>/fonts/`, and each
//! [`Options::export_ui`] entry (relative to the assets dir, e.g. `UI/Textures/FONT` or `Vfx`) writes
//! every Texture2D below it to `<out>/ui/<path relative to assets>.png`. [`Options::meshes`] writes the
//! note meshes (`Vfx/Mesh/Notes`) as `<out>/meshes/<name>.obj`; [`Options::effects`] writes the
//! hit-effect particle systems, their materials and meshes (`effects.rs`).
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

mod catalog;
mod effects;
mod fonts;
mod meshes;
mod textures;

use std::path::{Path, PathBuf};
use std::time::Instant;

use wacca_game::Difficulty;
use wacca_game::catalog::Song;

pub use textures::{Summary as TextureSummary, is_plain_relative};

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    #[error("serialising JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{}: {source}", path.display())]
    Texture {
        path: PathBuf,
        source: wacca_ue4::Ue4Error,
    },
    #[error("encoding PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("cancelled")]
    Cancelled,
}

/// Which outputs [`convert`] writes besides `songs.json`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Options {
    pub textures: bool,
    pub fonts: bool,
    pub meshes: bool,
    pub effects: bool,
    /// Directories below the assets dir whose textures are exported to `<out>/ui/`.
    pub export_ui: Vec<PathBuf>,
}

impl Options {
    /// Everything the game (`lily`) reads from `data/`.
    pub fn full() -> Self {
        Options {
            textures: true,
            fonts: true,
            meshes: true,
            effects: true,
            export_ui: vec!["UI/Textures".into(), "Vfx/Textures".into()],
        }
    }
}

/// Progress of one conversion step; `done`/`total` count files (0/0 when unknown).
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub step: String,
    pub done: usize,
    pub total: usize,
}

/// What [`convert`] wrote.
#[derive(Debug, Default)]
pub struct Report {
    pub songs: usize,
    pub jackets: TextureSummary,
    pub fonts: usize,
    /// `(written, failed)` note meshes.
    pub meshes: (usize, usize),
    /// `(systems, materials, meshes)`.
    pub effects: (usize, usize, usize),
    /// Per [`Options::export_ui`] directory.
    pub ui: Vec<(PathBuf, TextureSummary)>,
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

/// The song catalog from `<assets>/Table/MusicParameterTable` (charts whose `.mer` is missing are
/// left out; rows with malformed fields are skipped with a warning).
pub fn read_songs(assets: &Path) -> Result<Vec<Song>, Error> {
    let table = read_table(assets, "MusicParameterTable")?;
    Ok(catalog::songs_from_table(&table, |p| {
        assets.join(p).is_file()
    }))
}

/// Converts `assets` into `out`. `progress` is called before each step and while textures are
/// exported; returning `false` cancels ([`Error::Cancelled`], with partial output left in `out`).
pub fn convert(
    assets: &Path,
    out: &Path,
    opts: &Options,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Report, Error> {
    let mut step = |name: &str, done: usize, total: usize| {
        let p = Progress {
            step: name.to_owned(),
            done,
            total,
        };
        if progress(p) {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    };
    let mut report = Report::default();
    step("songs", 0, 0)?;
    let songs = read_songs(assets)?;
    let inferno = songs
        .iter()
        .filter(|s| s.chart(Difficulty::Inferno).is_some())
        .count();
    let path = out.join("songs.json");
    write(&path, &serde_json::to_vec_pretty(&songs)?)?;
    report.songs = songs.len();
    log::info!(
        "wrote {} ({} songs, {inferno} with Inferno)",
        path.display(),
        songs.len()
    );
    if opts.textures {
        let start = Instant::now();
        let jackets: std::collections::BTreeSet<&str> =
            songs.iter().map(|s| s.jacket.as_str()).collect();
        let jobs: Vec<_> = jackets
            .into_iter()
            .filter(|j| textures::is_plain_relative(Path::new(j)))
            .map(|j| textures::jacket_job(assets, out, j))
            .collect();
        report.jackets = textures::export_all(&jobs, &mut |d, t| step("jackets", d, t).is_ok())
            .ok_or(Error::Cancelled)?;
        log::info!("jackets: {} in {:.1?}", report.jackets, start.elapsed());
    }
    if opts.fonts {
        step("fonts", 0, 0)?;
        let (counts, failed) = fonts::copy_fonts(assets, out)?;
        report.fonts = counts.values().sum();
        log::info!("fonts: {counts:?}, {failed} failed");
    }
    if opts.meshes {
        step("meshes", 0, 0)?;
        report.meshes = meshes::export_notes(assets, out)?;
        log::info!(
            "meshes: {} written, {} failed",
            report.meshes.0,
            report.meshes.1
        );
    }
    if opts.effects {
        step("effects", 0, 0)?;
        report.effects = effects::export(assets, out)?;
        let (systems, materials, meshes) = report.effects;
        log::info!("effects: {systems} systems, {materials} materials, {meshes} meshes");
    }
    for subdir in &opts.export_ui {
        let start = Instant::now();
        let name = subdir.display().to_string();
        step(&name, 0, 0)?;
        let jobs = textures::dir_jobs(assets, subdir, out)?;
        let summary = textures::export_all(&jobs, &mut |d, t| step(&name, d, t).is_ok())
            .ok_or(Error::Cancelled)?;
        log::info!("{name}: {summary} in {:.1?}", start.elapsed());
        report.ui.push((subdir.clone(), summary));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_table_is_an_io_error_and_cancel_stops_first_step() {
        let dir = Path::new("does-not-exist");
        assert!(matches!(
            convert(dir, dir, &Options::default(), &mut |_| true),
            Err(Error::Io { .. })
        ));
        let mut steps = Vec::new();
        let r = convert(dir, dir, &Options::full(), &mut |p| {
            steps.push(p.step);
            false
        });
        assert!(matches!(r, Err(Error::Cancelled)));
        assert_eq!(steps, ["songs"]);
    }

    #[test]
    #[ignore = "needs WACCA_ASSETS"]
    fn real_catalog_counts() {
        let assets = PathBuf::from(std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS"));
        let songs = read_songs(&assets).unwrap();
        let inferno = songs
            .iter()
            .filter(|s| s.chart(Difficulty::Inferno).is_some())
            .count();
        let charts: usize = songs.iter().map(|s| s.charts.len()).sum();
        assert_eq!((songs.len(), inferno, charts), (395, 26, 395 * 3 + 26));
    }
}
