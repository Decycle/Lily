//! First-run import: turns the user's own WACCA Reverse files into a portable game dir
//! (`<dest>/assets` = the minimal runtime subset, `<dest>/data` = `wacca-convert` output).
//!
//! Flow: [`detect`] a user-picked folder → optionally [`verify`] archive checksums → [`run`] a
//! [`Plan`]. Two sources:
//! - the nine `WACCA Reverse (3.xx.xx).7z` archives (base + updates), extracted in version order
//!   with 7-Zip's command line (only the read set, see [`readset`]), later archives overwriting
//!   earlier ones;
//! - an unpacked (possibly modded) game: its `Content` dir is converted in place (read-only) and
//!   the runtime subset copied.
//!
//! Cancellation: every callback returns `bool`; `false` cancels (the call returns
//! [`SetupError::Cancelled`] and leaves no partial output; an existing `<dest>/assets`/`data`
//! is only replaced after a fully successful run).
#![cfg_attr(test, allow(clippy::unwrap_used))] // tests may unwrap (CONTRIBUTING)

pub mod archives;
mod detect;
pub mod readset;
pub mod sevenzip;
pub mod space;
pub mod validate;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use archives::verify;

use readset::{ARCHIVE_CONTENT, Use};

/// One recognised archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: PathBuf,
    /// `3.00.00` … `3.07.01`.
    pub version: String,
    /// Actual size on disk.
    pub size: u64,
    /// Expected md5 (lower-case hex).
    pub md5: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The archives found, in version order (base first).
    Archives(Vec<ArchiveFile>),
    /// An unpacked game's `Content` directory.
    Installed { content_dir: PathBuf },
}

/// Something the user should know before [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// These update versions were not found; the result is the base plus the updates present.
    MissingUpdates(Vec<String>),
    /// Archive checksums are not verified yet ([`verify`] reads every archive once).
    ChecksumNotVerified,
    /// An archive recognised by its exact size although its name differs.
    Renamed { archive: PathBuf, version: String },
    /// Name matches but the size doesn't: probably an incomplete or damaged file.
    SizeMismatch {
        archive: PathBuf,
        expected: u64,
        actual: u64,
    },
    /// Background movies available, and their total size (for [`Plan::include_movies`]).
    Movies { present: bool, bytes: u64 },
    /// Number of playable songs in an installed game's song table.
    Songs(usize),
    /// The installed game's version could not be determined (it may still work).
    UnknownVersion,
    /// The song table could not be used: [`run`] will fail.
    UnsupportedVersion(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub source: Source,
    /// Archives: the newest archive's version (`3.07.01`). Installed: `3.07` from the BGM banks.
    pub version: Option<String>,
    pub report: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Also copy/extract `Movie/*.usm` (≈ 21 GB).
    pub include_movies: bool,
    /// The portable game dir: [`run`] writes `<dest>/assets` and `<dest>/data`.
    pub dest: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// md5 of one archive ([`verify`]); bytes.
    Verify,
    /// Listing archives / scanning the game; items.
    Prepare,
    /// 7-Zip extraction; bytes of archive (estimated from its percentage).
    Extract,
    /// Copying (installed) or moving (archives) the runtime subset; bytes.
    Copy,
    /// `wacca-convert`; files of the current step (`detail` names it), 0/0 when unknown.
    Convert,
    /// Parsing every chart; songs.
    Validate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
    /// Archive version, converter step, …
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedChart {
    pub song: String,
    /// Relative to `<dest>/assets`.
    pub chart_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub songs_playable: usize,
    pub charts_playable: usize,
    /// Songs left out because none of their charts is playable.
    pub songs_skipped: usize,
    pub charts_skipped: Vec<SkippedChart>,
    /// Distinct background movies referenced by playable charts that exist / are missing
    /// (both 0 without movies).
    pub movies_present: usize,
    pub movies_missing: usize,
    pub warnings: Vec<String>,
    pub assets_bytes: u64,
    pub data_bytes: u64,
    /// Wall time per phase, in order.
    pub timings: Vec<(Phase, Duration)>,
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "no WACCA Reverse archives or game files found in {} (looking for `WACCA Reverse (3.00.00).7z` or a folder with Table/MusicParameterTable and Sound/Bgm)",
        .0.display()
    )]
    NothingFound(PathBuf),
    #[error(
        "only update archives were found: the base archive `WACCA Reverse (3.00.00).7z` is needed too"
    )]
    BaseArchiveMissing,
    #[error(
        "7-Zip was not found: install it, or put 7z.exe + 7z.dll (or 7zr.exe) in the `tools` folder next to the app"
    )]
    SevenZipMissing,
    #[error("7-Zip, {}: {message}", archive.display())]
    SevenZip { archive: PathBuf, message: String },
    #[error("{} does not contain the game's Content folder", .0.display())]
    UnexpectedArchive(PathBuf),
    #[error(
        "not enough disk space on {}: {:.1} GB needed, {:.1} GB free",
        path.display(),
        *needed as f64 / 1e9,
        *available as f64 / 1e9
    )]
    DiskSpace {
        path: PathBuf,
        needed: u64,
        available: u64,
    },
    #[error("the destination {} overlaps the game files", .0.display())]
    InvalidDestination(PathBuf),
    #[error("cancelled")]
    Cancelled,
    #[error("a required game file is missing or broken: {0}")]
    MissingCore(String),
    #[error("unsupported game version: {0}")]
    UnsupportedVersion(String),
    #[error("conversion failed: {0}")]
    Convert(wacca_convert::Error),
}

impl From<wacca_convert::Error> for SetupError {
    fn from(e: wacca_convert::Error) -> Self {
        match e {
            wacca_convert::Error::Cancelled => SetupError::Cancelled,
            e @ wacca_convert::Error::Table { .. } => SetupError::UnsupportedVersion(e.to_string()),
            e => SetupError::Convert(e),
        }
    }
}

/// [`SetupError::Io`] for an IO error on `path`.
fn io_err(path: &Path) -> impl Fn(std::io::Error) -> SetupError + '_ {
    move |source| SetupError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Scans a user-picked folder: the Reverse `.7z` archives (by name, or by exact size if renamed)
/// in it or one level down, else the shallowest folder (up to 5 levels down,
/// the folder itself included) that contains `Table/MusicParameterTable.uasset+.uexp` and a
/// `Sound/Bgm/*.awb` — e.g. `<pick>/WindowsNoEditor/Mercury/Content`, `<pick>/App/...`, or the
/// `Content` dir itself. Reads only file names, sizes and the song table.
pub fn detect(path: &Path) -> Result<Detected, SetupError> {
    detect::detect(path)
}

/// Temporary layout below `<dest>/.setup-tmp`.
struct Work {
    root: PathBuf,
    extract: PathBuf,
    assets: PathBuf,
    data: PathBuf,
}

impl Work {
    fn new(dest: &Path) -> Self {
        let root = dest.join(".setup-tmp");
        Work {
            extract: root.join("extract"),
            assets: root.join("assets"),
            data: root.join("data"),
            root,
        }
    }
}

fn remove_dir(path: &Path) -> Result<(), SetupError> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err(path)(e)),
        _ => Ok(()),
    }
}

/// Bytes of all files below `dir`.
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_bytes(&e.path()),
                    _ => e.metadata().map_or(0, |m| m.len()),
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Imports `source` into `plan.dest` (see the crate docs). Steps: (archives) list + disk-space
/// check + extract the read set in version order into `<dest>/.setup-tmp`; (installed) disk-space
/// check; then convert to `data`, move/copy the runtime subset to `assets`, validate, and only
/// then replace `<dest>/assets` and `<dest>/data`. `progress` returning `false` cancels.
pub fn run(
    source: &Source,
    plan: &Plan,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Summary, SetupError> {
    let work = Work::new(&plan.dest);
    if let Source::Installed { content_dir } = source {
        let overlaps = |p: &Path| content_dir.starts_with(p) || p.starts_with(content_dir);
        if [
            &work.root,
            &plan.dest.join("assets"),
            &plan.dest.join("data"),
        ]
        .iter()
        .any(|p| overlaps(p))
        {
            return Err(SetupError::InvalidDestination(plan.dest.clone()));
        }
    }
    remove_dir(&work.root)?;
    std::fs::create_dir_all(&work.root).map_err(io_err(&work.root))?;
    let result = run_in(source, plan, &work, progress);
    let cleanup = remove_dir(&work.root);
    let summary = result?;
    if let Err(e) = cleanup {
        log::warn!("could not remove the temporary folder: {e}");
    }
    Ok(summary)
}

fn run_in(
    source: &Source,
    plan: &Plan,
    work: &Work,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Summary, SetupError> {
    let mut timings = Vec::new();
    let mut clock = Instant::now();
    let mut lap = |phase, timings: &mut Vec<(Phase, Duration)>| {
        timings.push((phase, clock.elapsed()));
        clock = Instant::now();
    };
    let content = match source {
        Source::Archives(archives) => {
            let content = extract_archives(archives, plan, work, progress)?;
            lap(Phase::Extract, &mut timings);
            content
        }
        Source::Installed { content_dir } => {
            let needed = runtime_bytes(content_dir, plan.include_movies)? + space::DATA_BYTES;
            check_space(&plan.dest, needed)?;
            content_dir.clone()
        }
    };
    convert(&content, &work.data, progress)?;
    lap(Phase::Convert, &mut timings);
    let mv = matches!(source, Source::Archives(_));
    transfer(&content, &work.assets, plan.include_movies, mv, progress)?;
    lap(Phase::Copy, &mut timings);
    let step = Progress {
        phase: Phase::Validate,
        done: 0,
        total: 0,
        detail: String::new(),
    };
    if !progress(step) {
        return Err(SetupError::Cancelled);
    }
    let mut summary = validate::validate(&work.assets, &work.data, plan.include_movies)?;
    lap(Phase::Validate, &mut timings);
    for name in ["assets", "data"] {
        let (new, dst) = (work.root.join(name), plan.dest.join(name));
        let old = work.root.join(format!("{name}.old"));
        if dst.exists() {
            std::fs::rename(&dst, &old).map_err(io_err(&dst))?;
        }
        std::fs::rename(&new, &dst).map_err(io_err(&dst))?;
    }
    summary.assets_bytes = dir_bytes(&plan.dest.join("assets"));
    summary.data_bytes = dir_bytes(&plan.dest.join("data"));
    summary.timings = timings;
    Ok(summary)
}

fn check_space(dest: &Path, needed: u64) -> Result<(), SetupError> {
    match space::free_bytes(dest) {
        Some(free) => space::check(dest, needed, free),
        None => {
            log::warn!("free space on {} unknown; not checked", dest.display());
            Ok(())
        }
    }
}

fn runtime_bytes(content: &Path, movies: bool) -> Result<u64, SetupError> {
    Ok(readset::collect(content, movies)?
        .iter()
        .filter(|e| e.used != Use::Convert)
        .map(|e| e.size)
        .sum())
}

/// Lists, checks space, extracts every archive in order; returns the staged `Content` dir.
fn extract_archives(
    archives: &[ArchiveFile],
    plan: &Plan,
    work: &Work,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<PathBuf, SetupError> {
    let exe = sevenzip::find()?;
    log::info!("7-Zip: {}", exe.display());
    let mut listings = Vec::new();
    for (i, a) in archives.iter().enumerate() {
        let p = Progress {
            phase: Phase::Prepare,
            done: i as u64,
            total: archives.len() as u64,
            detail: a.version.clone(),
        };
        if !progress(p) {
            return Err(SetupError::Cancelled);
        }
        let list = sevenzip::list(&exe, &a.path)?;
        let prefix = format!("{}\\", ARCHIVE_CONTENT.to_ascii_lowercase());
        if !list
            .iter()
            .any(|e| e.path.to_ascii_lowercase().starts_with(&prefix))
        {
            return Err(SetupError::UnexpectedArchive(a.path.clone()));
        }
        listings.push(list);
    }
    let needed = space::staged_bytes(&listings, plan.include_movies) + space::DATA_BYTES;
    check_space(&plan.dest, needed)?;
    let includes = readset::seven_zip_includes(plan.include_movies);
    let total: u64 = archives.iter().map(|a| a.size).sum();
    let mut before = 0;
    for a in archives {
        log::info!("extracting {}", a.path.display());
        let mut percent = |pct: u8| {
            progress(Progress {
                phase: Phase::Extract,
                done: before + a.size * u64::from(pct) / 100,
                total,
                detail: a.version.clone(),
            })
        };
        sevenzip::extract(&exe, &a.path, &work.extract, &includes, &mut percent)?;
        before += a.size;
    }
    let content = ARCHIVE_CONTENT
        .split('\\')
        .fold(work.extract.clone(), |p, c| p.join(c));
    if !readset::is_content_dir(&content) {
        return Err(SetupError::MissingCore(
            "the archives contain no song table or BGM bank".into(),
        ));
    }
    Ok(content)
}

fn convert(
    content: &Path,
    out: &Path,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<(), SetupError> {
    let report = wacca_convert::convert(content, out, &wacca_convert::Options::full(), &mut |p| {
        progress(Progress {
            phase: Phase::Convert,
            done: p.done as u64,
            total: p.total as u64,
            detail: p.step,
        })
    })?;
    log::info!("converted: {report:?}");
    if report.meshes.0 == 0 {
        return Err(SetupError::MissingCore(
            "no note meshes (Vfx/Mesh/Notes)".into(),
        ));
    }
    Ok(())
}

/// Copies (installed) or moves (staged archives) the runtime subset of `content` into `assets`.
fn transfer(
    content: &Path,
    assets: &Path,
    movies: bool,
    mv: bool,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<(), SetupError> {
    let files: Vec<_> = readset::collect(content, movies)?
        .into_iter()
        .filter(|e| e.used != Use::Convert)
        .collect();
    let total = files.iter().map(|e| e.size).sum();
    let mut done = 0;
    for e in files {
        let p = Progress {
            phase: Phase::Copy,
            done,
            total,
            detail: e.rel.display().to_string(),
        };
        if !progress(p) {
            return Err(SetupError::Cancelled);
        }
        let (src, dst) = (content.join(&e.rel), assets.join(&e.rel));
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        if mv {
            std::fs::rename(&src, &dst).map_err(io_err(&src))?;
        } else {
            std::fs::copy(&src, &dst).map_err(io_err(&src))?;
        }
        done += e.size;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh, empty temp dir unique to this process and call.
    pub fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("wacca-setup-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Creates empty files (and their parents) below `root`.
    pub fn touch_all(root: &Path, files: &[&str]) {
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
    }

    #[test]
    fn transfer_copies_only_the_runtime_subset() {
        let root = temp_dir("transfer");
        let content = root.join("Content");
        touch_all(
            &content,
            &[
                "MusicData/S/S_00.mer",
                "Sound/Bgm/MER_BGM.awb",
                "Sound/Bgm/MER_BGM_S00_001.uexp",
                "UI/Textures/a.uasset",
                "Movie/mv.usm",
            ],
        );
        let assets = root.join("assets");
        let mut seen = 0;
        transfer(&content, &assets, false, false, &mut |_| {
            seen += 1;
            true
        })
        .unwrap();
        assert_eq!(seen, 2);
        assert!(assets.join("MusicData/S/S_00.mer").is_file());
        assert!(assets.join("Sound/Bgm/MER_BGM.awb").is_file());
        assert!(!assets.join("UI").exists() && !assets.join("Movie").exists());
        assert!(
            content.join("MusicData/S/S_00.mer").is_file(),
            "copy keeps the source"
        );
        assert!(matches!(
            transfer(&content, &assets, false, false, &mut |_| false),
            Err(SetupError::Cancelled)
        ));
        transfer(&content, &root.join("moved"), true, true, &mut |_| true).unwrap();
        assert!(
            root.join("moved/Movie/mv.usm").is_file() && !content.join("Movie/mv.usm").exists()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_rejects_overlap_and_cleans_up_on_failure() {
        let root = temp_dir("run");
        let content = root.join("dest/assets");
        let plan = Plan {
            include_movies: false,
            dest: root.join("dest"),
        };
        let src = Source::Installed {
            content_dir: content.clone(),
        };
        assert!(matches!(
            run(&src, &plan, &mut |_| true),
            Err(SetupError::InvalidDestination(_))
        ));
        // A game dir without convertible files: fails in conversion, leaves no temp dir and no
        // assets/data behind.
        let game = root.join("game");
        touch_all(
            &game,
            &[
                "Table/MusicParameterTable.uasset",
                "Table/MusicParameterTable.uexp",
                "Sound/Bgm/a.awb",
            ],
        );
        let src = Source::Installed { content_dir: game };
        let err = run(&src, &plan, &mut |_| true).unwrap_err();
        assert!(matches!(err, SetupError::UnsupportedVersion(_)), "{err}");
        assert!(!plan.dest.join(".setup-tmp").exists());
        assert!(!plan.dest.join("assets").exists() && !plan.dest.join("data").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn convert_errors_map_to_setup_errors() {
        let e: SetupError = wacca_convert::Error::Cancelled.into();
        assert!(matches!(e, SetupError::Cancelled));
        let e: SetupError = wacca_convert::Error::Table {
            table: "MusicParameterTable",
            source: wacca_ue4_error(),
        }
        .into();
        assert!(
            e.to_string()
                .starts_with("unsupported game version: MusicParameterTable")
        );
    }

    fn wacca_ue4_error() -> wacca_ue4::Ue4Error {
        wacca_ue4::read_datatable(&[], &[]).unwrap_err()
    }
}
